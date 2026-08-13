//! GitHub Actions general settings.
//!
//! A singleton resource, like `repository` and `pages`, but spread over seven
//! endpoints. *Settings → Actions → General* is one screen in the web UI and one
//! `actions:` section in the configuration; the REST API splits it into seven
//! siblings under `/repos/{owner}/{repo}/actions/permissions`, each with its own
//! `GET` and its own `PUT`.
//!
//! # One change per endpoint
//!
//! The grouping is not cosmetic. Each `PUT` accepts only its own fields and
//! rejects a mixed body, the same constraint that keeps `security_and_analysis`
//! in a request of its own in the `repository` resource. So a change is keyed by
//! the endpoint suffix it will be sent to, and `apply` needs no more than that
//! key to know where the body goes.
//!
//! # Endpoints that may not exist
//!
//! Three of these endpoints do not always exist, and GitHub does not use `404`
//! to say so. Verified against a real repository:
//!
//! * `GET .../selected-actions` → `409 Conflict`, *"All actions and workflows
//!   are allowed on this repository"*, whenever the policy is not `selected`;
//! * `GET .../access` → `422`, *"Access policy only applies to internal and
//!   private repositories"*;
//! * `GET .../fork-pr-workflows-private-repos` → `422`, *"Fork PR workflow
//!   settings is not allowed for public repositories"*.
//!
//! So [`read`] absorbs `404`, `409` and `422` into `None` rather than using
//! `send_optional`, which only knows about `404`. On a `GET` — a request with no
//! body to be malformed — all three say the same thing: this setting does not
//! apply to this repository right now. Treating them as errors made every plan
//! against a public repository fail outright, which is how this was found.
//!
//! An absent group that the configuration *declares* still produces a change.
//! Reporting "up to date" for a setting we could not read would claim a
//! convergence that has not happened; emitting the change means `apply` surfaces
//! GitHub's own refusal, which says far more than our silence would.
//!
//! A `403` is deliberately *not* absorbed the same way. Artifact retention can
//! be locked by an enterprise owner, and that comes back as `403` — but so does
//! a token that simply lacks the permission, and the two are indistinguishable
//! from here. Swallowing it would turn a credential problem into a silent
//! no-op, so it stays an error and `doctor` gets to explain it.
//!
//! # This resource never deletes
//!
//! There is nothing to prune: every setting has a value, never an existence.
//! `prune` is ignored.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use serde::de::DeserializeOwned;

use crate::config::{Finding, Settings};
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

pub mod model;

pub use model::{
    AccessLevel, ActionsSettings, AllowedActions, ForkPrApproval, ForkPrWorkflowsPrivateRepos,
    SelectedActions, WorkflowPermissions,
};

/// The `actions` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Actions;

/// The endpoint suffix a change is keyed by, relative to `actions/permissions`.
mod endpoint {
    /// `enabled`, `allowed_actions`, `sha_pinning_required`.
    pub const PERMISSIONS: &str = "permissions";
    /// The allow list consulted when the policy is `selected`.
    pub const SELECTED_ACTIONS: &str = "selected-actions";
    /// `GITHUB_TOKEN` defaults.
    pub const WORKFLOW: &str = "workflow";
    /// Artifact and log retention.
    pub const RETENTION: &str = "artifact-and-log-retention";
    /// Fork pull request approval policy.
    pub const FORK_PR_APPROVAL: &str = "fork-pr-contributor-approval";
    /// Visibility of this repository's actions to other repositories.
    pub const ACCESS: &str = "access";
    /// Fork pull request behaviour on private repositories.
    pub const FORK_PR_PRIVATE: &str = "fork-pr-workflows-private-repos";

    /// The full path for a change key.
    pub fn path(key: &str) -> String {
        match key {
            PERMISSIONS => "actions/permissions".to_string(),
            other => format!("actions/permissions/{other}"),
        }
    }
}

/// `GET /actions/permissions`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionsState {
    /// Whether Actions runs on this repository.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Which actions are allowed to run.
    #[serde(default)]
    pub allowed_actions: Option<String>,
    /// Whether actions must be pinned to a SHA.
    #[serde(default)]
    pub sha_pinning_required: Option<bool>,
}

/// `GET /actions/permissions/selected-actions`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SelectedActionsState {
    /// Whether GitHub-owned actions are allowed.
    #[serde(default)]
    pub github_owned_allowed: Option<bool>,
    /// Whether verified creators' actions are allowed.
    #[serde(default)]
    pub verified_allowed: Option<bool>,
    /// The allow list patterns.
    #[serde(default)]
    pub patterns_allowed: Option<Vec<String>>,
}

/// `GET /actions/permissions/workflow`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowState {
    /// Default `GITHUB_TOKEN` permissions.
    #[serde(default)]
    pub default_workflow_permissions: Option<String>,
    /// Whether workflows may approve pull requests.
    #[serde(default)]
    pub can_approve_pull_request_reviews: Option<bool>,
}

/// `GET /actions/permissions/artifact-and-log-retention`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RetentionState {
    /// The retention period, in days.
    #[serde(default)]
    pub days: Option<u32>,
    /// The ceiling GitHub will accept. Read-only: reported, never sent, never
    /// exported — it describes the plan, not this repository's configuration.
    #[serde(default)]
    pub maximum_allowed_days: Option<u32>,
}

/// `GET /actions/permissions/fork-pr-contributor-approval`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForkPrApprovalState {
    /// When a fork pull request needs approval.
    #[serde(default)]
    pub approval_policy: Option<String>,
}

/// `GET /actions/permissions/access`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccessState {
    /// How widely this repository's actions are shared.
    #[serde(default)]
    pub access_level: Option<String>,
}

/// `GET /actions/permissions/fork-pr-workflows-private-repos`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForkPrPrivateState {
    /// Whether fork pull requests may run workflows.
    #[serde(default)]
    pub run_workflows_from_fork_pull_requests: Option<bool>,
    /// Whether they get a write-capable token.
    #[serde(default)]
    pub send_write_tokens_to_workflows: Option<bool>,
    /// Whether they can read secrets and variables.
    #[serde(default)]
    pub send_secrets_and_variables: Option<bool>,
    /// Whether they need a maintainer's approval.
    #[serde(default)]
    pub require_approval_for_fork_pr_workflows: Option<bool>,
}

/// Current state, one entry per endpoint.
///
/// `None` means GitHub did not answer with a body — the endpoint does not apply
/// to this repository, or a policy hides it. It is *not* the same as a group
/// whose fields are all `None`, which means the endpoint answered and said
/// nothing.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// `GET /actions/permissions`.
    pub permissions: Option<PermissionsState>,
    /// `GET .../selected-actions`.
    pub selected_actions: Option<SelectedActionsState>,
    /// `GET .../workflow`.
    pub workflow: Option<WorkflowState>,
    /// `GET .../artifact-and-log-retention`.
    pub retention: Option<RetentionState>,
    /// `GET .../fork-pr-contributor-approval`.
    pub fork_pr_approval: Option<ForkPrApprovalState>,
    /// `GET .../access`.
    pub access: Option<AccessState>,
    /// `GET .../fork-pr-workflows-private-repos`.
    pub fork_pr_private: Option<ForkPrPrivateState>,
}

impl Current {
    /// A normalised copy, safe to compare against a normalised counterpart.
    pub fn normalized(mut self) -> Self {
        if let Some(permissions) = &mut self.permissions {
            permissions.allowed_actions =
                model::normalize_enum(permissions.allowed_actions.as_deref());
        }
        if let Some(selected) = &mut self.selected_actions {
            selected.patterns_allowed =
                selected.patterns_allowed.as_deref().map(normalize_patterns);
        }
        if let Some(workflow) = &mut self.workflow {
            workflow.default_workflow_permissions =
                model::normalize_enum(workflow.default_workflow_permissions.as_deref());
        }
        if let Some(approval) = &mut self.fork_pr_approval {
            approval.approval_policy = model::normalize_enum(approval.approval_policy.as_deref());
        }
        if let Some(access) = &mut self.access {
            access.access_level = model::normalize_enum(access.access_level.as_deref());
        }
        self
    }
}

/// Normalise an allow list for comparison and for sending.
///
/// Trimmed, emptied of blanks, sorted and deduplicated. An allow list is a set:
/// GitHub does not promise an order, so comparing the order as written would
/// produce a difference that could never be applied away.
fn normalize_patterns(patterns: &[String]) -> Vec<String> {
    let mut patterns: Vec<String> = patterns
        .iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect();
    patterns.sort();
    patterns.dedup();
    patterns
}

/// Payload of an actions change: the body, and the key says where it goes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The `PUT` body.
    pub body: Value,
}

/// One endpoint's worth of differences, accumulated field by field.
#[derive(Debug, Default)]
struct Group {
    body: Map<String, Value>,
    fields: Vec<FieldDiff>,
}

impl Group {
    /// Record a difference, or nothing when the values already agree.
    ///
    /// `current` is `Option` twice over in effect: `None` means either the
    /// endpoint did not answer or it answered without the field, and both mean
    /// the same thing here — we cannot say the value already matches.
    fn set(&mut self, name: &str, desired: Option<(Value, String)>, current: Option<String>) {
        let Some((value, rendered)) = desired else {
            return;
        };
        if current.as_deref() == Some(rendered.as_str()) {
            return;
        }
        self.body.insert(name.into(), value);
        self.fields.push(match current {
            Some(before) => FieldDiff::changed(name, before, rendered),
            None => FieldDiff::added(name, rendered),
        });
    }

    /// A boolean field.
    fn flag(&mut self, name: &str, desired: Option<bool>, current: Option<bool>) {
        self.set(
            name,
            desired.map(|value| (json!(value), value.to_string())),
            current.map(|value| value.to_string()),
        );
    }

    /// An enum field, compared and sent in the API's own spelling.
    fn text(&mut self, name: &str, desired: Option<&str>, current: Option<&str>) {
        self.set(
            name,
            desired.map(|value| (json!(value), value.to_string())),
            current.map(str::to_string),
        );
    }

    /// A numeric field.
    fn number(&mut self, name: &str, desired: Option<u32>, current: Option<u32>) {
        self.set(
            name,
            desired.map(|value| (json!(value), value.to_string())),
            current.map(|value| value.to_string()),
        );
    }

    /// A list field, normalised on both sides.
    fn list(&mut self, name: &str, desired: Option<&[String]>, current: Option<&[String]>) {
        let desired = desired.map(normalize_patterns);
        let current = current.map(normalize_patterns);
        self.set(
            name,
            desired.map(|value| (json!(value), value.join(", "))),
            current.map(|value| value.join(", ")),
        );
    }

    /// Make sure a field the API requires is in the body, even when the
    /// configuration left it unmanaged.
    ///
    /// `PUT /actions/permissions` refuses a body without `enabled`, so changing
    /// only `allowed_actions` would fail. Filling it from the current state
    /// preserves the setting; defaulting it would silently turn Actions on or
    /// off. When the current state is unknown too, the body goes out without it
    /// and GitHub's own error stands — inventing a value for a required field we
    /// cannot read is exactly the guess this codebase does not make.
    fn require(&mut self, name: &str, current: Option<Value>) {
        if self.body.is_empty() || self.body.contains_key(name) {
            return;
        }
        if let Some(value) = current {
            self.body.insert(name.into(), value);
        }
    }

    /// The change, or `None` when nothing differs.
    fn change(self, key: &'static str, summary: &str) -> Option<Change> {
        if self.body.is_empty() {
            return None;
        }
        Some(
            Change::new(ResourceId::Actions, Op::Update, key)
                .summary(summary.to_string())
                .fields(self.fields)
                .payload(Payload {
                    body: Value::Object(self.body),
                }),
        )
    }
}

#[async_trait]
impl Resource for Actions {
    type Desired = ActionsSettings;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Actions
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ACTIONS
    }

    fn depends_on(&self) -> &'static [ResourceId] {
        // Visibility decides which of these endpoints exist at all, so a run
        // that makes a repository private must do so before configuring the
        // private-only settings.
        &[ResourceId::Repository]
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        settings.actions.clone()
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(desired, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let get = |key: &'static str| Request::get(target.endpoint(&endpoint::path(key)));

        let current = Current {
            permissions: read(client, get(endpoint::PERMISSIONS)).await?,
            selected_actions: read(client, get(endpoint::SELECTED_ACTIONS)).await?,
            workflow: read(client, get(endpoint::WORKFLOW)).await?,
            retention: read(client, get(endpoint::RETENTION)).await?,
            fork_pr_approval: read(client, get(endpoint::FORK_PR_APPROVAL)).await?,
            access: read(client, get(endpoint::ACCESS)).await?,
            fork_pr_private: read(client, get(endpoint::FORK_PR_PRIVATE)).await?,
        };

        Ok(current.normalized())
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        // Unused on purpose: this resource has no delete path. See the module
        // documentation.
        _prune: &PruneOpts,
    ) -> Vec<Change> {
        let mut changes = Vec::new();

        let mut permissions = Group::default();
        let state = current.permissions.as_ref();
        permissions.flag(
            "enabled",
            desired.enabled,
            state.and_then(|state| state.enabled),
        );
        permissions.text(
            "allowed_actions",
            desired.allowed_actions.map(|value| value.as_str()),
            state.and_then(|state| state.allowed_actions.as_deref()),
        );
        permissions.flag(
            "sha_pinning_required",
            desired.sha_pinning_required,
            state.and_then(|state| state.sha_pinning_required),
        );
        permissions.require(
            "enabled",
            state.and_then(|state| state.enabled).map(Value::from),
        );
        changes.extend(permissions.change(endpoint::PERMISSIONS, "update Actions permissions"));

        if let Some(desired) = &desired.selected_actions {
            let mut group = Group::default();
            let state = current.selected_actions.as_ref();
            group.flag(
                "github_owned_allowed",
                desired.github_owned_allowed,
                state.and_then(|state| state.github_owned_allowed),
            );
            group.flag(
                "verified_allowed",
                desired.verified_allowed,
                state.and_then(|state| state.verified_allowed),
            );
            group.list(
                "patterns_allowed",
                desired.patterns_allowed.as_deref(),
                state.and_then(|state| state.patterns_allowed.as_deref()),
            );
            changes.extend(group.change(endpoint::SELECTED_ACTIONS, "update the allowed actions"));
        }

        let mut workflow = Group::default();
        let state = current.workflow.as_ref();
        workflow.text(
            "default_workflow_permissions",
            desired
                .default_workflow_permissions
                .map(|value| value.as_str()),
            state.and_then(|state| state.default_workflow_permissions.as_deref()),
        );
        workflow.flag(
            "can_approve_pull_request_reviews",
            desired.can_approve_pull_request_reviews,
            state.and_then(|state| state.can_approve_pull_request_reviews),
        );
        changes.extend(workflow.change(endpoint::WORKFLOW, "update workflow permissions"));

        let mut retention = Group::default();
        retention.number(
            "days",
            desired.artifact_and_log_retention_days,
            current.retention.as_ref().and_then(|state| state.days),
        );
        changes.extend(retention.change(endpoint::RETENTION, "update artifact and log retention"));

        let mut approval = Group::default();
        approval.text(
            "approval_policy",
            desired.fork_pr_contributor_approval.map(|v| v.as_str()),
            current
                .fork_pr_approval
                .as_ref()
                .and_then(|state| state.approval_policy.as_deref()),
        );
        changes.extend(approval.change(endpoint::FORK_PR_APPROVAL, "update fork PR approval"));

        let mut access = Group::default();
        access.text(
            "access_level",
            desired.access_level.map(|value| value.as_str()),
            current
                .access
                .as_ref()
                .and_then(|state| state.access_level.as_deref()),
        );
        changes.extend(access.change(endpoint::ACCESS, "update the outside access level"));

        if let Some(desired) = &desired.fork_pr_workflows_private_repos {
            let mut group = Group::default();
            let state = current.fork_pr_private.as_ref();
            group.flag(
                "run_workflows_from_fork_pull_requests",
                desired.run_workflows_from_fork_pull_requests,
                state.and_then(|state| state.run_workflows_from_fork_pull_requests),
            );
            group.flag(
                "send_write_tokens_to_workflows",
                desired.send_write_tokens_to_workflows,
                state.and_then(|state| state.send_write_tokens_to_workflows),
            );
            group.flag(
                "send_secrets_and_variables",
                desired.send_secrets_and_variables,
                state.and_then(|state| state.send_secrets_and_variables),
            );
            group.flag(
                "require_approval_for_fork_pr_workflows",
                desired.require_approval_for_fork_pr_workflows,
                state.and_then(|state| state.require_approval_for_fork_pr_workflows),
            );
            group.require(
                "run_workflows_from_fork_pull_requests",
                state
                    .and_then(|state| state.run_workflows_from_fork_pull_requests)
                    .map(Value::from),
            );
            changes.extend(group.change(
                endpoint::FORK_PR_PRIVATE,
                "update private-repository fork PR workflows",
            ));
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
            .unwrap_or_else(|error| panic!("actions change carried a bad payload: {error}"));

        client
            .execute(Request::put(
                target.endpoint(&endpoint::path(&change.key)),
                payload.body,
            ))
            .await
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;

        let settings = ActionsSettings {
            enabled: current.permissions.as_ref().and_then(|state| state.enabled),
            allowed_actions: current
                .permissions
                .as_ref()
                .and_then(|state| state.allowed_actions.as_deref())
                .and_then(AllowedActions::parse),
            sha_pinning_required: current
                .permissions
                .as_ref()
                .and_then(|state| state.sha_pinning_required),
            // Only exported alongside a `selected` policy. An allow list under
            // any other policy is a leftover GitHub is ignoring, and writing it
            // into a file people copy would spread it.
            selected_actions: current
                .selected_actions
                .as_ref()
                .filter(|_| {
                    current
                        .permissions
                        .as_ref()
                        .and_then(|state| state.allowed_actions.as_deref())
                        == Some("selected")
                })
                .map(|state| SelectedActions {
                    github_owned_allowed: state.github_owned_allowed,
                    verified_allowed: state.verified_allowed,
                    patterns_allowed: state.patterns_allowed.clone(),
                })
                // An endpoint that answered but said nothing is not a section:
                // exporting `selected_actions: {}` would put an empty block in
                // the file for a repository that has no allow list.
                .filter(|selected| *selected != SelectedActions::default()),
            default_workflow_permissions: current
                .workflow
                .as_ref()
                .and_then(|state| state.default_workflow_permissions.as_deref())
                .and_then(WorkflowPermissions::parse),
            can_approve_pull_request_reviews: current
                .workflow
                .as_ref()
                .and_then(|state| state.can_approve_pull_request_reviews),
            // `maximum_allowed_days` is deliberately not exported: it describes
            // the plan, not this repository, and is not a body parameter.
            artifact_and_log_retention_days: current
                .retention
                .as_ref()
                .and_then(|state| state.days),
            fork_pr_contributor_approval: current
                .fork_pr_approval
                .as_ref()
                .and_then(|state| state.approval_policy.as_deref())
                .and_then(ForkPrApproval::parse),
            access_level: current
                .access
                .as_ref()
                .and_then(|state| state.access_level.as_deref())
                .and_then(AccessLevel::parse),
            fork_pr_workflows_private_repos: current
                .fork_pr_private
                .as_ref()
                .map(|state| ForkPrWorkflowsPrivateRepos {
                    run_workflows_from_fork_pull_requests: state
                        .run_workflows_from_fork_pull_requests,
                    send_write_tokens_to_workflows: state.send_write_tokens_to_workflows,
                    send_secrets_and_variables: state.send_secrets_and_variables,
                    require_approval_for_fork_pr_workflows: state
                        .require_approval_for_fork_pr_workflows,
                })
                // Same again: `fork_pr_workflows_private_repos: {}` says nothing.
                .filter(|fork_pr| *fork_pr != ForkPrWorkflowsPrivateRepos::default()),
        };

        // A repository that answered nothing gets no section, rather than an
        // `actions: {}` block that says nothing either.
        if settings == ActionsSettings::default() {
            return Ok(None);
        }

        Ok(Some(serde_json::to_value(settings).unwrap_or(Value::Null)))
    }
}

/// Read one group, treating "does not apply here" as absent.
///
/// `GitHubClientExt::send_optional` maps only `404`, which is not enough here:
/// GitHub answers `409` for the allow list while the policy is not `selected`,
/// and `422` for the two private-repository endpoints on a public repository.
/// Both are statements about the repository rather than about the request — a
/// `GET` carries no body to be unprocessable — and both mean the group is not
/// there to be compared against.
///
/// `403` deliberately still propagates: it is the shape a missing permission
/// takes, and hiding that would turn a credential problem into a silent no-op.
async fn read<T: DeserializeOwned>(
    client: &dyn GitHubClient,
    request: Request,
) -> GitHubResult<Option<T>> {
    match client.send::<T>(request).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if matches!(error.status(), Some(404 | 409 | 422)) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
