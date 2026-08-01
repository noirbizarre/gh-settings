//! Slug to identifier resolution.
//!
//! Ruleset `bypass_actors` are declared in the configuration by name
//! (`{ team: engineering }`) because identifiers are neither stable across
//! organisations nor meaningful to a human reading a config file. The API,
//! however, wants a numeric `actor_id`.
//!
//! Resolution therefore happens once, up front, and a failure is reported as a
//! *validation* error pointing at the offending line, not as a stray HTTP 404
//! halfway through an apply.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::github::client::{GitHubClient, GitHubClientExt, Request};
use crate::github::{GitHubError, Result};

#[derive(Debug, Deserialize)]
struct Team {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct App {
    id: u64,
}

/// Caches slug lookups for the duration of a run.
///
/// A single ruleset commonly references the same team several times; without the
/// cache each reference would cost a round trip.
pub struct Resolver {
    client: Arc<dyn GitHubClient>,
    teams: Mutex<HashMap<String, u64>>,
    apps: Mutex<HashMap<String, u64>>,
}

impl Resolver {
    /// Build a resolver over a client.
    pub fn new(client: Arc<dyn GitHubClient>) -> Self {
        Self {
            client,
            teams: Mutex::new(HashMap::new()),
            apps: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve an organisation team slug to its identifier.
    pub async fn team(&self, org: &str, slug: &str) -> Result<u64> {
        let key = format!("{org}/{slug}");
        if let Some(id) = self.teams.lock().await.get(&key) {
            return Ok(*id);
        }

        let team: Option<Team> = self
            .client
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
    pub async fn app(&self, slug: &str) -> Result<u64> {
        if let Some(id) = self.apps.lock().await.get(slug) {
            return Ok(*id);
        }

        let app: Option<App> = self
            .client
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
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver").finish_non_exhaustive()
    }
}
