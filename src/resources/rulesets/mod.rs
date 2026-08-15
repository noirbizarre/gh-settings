//! Repository rulesets.
//!
//! This is the most intricate v1 resource and was chosen deliberately as the
//! stress test for the [`Resource`] abstraction (plan §10, M3).
//!
//! # Identity
//!
//! Rulesets are matched by `name`. Server-assigned ids are deliberately never
//! written to the configuration: they are not stable across repositories, which
//! would make an exported file useless anywhere but its origin.
//!
//! # Normalisation
//!
//! The API returns `id`, `node_id`, `created_at`, `updated_at`, `_links` and
//! `current_user_can_bypass`, none of which are configuration. It also returns
//! `rules` in an arbitrary order. All of that is stripped and canonically sorted
//! before comparison, or the plan would report spurious changes on every run.
//!
//! # Unknown rules
//!
//! GitHub adds rule types faster than any client can track. An unrecognised rule
//! is preserved verbatim through [`Rule::Unknown`] rather than dropped, so
//! exporting and re-syncing never silently deletes a rule this build predates.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Finding, Settings};
use crate::diff::diff_keyed;
use crate::github::{
    GitHubClient, GitHubClientExt, Request, Resolver, Result as GitHubResult, Target,
};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

pub mod model;

pub use model::{
    BypassActor, BypassMode, Conditions, Enforcement, RefNameCondition, Rule, Ruleset,
    Target as RulesetTarget,
};

/// The `rulesets` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Rulesets;

/// Desired ruleset configuration.
#[derive(Debug, Clone)]
pub struct Desired {
    /// Declared rulesets, normalised and with actors resolved.
    pub rulesets: Vec<Ruleset>,
    /// Whether unmanaged rulesets should be deleted.
    pub prune: bool,
}

/// Current rulesets, keyed by name.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Existing rulesets and their server ids.
    pub rulesets: HashMap<String, (u64, Ruleset)>,
}

/// Payload of a ruleset change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The ruleset to write, absent for a deletion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruleset: Option<Ruleset>,
    /// The server id, for updates and deletions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
}

/// The subset of the API ruleset payload we consume.
#[derive(Debug, Deserialize)]
struct RulesetState {
    id: u64,
    name: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    enforcement: Option<String>,
    #[serde(default)]
    bypass_actors: Vec<Value>,
    #[serde(default)]
    conditions: Option<Value>,
    #[serde(default)]
    rules: Vec<Value>,
}

#[async_trait]
impl Resource for Rulesets {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Rulesets
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ADMINISTRATION
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        let section = settings.rulesets.as_ref()?;
        Some(Desired {
            rulesets: section.items().iter().map(Ruleset::normalized).collect(),
            prune: section.prune(),
        })
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(&desired.rulesets, ctx)
    }

    /// Resolve bypass actor slugs to the numeric ids the API requires.
    async fn prepare(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        mut desired: Self::Desired,
    ) -> GitHubResult<Self::Desired> {
        // Only pay for the lookups if some ruleset actually names an actor.
        if !desired.rulesets.iter().any(|ruleset| {
            ruleset
                .bypass_actors
                .iter()
                .any(BypassActor::needs_resolution)
        }) {
            return Ok(desired);
        }

        // One resolver for the whole run: a fleet of rulesets almost always
        // names the same handful of teams, and each mention would otherwise be
        // a round trip.
        let resolver = Resolver::new();

        for ruleset in &mut desired.rulesets {
            for actor in &mut ruleset.bypass_actors {
                actor.resolve(client, &target.owner, &resolver).await?;
            }
        }

        Ok(desired)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let summaries: Vec<RulesetState> = client
            .send(Request::list(target.endpoint("rulesets")))
            .await?;

        let mut rulesets = HashMap::new();
        for summary in summaries {
            // The list endpoint omits `rules` and `bypass_actors`; only the
            // detail endpoint returns a complete ruleset.
            let detail: RulesetState = client
                .send(Request::get(
                    target.endpoint(&format!("rulesets/{}", summary.id)),
                ))
                .await?;
            rulesets.insert(detail.name.clone(), (detail.id, from_state(&detail)));
        }

        Ok(Current { rulesets })
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        prune: &PruneOpts,
    ) -> Vec<Change> {
        let prune = prune.resolve(desired.prune);

        let diff = diff_keyed(
            desired
                .rulesets
                .iter()
                .map(|ruleset| (ruleset.name.clone(), ruleset.clone())),
            current
                .rulesets
                .iter()
                .map(|(name, entry)| (name.clone(), entry.clone())),
        );

        let mut changes = Vec::new();

        for (name, ruleset) in diff.created {
            changes.push(
                Change::new(ResourceId::Rulesets, Op::Create, &name)
                    .summary(format!("create ruleset {name}"))
                    .fields(vec![
                        FieldDiff::added("enforcement", ruleset.enforcement.as_str()),
                        FieldDiff::added("rules", ruleset.rules.len().to_string()),
                    ])
                    .payload(Payload {
                        ruleset: Some(ruleset),
                        id: None,
                    }),
            );
        }

        for (name, desired_ruleset, (id, current_ruleset)) in diff.matched {
            let fields = desired_ruleset.diff_against(&current_ruleset);
            if fields.is_empty() {
                continue;
            }
            changes.push(
                Change::new(ResourceId::Rulesets, Op::Update, &name)
                    .summary(format!("update ruleset {name}"))
                    .fields(fields)
                    .payload(Payload {
                        ruleset: Some(desired_ruleset),
                        id: Some(id),
                    }),
            );
        }

        if prune {
            for (name, (id, _)) in diff.deleted {
                changes.push(
                    Change::new(ResourceId::Rulesets, Op::Delete, &name)
                        .summary(format!("delete ruleset {name}"))
                        .payload(Payload {
                            ruleset: None,
                            id: Some(id),
                        }),
                );
            }
        }

        changes
    }

    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()> {
        let payload: Payload = change
            .decode()
            .unwrap_or_else(|error| panic!("ruleset change carried a bad payload: {error}"));

        match change.op {
            Op::Create => {
                let ruleset = payload
                    .ruleset
                    .unwrap_or_else(|| panic!("a ruleset create change carried no ruleset"));
                client
                    .execute(Request::post(
                        target.endpoint("rulesets"),
                        ruleset.as_body(),
                    ))
                    .await
            }
            Op::Update => {
                let ruleset = payload
                    .ruleset
                    .unwrap_or_else(|| panic!("a ruleset update change carried no ruleset"));
                let id = payload
                    .id
                    .unwrap_or_else(|| panic!("a ruleset update change carried no id"));
                client
                    .execute(Request::put(
                        target.endpoint(&format!("rulesets/{id}")),
                        ruleset.as_body(),
                    ))
                    .await
            }
            Op::Delete => {
                let id = payload
                    .id
                    .unwrap_or_else(|| panic!("a ruleset delete change carried no id"));
                client
                    .execute(Request::delete(target.endpoint(&format!("rulesets/{id}"))))
                    .await
            }
            // `PUT /rulesets/{id}` replaces the whole ruleset, so `diff` never
            // needs a recreate.
            Op::Recreate => unreachable!("rulesets are never recreated"),
        }
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;
        if current.rulesets.is_empty() {
            return Ok(None);
        }

        let mut rulesets: Vec<Ruleset> = current
            .rulesets
            .values()
            .map(|(_, ruleset)| ruleset.exportable())
            .collect();
        rulesets.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Some(serde_json::to_value(rulesets).unwrap_or(Value::Null)))
    }
}

/// Convert an API payload into the comparable configuration shape.
///
/// This is where server-only fields are dropped.
fn from_state(state: &RulesetState) -> Ruleset {
    Ruleset {
        name: state.name.clone(),
        target: state
            .target
            .as_deref()
            .and_then(model::parse_target)
            .unwrap_or_default(),
        enforcement: state
            .enforcement
            .as_deref()
            .and_then(model::parse_enforcement)
            .unwrap_or_default(),
        bypass_actors: state
            .bypass_actors
            .iter()
            .filter_map(BypassActor::from_api)
            .collect(),
        conditions: state.conditions.as_ref().and_then(Conditions::from_api),
        rules: state.rules.iter().filter_map(Rule::from_api).collect(),
    }
    .normalized()
}

#[cfg(test)]
mod tests;
