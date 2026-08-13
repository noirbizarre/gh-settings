//! GitHub Actions general settings model.
//!
//! # One section, seven endpoints
//!
//! GitHub's *Settings → Actions → General* page is a single screen, but the REST
//! API splits it across seven sibling endpoints under
//! `/repos/{owner}/{repo}/actions/permissions`. The configuration follows the
//! screen rather than the API — one `actions:` section — and the resource does
//! the fanning out. Field names are the API's own, so there is no mapping table
//! to keep in step; the only nesting is where the API itself groups.
//!
//! # Absent is unmanaged
//!
//! Every field is an `Option`, and an omitted one is left alone. There is no
//! `Nullable` here: none of these settings has a "cleared" state, only values.
//!
//! # Normalisation
//!
//! Every enum is a lowercase string on both sides, and GitHub has been
//! consistent about that. It is still normalised — trimmed and lowercased —
//! because a value that arrives in another case would become a difference that
//! could never be applied away (ADR-002).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Finding;
use crate::resources::ValidateCtx;

/// The widest retention window GitHub documents anywhere.
///
/// Deliberately not "the limit": the real ceiling is per repository and depends
/// on visibility, plan and organisation policy. A *public* personal repository
/// reported `maximum_allowed_days: 90` in testing, not the 400 the documentation
/// quotes for public repositories — so any figure hard-coded here would be
/// wrong somewhere.
///
/// Validation therefore only rejects what is absurd under every configuration,
/// and a value this repository happens not to allow comes back as GitHub's own
/// `409` at apply time, naming the endpoint. The alternative — refusing a value
/// GitHub would have accepted — is the error that cannot be argued with.
pub const MAX_RETENTION_DAYS: u32 = 400;

/// Which actions and reusable workflows may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AllowedActions {
    /// Any action or reusable workflow.
    All,
    /// Only those defined in this repository.
    LocalOnly,
    /// Only those matching the `selected_actions` allow list.
    Selected,
}

/// Default permissions granted to `GITHUB_TOKEN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPermissions {
    /// Read-only access to repository contents.
    Read,
    /// Read and write access to all scopes.
    Write,
}

/// When a fork pull request needs a maintainer's approval before workflows run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForkPrApproval {
    /// Contributors new to GitHub itself.
    FirstTimeContributorsNewToGithub,
    /// First-time contributors to this repository.
    FirstTimeContributors,
    /// Everyone who is not a collaborator.
    AllExternalContributors,
}

/// How far outside the repository its actions and reusable workflows are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// Only workflows in this repository.
    None,
    /// Also the owner's other private repositories.
    User,
    /// Also the rest of the organisation.
    Organization,
}

macro_rules! api_str {
    ($type:ty { $($variant:ident => $value:literal),+ $(,)? }) => {
        impl $type {
            /// The value as the API spells it.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }

            /// Parse a normalised API value, ignoring anything unrecognised.
            ///
            /// Unrecognised rather than an error: a value GitHub adds later must
            /// not turn every `export` of every repository into a failure.
            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

api_str!(AllowedActions {
    All => "all",
    LocalOnly => "local_only",
    Selected => "selected",
});

api_str!(WorkflowPermissions {
    Read => "read",
    Write => "write",
});

api_str!(ForkPrApproval {
    FirstTimeContributorsNewToGithub => "first_time_contributors_new_to_github",
    FirstTimeContributors => "first_time_contributors",
    AllExternalContributors => "all_external_contributors",
});

api_str!(AccessLevel {
    None => "none",
    User => "user",
    Organization => "organization",
});

/// The allow list consulted when `allowed_actions` is `selected`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectedActions {
    /// Whether actions published by GitHub itself are allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_owned_allowed: Option<bool>,

    /// Whether actions by GitHub Marketplace verified creators are allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_allowed: Option<bool>,

    /// Patterns naming further actions and reusable workflows to allow.
    ///
    /// Wildcards, tags and SHAs are accepted, for example `monalisa/octocat@*`
    /// or `docker/*`. GitHub only applies this list to public repositories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patterns_allowed: Option<Vec<String>>,
}

/// Fork pull request behaviour on private repositories.
///
/// GitHub exposes this endpoint for private repositories only; on a public one
/// it answers `404`. It can also be locked by an enterprise owner, which comes
/// back as `403`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkPrWorkflowsPrivateRepos {
    /// Whether fork pull requests may run workflows at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_workflows_from_fork_pull_requests: Option<bool>,

    /// Whether those workflows receive a write-capable `GITHUB_TOKEN`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_write_tokens_to_workflows: Option<bool>,

    /// Whether those workflows can read secrets and variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_secrets_and_variables: Option<bool>,

    /// Whether those workflows need a maintainer's approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_approval_for_fork_pr_workflows: Option<bool>,
}

/// The `actions` configuration section.
///
/// Everything on *Settings → Actions → General*. An omitted field is left alone,
/// and an omitted section means the whole page is unmanaged.
///
/// ```yaml
/// actions:
///   enabled: true
///   allowed_actions: selected
///   selected_actions:
///     github_owned_allowed: true
///     verified_allowed: false
///     patterns_allowed:
///       - docker/*
///   default_workflow_permissions: read
///   can_approve_pull_request_reviews: false
///   artifact_and_log_retention_days: 90
///   fork_pr_contributor_approval: first_time_contributors
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionsSettings {
    /// Whether GitHub Actions runs on this repository at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Which actions and reusable workflows are allowed to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_actions: Option<AllowedActions>,

    /// Whether actions must be referenced by a full-length commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha_pinning_required: Option<bool>,

    /// The allow list, meaningful only with `allowed_actions: selected`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_actions: Option<SelectedActions>,

    /// Default permissions granted to `GITHUB_TOKEN` in a workflow run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_workflow_permissions: Option<WorkflowPermissions>,

    /// Whether workflows may approve pull requests.
    ///
    /// Enabling this lets a workflow satisfy a review requirement, so a change
    /// that can push to the repository can also approve itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_approve_pull_request_reviews: Option<bool>,

    /// How many days artifacts and logs are kept.
    ///
    /// The ceiling is not a fixed number: it depends on the repository's
    /// visibility, the plan and any organisation policy, and GitHub reports it
    /// as `maximum_allowed_days` on the same endpoint. A value above it is
    /// refused at apply time rather than guessed at here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_and_log_retention_days: Option<u32>,

    /// Which fork pull requests need approval before their workflows run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_pr_contributor_approval: Option<ForkPrApproval>,

    /// How far outside this repository its actions and reusable workflows are
    /// visible.
    ///
    /// Private repositories only. GitHub answers `404` for a public one, where
    /// everything is visible to everyone by definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_level: Option<AccessLevel>,

    /// Fork pull request behaviour, on private repositories only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_pr_workflows_private_repos: Option<ForkPrWorkflowsPrivateRepos>,
}

/// Normalise an enum value the API reported.
pub fn normalize_enum(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

/// Validate the actions section.
pub fn validate(settings: &ActionsSettings, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    // GitHub answers a `PUT /selected-actions` with a 422 unless the policy is
    // `selected`, and the message does not say why.
    if settings.selected_actions.is_some()
        && let Some(policy) = settings.allowed_actions
        && policy != AllowedActions::Selected
    {
        findings.push(
            Finding::error(
                "gh_settings::actions::allow_list_without_selected",
                "`selected_actions` needs `allowed_actions: selected`",
            )
            .at(ctx.key_span("actions.selected_actions"))
            .labelled(format!("ignored while the policy is `{}`", policy.as_str()))
            .help("set `allowed_actions: selected`, or remove the allow list"),
        );
    }

    // An allow list declared with no policy at all is not an error — the policy
    // may already be `selected` on the repository — but it is worth saying,
    // because the far likelier reading is that the policy was forgotten.
    if settings.selected_actions.is_some() && settings.allowed_actions.is_none() {
        findings.push(
            Finding::warning(
                "gh_settings::actions::allow_list_without_policy",
                "`selected_actions` is declared without `allowed_actions`",
            )
            .at(ctx.key_span("actions.selected_actions"))
            .labelled("only applies when the policy is `selected`")
            .help(
                "the allow list is still written, but GitHub ignores it unless the repository's \
                 policy is already `selected`; declare `allowed_actions: selected` to be sure",
            ),
        );
    }

    if let Some(days) = settings.artifact_and_log_retention_days
        && (days == 0 || days > MAX_RETENTION_DAYS)
    {
        findings.push(
            Finding::error(
                "gh_settings::actions::retention_out_of_range",
                format!("`{days}` is not a valid retention period"),
            )
            .at(ctx.span("actions.artifact_and_log_retention_days"))
            .labelled("outside the accepted range")
            .help(format!(
                "the period must be between 1 and {MAX_RETENTION_DAYS} days; the ceiling for this \
                 repository may be lower still, and GitHub reports it as `maximum_allowed_days` \
                 on `actions/permissions/artifact-and-log-retention`"
            )),
        );
    }

    findings
}
