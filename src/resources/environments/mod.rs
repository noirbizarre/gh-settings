//! Deployment environments.
//!
//! # Two endpoints for one thing
//!
//! `PUT .../environments/{name}` is create-or-update, so a creation and an
//! update are the same request and [`Op::Recreate`] never arises. Its body
//! carries only the two branch-policy *flags*, though: the patterns themselves
//! live behind `.../environments/{name}/deployment-branch-policies`, are
//! created one at a time, and are deleted by a server-assigned identifier —
//! which is why [`Current`] remembers those identifiers, exactly as rulesets
//! does.
//!
//! Deletions of patterns precede creations within a single change, because
//! GitHub answers a duplicate pattern name with a 422 rather than a merge.
//!
//! # What this resource exports
//!
//! Environment-scoped variables are declared under `environments[].variables`
//! but are written by the `variables` resource. Because the engine files an
//! exported section under the resource's own identifier, the `environments`
//! section — variables included — can only be produced here. See ADR-018.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Finding, Settings};
use crate::diff::diff_keyed;
use crate::github::{
    GitHubClient, GitHubClientExt, Request, Resolver, Result as GitHubResult, Target, urlencode,
};
use crate::resources::variables::model::VariablePage;
use crate::resources::{Change, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx};

pub mod model;

pub use model::{DeploymentBranchPolicy, Environment, EnvironmentState, Pattern, Reviewer};

/// The `environments` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Environments;

/// Desired environment configuration.
#[derive(Debug, Clone, Default)]
pub struct Desired {
    /// Environments declared in the configuration, normalised.
    pub environments: Vec<Environment>,
    /// The section exactly as declared, for diagnostics.
    pub declared: Vec<Environment>,
    /// Whether environments absent from the configuration should be deleted.
    pub prune: bool,
}

/// One environment as it exists on GitHub.
#[derive(Debug, Clone, Default)]
pub struct CurrentEnvironment {
    /// The comparable state.
    pub environment: Environment,
    /// Server identifiers of its branch policy patterns, needed to delete them.
    pub pattern_ids: HashMap<Pattern, u64>,
}

/// Current environment state on GitHub, normalised.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Environments that exist, keyed by their matching name.
    pub environments: HashMap<String, CurrentEnvironment>,
}

/// Payload carried by an environment change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The name to address, spelled as the configuration declares it.
    pub name: String,
    /// The environment to write, absent for a deletion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Patterns to create, after the environment itself exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub create_patterns: Vec<Pattern>,
    /// Server identifiers of patterns to remove, resolved while reading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delete_pattern_ids: Vec<u64>,
}

#[async_trait]
impl Resource for Environments {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Environments
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ENVIRONMENTS
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        let section = settings.environments.as_ref()?;
        Some(Desired {
            environments: section
                .items()
                .iter()
                .map(Environment::normalized)
                .collect(),
            declared: section.items().to_vec(),
            prune: section.prune(),
        })
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(&desired.declared, ctx)
    }

    async fn prepare(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        mut desired: Self::Desired,
    ) -> GitHubResult<Self::Desired> {
        // Only pay for the lookups if somebody is actually named. A fleet of
        // environments almost always names the same approving team, so one
        // resolver serves the whole run.
        if !desired.environments.iter().any(|environment| {
            environment
                .reviewers
                .iter()
                .flatten()
                .any(Reviewer::needs_resolution)
        }) {
            return Ok(desired);
        }

        let resolver = Resolver::new();
        for environment in &mut desired.environments {
            for reviewer in environment.reviewers.iter_mut().flatten() {
                reviewer.resolve(client, &target.owner, &resolver).await?;
            }
        }

        // Resolution can change the sort key, so the canonical order has to be
        // re-established or an unchanged reviewer list would look reordered.
        for environment in &mut desired.environments {
            *environment = environment.normalized();
        }

        Ok(desired)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let page: model::EnvironmentPage = client
            .send_optional(Request::get(target.endpoint("environments?per_page=100")))
            .await?
            .unwrap_or_default();

        let mut current = Current::default();

        for state in page.environments {
            let key = model::key(&state.name);
            let mut environment = state.as_environment();
            let mut pattern_ids = HashMap::new();

            // The second call is made only where there can be patterns to
            // fetch: a null or protected-branches policy has none by
            // definition, so this is a request not made rather than one made
            // and thrown away.
            if state.has_custom_branch_policies() {
                let patterns: model::PatternPage = client
                    .send_optional(Request::get(target.endpoint(&format!(
                        "environments/{}/deployment-branch-policies?per_page=100",
                        urlencode(&state.name)
                    ))))
                    .await?
                    .unwrap_or_default();

                let mut branches = Vec::new();
                let mut tags = Vec::new();
                for state in &patterns.branch_policies {
                    let pattern = state.as_pattern();
                    if pattern.r#type == "tag" {
                        tags.push(pattern.name.clone());
                    } else {
                        branches.push(pattern.name.clone());
                    }
                    pattern_ids.insert(pattern, state.id);
                }
                branches.sort();
                tags.sort();
                environment.deployment_branch_policy =
                    Some(Some(DeploymentBranchPolicy::Custom { branches, tags }));
            }

            current.environments.insert(
                key,
                CurrentEnvironment {
                    environment: environment.normalized(),
                    pattern_ids,
                },
            );
        }

        Ok(current)
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
                .environments
                .iter()
                .map(|environment| (model::key(&environment.name), environment.clone())),
            current
                .environments
                .iter()
                .map(|(key, entry)| (key.clone(), entry.clone())),
        );

        let mut changes = Vec::new();

        for (_, environment) in diff.created {
            let patterns = environment
                .deployment_branch_policy
                .clone()
                .flatten()
                .map(|policy| policy.patterns())
                .unwrap_or_default();

            changes.push(
                Change::new(
                    ResourceId::Environments,
                    Op::Create,
                    environment.name.clone(),
                )
                .summary(format!("create environment {}", environment.name))
                .fields(environment.as_fields())
                .payload(Payload {
                    name: environment.name.clone(),
                    environment: Some(environment),
                    create_patterns: patterns,
                    delete_pattern_ids: Vec::new(),
                }),
            );
        }

        for (_, desired_environment, existing) in diff.matched {
            let fields = desired_environment.diff_against(&existing.environment);

            // Not gated on `prune`. Removing a pattern from a policy the user
            // *is* declaring is an edit to that policy, not a prune: the
            // declared list is the whole list, and the alternative is a
            // permanent diff nothing could ever reconcile. Pruning governs
            // whole environments the configuration stops mentioning, which is
            // the `diff.deleted` loop below.
            let (create_patterns, delete_pattern_ids) =
                pattern_changes(&desired_environment, &existing);

            if fields.is_empty() && create_patterns.is_empty() && delete_pattern_ids.is_empty() {
                continue;
            }

            changes.push(
                Change::new(
                    ResourceId::Environments,
                    Op::Update,
                    desired_environment.name.clone(),
                )
                .summary(format!("update environment {}", desired_environment.name))
                .fields(fields)
                .payload(Payload {
                    name: desired_environment.name.clone(),
                    environment: Some(desired_environment.applied(&existing.environment)),
                    create_patterns,
                    delete_pattern_ids,
                }),
            );
        }

        if prune {
            for (_, entry) in diff.deleted {
                let name = entry.environment.name.clone();
                changes.push(
                    Change::new(ResourceId::Environments, Op::Delete, name.clone())
                        .summary(format!(
                            "delete environment {name} \
                             (also deletes its variables, secrets and deployment history)"
                        ))
                        .payload(Payload {
                            name,
                            environment: None,
                            create_patterns: Vec::new(),
                            delete_pattern_ids: Vec::new(),
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
        let payload: Payload = change.decode().unwrap_or_else(|error| {
            panic!("environment change carried an undecodable payload: {error}")
        });

        let endpoint = target.endpoint(&format!("environments/{}", urlencode(&payload.name)));

        if change.op == Op::Delete {
            return client.execute(Request::delete(endpoint)).await;
        }

        let environment = payload
            .environment
            .as_ref()
            .unwrap_or_else(|| panic!("a non-deleting environment change carried no environment"));

        // One request for both create and update: the endpoint is idempotent,
        // so there is nothing for `Op::Recreate` to mean here.
        client
            .execute(Request::put(
                endpoint.clone(),
                environment.as_body(environment),
            ))
            .await?;

        // Patterns can only exist once the environment does, so they follow the
        // PUT — and deletions precede creations because GitHub answers a
        // duplicate pattern name with a 422 rather than merging the two.
        for id in &payload.delete_pattern_ids {
            client
                .execute(Request::delete(format!(
                    "{endpoint}/deployment-branch-policies/{id}"
                )))
                .await?;
        }
        for pattern in &payload.create_patterns {
            client
                .execute(Request::post(
                    format!("{endpoint}/deployment-branch-policies"),
                    pattern.as_body(),
                ))
                .await?;
        }

        Ok(())
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;
        if current.environments.is_empty() {
            return Ok(None);
        }

        let mut keys: Vec<&String> = current.environments.keys().collect();
        keys.sort();

        let mut exported = Vec::new();
        for key in keys {
            let entry = &current.environments[key];
            let mut environment = entry.environment.clone();

            // Reviewers are exported by login and slug, never by identifier: an
            // exported configuration has to be readable, and reusable in a
            // repository where the numbers mean something else.
            if let Some(reviewers) = environment.reviewers.as_mut() {
                for reviewer in reviewers.iter_mut() {
                    reviewer.id = None;
                }
                if reviewers.is_empty() {
                    environment.reviewers = None;
                }
            }
            if environment.prevent_self_review == Some(false) {
                environment.prevent_self_review = None;
            }
            if environment.deployment_branch_policy == Some(None) {
                environment.deployment_branch_policy = None;
            }

            // This resource owns the `environments` section, so it is the only
            // place environment variables can be emitted (ADR-018).
            let page: VariablePage = client
                .send_optional(Request::get(format!(
                    "{}?per_page=100",
                    target.endpoint(&format!(
                        "environments/{}/variables",
                        urlencode(&entry.environment.name)
                    ))
                )))
                .await?
                .unwrap_or_default();
            if !page.variables.is_empty() {
                let mut variables: Vec<crate::resources::variables::Variable> = page
                    .variables
                    .iter()
                    .map(|state| state.as_variable())
                    .collect();
                variables.sort_by(|left, right| left.name.cmp(&right.name));
                environment.variables = Some(variables);
            }

            exported.push(serde_json::to_value(environment).unwrap_or(Value::Null));
        }

        Ok(Some(Value::Array(exported)))
    }
}

/// Which patterns have to be created, and which removed by identifier.
///
/// Returns nothing at all when the policy is not managed, or when it is moving
/// away from custom patterns: the `PUT` discards the patterns on its own, so
/// deleting them individually first would be a wasted round trip per pattern.
fn pattern_changes(
    desired: &Environment,
    current: &CurrentEnvironment,
) -> (Vec<Pattern>, Vec<u64>) {
    let Some(policy) = desired.deployment_branch_policy.clone().flatten() else {
        return (Vec::new(), Vec::new());
    };
    let DeploymentBranchPolicy::Custom { .. } = policy else {
        return (Vec::new(), Vec::new());
    };

    let existing = current
        .environment
        .deployment_branch_policy
        .clone()
        .flatten()
        .map(|policy| policy.patterns())
        .unwrap_or_default();

    let diff = diff_keyed(
        policy
            .patterns()
            .into_iter()
            .map(|pattern| (pattern.clone(), pattern)),
        existing
            .into_iter()
            .map(|pattern| (pattern.clone(), pattern)),
    );

    let create = diff
        .created
        .into_iter()
        .map(|(_, pattern)| pattern)
        .collect();
    let delete = diff
        .deleted
        .into_iter()
        .filter_map(|(_, pattern)| current.pattern_ids.get(&pattern).copied())
        .collect();

    (create, delete)
}

#[cfg(test)]
mod tests;
