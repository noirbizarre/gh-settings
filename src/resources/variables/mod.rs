//! Actions variables, at repository and at environment scope.
//!
//! # Why variables and not secrets
//!
//! A secret's value can never be read back, so it can be neither diffed nor
//! exported and every run would have to rewrite it — which is why ADR-009
//! declines them. A variable's value comes straight out of the API, so it
//! behaves like anything else here.
//!
//! # One resource, two configuration sections
//!
//! Repository variables are declared under the top-level `variables:` key,
//! environment variables under `environments[].variables`. Both are handled by
//! *this* resource, because the two scopes share an identical payload, verbs,
//! normalisation and diff — splitting them would be two copies of the same code
//! differing only in a path prefix, and `--only variables` would then silently
//! skip half the work.
//!
//! The cost is that the resource-to-section mapping is not one-to-one, which
//! shows up in `export`: the engine files an exported section under the
//! resource's own identifier, so this resource can only ever produce the
//! top-level `variables:` key. Environment variables are therefore exported by
//! the `environments` resource, which owns the section they live in. See
//! ADR-018.
//!
//! # Ordering
//!
//! An environment must exist before its variables can, so this resource depends
//! on `environments` (ADR-011). Planning, however, happens entirely before any
//! apply, so reading the variables of an environment that the configuration
//! declares but the repository does not yet have will 404 — that is read as
//! "no variables", which makes every declared variable a creation, which is
//! exactly the work that then needs doing.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Finding, Settings};
use crate::diff::diff_keyed;
use crate::github::{
    GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target, urlencode,
};
use crate::resources::environments::model::{EnvironmentPage, key as environment_key};
use crate::resources::{Change, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx};

pub mod model;

pub use model::{Variable, VariableState};

/// The `variables` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Variables;

/// Where a variable lives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// A repository-wide variable.
    Repository,
    /// A variable scoped to one environment, held under its matching key.
    Environment(String),
}

impl Scope {
    /// Prefix of the change key.
    ///
    /// Part of the plan artifact, and therefore public interface.
    fn prefix(&self) -> String {
        match self {
            Self::Repository => "repo:".into(),
            Self::Environment(name) => format!("env/{name}:"),
        }
    }

    /// The collection endpoint for this scope.
    fn endpoint(&self, target: &Target) -> String {
        match self {
            Self::Repository => target.endpoint("actions/variables"),
            Self::Environment(name) => target.endpoint(&format!(
                "environments/{}/variables",
                urlencode(name.as_str())
            )),
        }
    }

    /// How the scope reads in a plan summary.
    fn label(&self) -> String {
        match self {
            Self::Repository => "repository variable".into(),
            Self::Environment(name) => format!("{name} variable"),
        }
    }
}

/// Desired variable configuration, across every managed scope.
#[derive(Debug, Clone, Default)]
pub struct Desired {
    /// Declared variables, keyed by scope and normalised name.
    pub variables: BTreeMap<(Scope, String), Variable>,
    /// Scopes the configuration actually declares.
    ///
    /// Distinct from the keys above: an environment declaring `variables: []`
    /// manages its variables and has none, which is what makes pruning it
    /// meaningful.
    pub managed: HashSet<Scope>,
    /// Whether unmanaged variables may be deleted, per scope.
    pub prune: HashMap<Scope, bool>,
    /// The top-level section exactly as declared.
    ///
    /// Kept alongside the keyed map because diagnostics are reported against
    /// the file, and the map has already lost the declaration order the spans
    /// are indexed by.
    pub declared: Vec<Variable>,
}

/// Current variable state on GitHub, across every scope that exists.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Variables that exist, keyed by scope and normalised name.
    pub variables: BTreeMap<(Scope, String), Variable>,
}

/// Payload carried by a variable change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// Where the variable lives.
    pub scope: Scope,
    /// The variable to write.
    pub variable: Variable,
}

#[async_trait]
impl Resource for Variables {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Variables
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::VARIABLES
    }

    fn depends_on(&self) -> &'static [ResourceId] {
        // An environment has to exist before a variable can be written into it.
        &[ResourceId::Environments]
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        let repository = settings.variables.as_ref();
        let environments = settings.environments.as_ref();

        // Reading only `settings.variables` here would report the resource
        // unmanaged for a file that declares nothing but environment
        // variables — and an unmanaged resource is skipped entirely, so those
        // variables would silently never be written.
        let has_environment_variables = environments.is_some_and(|section| {
            section
                .items()
                .iter()
                .any(|environment| environment.variables.is_some())
        });
        if repository.is_none() && !has_environment_variables {
            return None;
        }

        let mut desired = Desired::default();

        if let Some(section) = repository {
            desired.declared = section.items().to_vec();
            desired.managed.insert(Scope::Repository);
            desired.prune.insert(Scope::Repository, section.prune());
            for variable in section.items() {
                let variable = variable.normalized();
                desired
                    .variables
                    .insert((Scope::Repository, variable.name.clone()), variable);
            }
        }

        // Environment variables are pruned under the `environments` section's
        // flag: a variable cannot outlive the environment holding it, so one
        // flag governing both is the only reading that stays coherent.
        let environments_prune = environments.is_some_and(|section| section.prune());
        for environment in environments.map(|section| section.items()).unwrap_or(&[]) {
            let Some(variables) = &environment.variables else {
                continue;
            };
            let scope = Scope::Environment(environment_key(&environment.name));
            desired.managed.insert(scope.clone());
            desired.prune.insert(scope.clone(), environments_prune);
            for variable in variables {
                let variable = variable.normalized();
                desired
                    .variables
                    .insert((scope.clone(), variable.name.clone()), variable);
            }
        }

        Some(desired)
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        // Environment variables are validated by the `environments` resource,
        // which owns the spans they sit under; this covers the top-level
        // section only.
        model::validate(&desired.declared, "variables", ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let mut current = Current::default();

        read_scope(client, target, &Scope::Repository, &mut current).await?;

        // `current` has no access to the configuration, so the environments
        // that exist are enumerated rather than assumed. Ones the file declares
        // but the repository does not have contribute nothing and cost no
        // request at all.
        let page: EnvironmentPage = client
            .send_optional(Request::get(target.endpoint("environments?per_page=100")))
            .await?
            .unwrap_or_default();

        for environment in page.environments {
            let scope = Scope::Environment(environment_key(&environment.name));
            read_scope(client, target, &scope, &mut current).await?;
        }

        Ok(current)
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        prune: &PruneOpts,
    ) -> Vec<Change> {
        let diff = diff_keyed(
            desired
                .variables
                .iter()
                .map(|(key, variable)| (key.clone(), variable.clone())),
            current
                .variables
                .iter()
                .map(|(key, variable)| (key.clone(), variable.clone())),
        );

        let mut changes = Vec::new();

        for ((scope, _), variable) in diff.created {
            changes.push(
                Change::new(
                    ResourceId::Variables,
                    Op::Create,
                    change_key(&scope, &variable),
                )
                .summary(format!("create {} {}", scope.label(), variable.name))
                .fields(variable.as_fields())
                .payload(Payload { scope, variable }),
            );
        }

        for ((scope, _), desired_variable, current_variable) in diff.matched {
            let fields = desired_variable.diff_against(&current_variable);
            if fields.is_empty() {
                continue;
            }
            changes.push(
                Change::new(
                    ResourceId::Variables,
                    Op::Update,
                    change_key(&scope, &desired_variable),
                )
                .summary(format!(
                    "update {} {}",
                    scope.label(),
                    desired_variable.name
                ))
                .fields(fields)
                .payload(Payload {
                    scope,
                    variable: desired_variable,
                }),
            );
        }

        for ((scope, _), variable) in diff.deleted {
            // A scope the configuration says nothing about is never pruned,
            // whatever `--prune` says: `variables: {prune: true}` asks to tidy
            // the repository's own variables, not every environment's.
            if !desired.managed.contains(&scope) {
                continue;
            }
            if !prune.resolve(desired.prune.get(&scope).copied().unwrap_or(false)) {
                continue;
            }
            changes.push(
                Change::new(
                    ResourceId::Variables,
                    Op::Delete,
                    change_key(&scope, &variable),
                )
                .summary(format!("delete {} {}", scope.label(), variable.name))
                .payload(Payload { scope, variable }),
            );
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
            panic!("variable change carried an undecodable payload: {error}")
        });

        let collection = payload.scope.endpoint(target);
        let item = format!("{collection}/{}", urlencode(&payload.variable.name));

        match change.op {
            Op::Create => {
                client
                    .execute(Request::post(collection, payload.variable.as_body()))
                    .await
            }
            Op::Update => {
                client
                    .execute(Request::patch(item, payload.variable.as_body()))
                    .await
            }
            Op::Delete => client.execute(Request::delete(item)).await,
            // A variable's value is patchable in place, so `diff` never emits a
            // recreate.
            Op::Recreate => unreachable!("variables are never recreated"),
        }
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        // Repository scope only. Environment variables belong to the
        // `environments` section, which the `environments` resource exports —
        // see the module documentation and ADR-018.
        let mut current = Current::default();
        read_scope(client, target, &Scope::Repository, &mut current).await?;

        if current.variables.is_empty() {
            return Ok(None);
        }

        let variables: Vec<_> = current.variables.values().collect();

        Ok(Some(serde_json::to_value(variables).unwrap_or(Value::Null)))
    }
}

/// Read one scope's variables into `current`.
///
/// The endpoint is enveloped (`{total_count, variables}`) rather than a bare
/// array, so it is read with a single `per_page=100` request instead of
/// `--paginate`: `gh api --paginate` concatenates JSON documents and cannot
/// merge two envelopes. A hundred is GitHub's own cap per scope, so nothing is
/// lost.
///
/// A 404 means the environment does not exist yet, which reads as "no
/// variables". A 403 still propagates: "you may not look" must never be
/// silently mistaken for "there is nothing there".
async fn read_scope(
    client: &dyn GitHubClient,
    target: &Target,
    scope: &Scope,
    current: &mut Current,
) -> GitHubResult<()> {
    let endpoint = format!("{}?per_page=100", scope.endpoint(target));
    let page: model::VariablePage = client
        .send_optional(Request::get(endpoint))
        .await?
        .unwrap_or_default();

    for state in page.variables {
        let variable = state.as_variable();
        current
            .variables
            .insert((scope.clone(), variable.name.clone()), variable);
    }
    Ok(())
}

/// The scope-qualified key a change is identified by.
fn change_key(scope: &Scope, variable: &Variable) -> String {
    format!("{}{}", scope.prefix(), variable.name)
}

#[cfg(test)]
mod tests;
