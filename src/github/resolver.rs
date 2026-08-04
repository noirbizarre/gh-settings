//! Slug to identifier resolution.
//!
//! Ruleset `bypass_actors` are declared in the configuration by name
//! (`{ team: engineering }`) because identifiers are neither stable across
//! organisations nor meaningful to a human reading a configuration file. The
//! API, however, wants a numeric `actor_id`.
//!
//! Resolution therefore happens **once, during planning** — in
//! [`Resource::prepare`] — rather than lazily during apply. A misspelled team
//! is reported by `plan`, before anything has been written, instead of aborting
//! an apply halfway through with a stray 404 and some changes already made.
//!
//! The cache exists because a single ruleset commonly names the same team
//! several times, and a fleet of rulesets almost always does; without it each
//! mention costs a round trip.
//!
//! [`Resource::prepare`]: crate::resources::Resource::prepare

use std::collections::HashMap;

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::github::client::{GitHubClient, GitHubClientExt, Request};
use crate::github::{GitHubError, Result};

/// Anything the API returns that we only need the identifier of.
#[derive(Debug, Deserialize)]
struct IdOnly {
    id: u64,
}

/// Caches slug lookups for the duration of a run.
///
/// Deliberately holds no client: the resolver is a cache, and the caller
/// already has a `&dyn GitHubClient` to lend it. That keeps it usable from
/// [`Resource::prepare`](crate::resources::Resource::prepare), which borrows
/// rather than owns its client.
#[derive(Default)]
pub struct Resolver {
    teams: Mutex<HashMap<String, u64>>,
    apps: Mutex<HashMap<String, u64>>,
}

impl Resolver {
    /// An empty resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve an organisation team slug to its identifier.
    ///
    /// A team that does not exist is [`GitHubError::UnresolvedActor`], not a
    /// bare 404 — the message names the slug, which is what the reader needs.
    pub async fn team(&self, client: &dyn GitHubClient, org: &str, slug: &str) -> Result<u64> {
        let key = format!("{org}/{slug}");
        if let Some(id) = self.teams.lock().await.get(&key) {
            return Ok(*id);
        }

        let team: Option<IdOnly> = client
            .send_optional(Request::get(format!("orgs/{org}/teams/{slug}")))
            .await?;

        let id = team
            .ok_or_else(|| GitHubError::UnresolvedActor {
                kind: "team",
                slug: slug.to_string(),
            })?
            .id;

        self.teams.lock().await.insert(key, id);
        Ok(id)
    }

    /// Resolve a GitHub App slug to its identifier.
    pub async fn app(&self, client: &dyn GitHubClient, slug: &str) -> Result<u64> {
        if let Some(id) = self.apps.lock().await.get(slug) {
            return Ok(*id);
        }

        let app: Option<IdOnly> = client
            .send_optional(Request::get(format!("apps/{slug}")))
            .await?;

        let id = app
            .ok_or_else(|| GitHubError::UnresolvedActor {
                kind: "app",
                slug: slug.to_string(),
            })?
            .id;

        self.apps.lock().await.insert(slug.to_string(), id);
        Ok(id)
    }

    /// How many lookups are cached, for tests.
    #[cfg(test)]
    pub(crate) async fn cached(&self) -> usize {
        self.teams.lock().await.len() + self.apps.lock().await.len()
    }
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::Response;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    /// Records every request and replays canned responses.
    #[derive(Default)]
    struct RecordingClient {
        calls: StdMutex<Vec<String>>,
        found: bool,
    }

    #[async_trait]
    impl GitHubClient for RecordingClient {
        async fn request(&self, request: Request) -> Result<Response> {
            self.calls.lock().unwrap().push(request.endpoint.clone());

            if !self.found {
                // Faithful to the real transport, which turns a non-2xx into an
                // `Err`. Returning `Ok` with a 404 status would make
                // `send_optional` miss the not-found case entirely and give the
                // test a false pass.
                return Err(GitHubError::Api {
                    method: crate::github::Method::Get,
                    endpoint: request.endpoint,
                    status: 404,
                    message: "Not Found".into(),
                    body: String::new(),
                });
            }

            Ok(Response::json(200, json!({ "id": 42 }), Vec::new()))
        }
    }

    fn client(found: bool) -> RecordingClient {
        RecordingClient {
            calls: StdMutex::new(Vec::new()),
            found,
        }
    }

    #[tokio::test]
    async fn resolves_a_team_to_its_identifier() {
        let client = client(true);
        let resolver = Resolver::new();

        assert_eq!(resolver.team(&client, "acme", "eng").await.unwrap(), 42);
        assert_eq!(client.calls.lock().unwrap()[0], "orgs/acme/teams/eng");
    }

    #[tokio::test]
    async fn repeated_lookups_cost_one_round_trip() {
        // A ruleset commonly names the same team several times; without the
        // cache each mention would be a request.
        let client = client(true);
        let resolver = Resolver::new();

        for _ in 0..5 {
            resolver.team(&client, "acme", "eng").await.unwrap();
        }

        assert_eq!(client.calls.lock().unwrap().len(), 1);
        assert_eq!(resolver.cached().await, 1);
    }

    #[tokio::test]
    async fn the_cache_is_keyed_by_organisation() {
        // Two organisations can each have a team called `eng`, and they are
        // different teams with different identifiers.
        let client = client(true);
        let resolver = Resolver::new();

        resolver.team(&client, "acme", "eng").await.unwrap();
        resolver.team(&client, "other", "eng").await.unwrap();

        assert_eq!(client.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_missing_team_names_the_slug() {
        // A bare 404 leaves the reader hunting; the error says which team.
        let client = client(false);
        let error = Resolver::new()
            .team(&client, "acme", "typo")
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            GitHubError::UnresolvedActor { kind: "team", .. }
        ));
        assert!(error.to_string().contains("typo"), "{error}");
    }

    #[tokio::test]
    async fn a_missing_app_names_the_slug() {
        let client = client(false);
        let error = Resolver::new().app(&client, "nope").await.unwrap_err();

        assert!(matches!(
            error,
            GitHubError::UnresolvedActor { kind: "app", .. }
        ));
        assert!(error.to_string().contains("nope"), "{error}");
    }

    #[tokio::test]
    async fn teams_and_apps_do_not_share_a_cache() {
        let client = client(true);
        let resolver = Resolver::new();

        resolver.team(&client, "acme", "same").await.unwrap();
        resolver.app(&client, "same").await.unwrap();

        let calls = client.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], "orgs/acme/teams/same");
        assert_eq!(calls[1], "apps/same");
    }
}
