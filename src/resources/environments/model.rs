//! The deployment environment model.
//!
//! # Two shapes for one thing
//!
//! `PUT .../environments/{name}` accepts a flat body — `wait_timer`,
//! `prevent_self_review`, `reviewers`, `deployment_branch_policy` — but `GET`
//! returns the same information as a heterogeneous `protection_rules` array,
//! one entry per rule *that is set*. [`EnvironmentState::as_environment`] folds
//! the array back into the flat shape, which is where most of the
//! normalisation in this module lives.
//!
//! # Normalisation
//!
//! Three traps, each of which produces a permanent diff if missed:
//!
//! * `wait_timer: 0` is not a rule GitHub stores, so an absent `wait_timer`
//!   rule and an explicit zero are the same state.
//! * `prevent_self_review` lives *on* the `required_reviewers` rule. With no
//!   reviewers there is no rule, and therefore nowhere for the flag to come
//!   back from — so it is only meaningful when reviewers exist.
//! * Reviewers come back in arbitrary order, so they are compared as a sorted
//!   set of resolved identifiers rather than as a list.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Finding;
use crate::github::{GitHubClient, Resolver, Result as GitHubResult};
use crate::resources::repository::model::{Nullable, double_option};
use crate::resources::variables::model::{self as variables, Variable};
use crate::resources::{FieldDiff, ValidateCtx};

/// GitHub's cap on `wait_timer`, in minutes (30 days).
const MAX_WAIT_TIMER: u32 = 43200;

/// A deployment environment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// The environment name, for example `production`.
    ///
    /// GitHub matches names case-insensitively, so `Production` and
    /// `production` are the same environment. Names may contain spaces and
    /// slashes.
    pub name: String,

    /// Minutes to wait before a deployment to this environment may proceed.
    ///
    /// Between 0 and 43200 (30 days). Zero and an omitted timer are the same
    /// state to GitHub, so setting it back to zero removes the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_timer: Option<u32>,

    /// Whether the user who triggered a deployment may approve it themselves.
    ///
    /// Only meaningful alongside `reviewers`: with nobody to review, there is
    /// no approval to prevent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prevent_self_review: Option<bool>,

    /// Who must approve a deployment to this environment.
    ///
    /// Omitted leaves reviewers unmanaged; an empty list removes them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewers: Option<Vec<Reviewer>>,

    /// Which refs may deploy to this environment.
    ///
    /// Three states, and the difference between the last two matters: omitting
    /// the field leaves the policy alone, whereas an explicit `null` sets it to
    /// *any branch*, which is GitHub's own default and a real setting.
    ///
    /// ```yaml
    /// deployment_branch_policy: protected      # protected branches only
    /// deployment_branch_policy:                # explicit patterns
    ///   branches: [main, "release/*"]
    ///   tags: ["v*"]
    /// deployment_branch_policy: null           # any branch
    /// ```
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub deployment_branch_policy: Nullable<DeploymentBranchPolicy>,

    /// Actions variables scoped to this environment.
    ///
    /// Omitting the key leaves the environment's variables unmanaged; an empty
    /// list declares that it should have none. Whether unmanaged variables are
    /// deleted is governed by the `environments` section's `prune` flag, since
    /// a variable cannot be pruned independently of the environment holding it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<Vec<Variable>>,
}

/// Who must approve a deployment.
///
/// Declared by login or slug rather than by numeric identifier: identifiers are
/// neither stable across organisations nor meaningful to a human, which would
/// make an exported configuration useless anywhere but its origin. Resolution
/// to identifiers happens in `prepare`, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reviewer {
    /// A user login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// An organisation team slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,

    /// Resolved identifier.
    ///
    /// Filled in during planning; there is no reason to write one by hand, and
    /// an exported configuration never contains one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub id: Option<u64>,
}

impl Reviewer {
    /// A user reviewer.
    pub fn user(login: impl Into<String>) -> Self {
        Self {
            user: Some(login.into()),
            team: None,
            id: None,
        }
    }

    /// A team reviewer.
    pub fn team(slug: impl Into<String>) -> Self {
        Self {
            user: None,
            team: Some(slug.into()),
            id: None,
        }
    }

    /// The API's spelling of the actor type.
    pub fn kind(&self) -> &'static str {
        if self.team.is_some() { "Team" } else { "User" }
    }

    /// The declared name, whichever kind it is.
    pub fn slug(&self) -> &str {
        self.team
            .as_deref()
            .or(self.user.as_deref())
            .unwrap_or_default()
    }

    /// Whether an identifier still has to be looked up.
    pub fn needs_resolution(&self) -> bool {
        self.id.is_none() && (self.user.is_some() || self.team.is_some())
    }

    /// Look the identifier up, unless it is already known.
    pub async fn resolve(
        &mut self,
        client: &dyn GitHubClient,
        owner: &str,
        resolver: &Resolver,
    ) -> GitHubResult<()> {
        if !self.needs_resolution() {
            return Ok(());
        }
        self.id = Some(match (&self.user, &self.team) {
            (Some(login), _) => resolver.user(client, login).await?,
            (_, Some(slug)) => resolver.team(client, owner, slug).await?,
            _ => return Ok(()),
        });
        Ok(())
    }

    /// The `{type, id}` pair the API wants.
    fn as_body(&self) -> Value {
        json!({ "type": self.kind(), "id": self.id })
    }

    /// The value compared between the two sides.
    ///
    /// Identifiers, not names: the API answers with whatever the actor is
    /// called *now*, and comparing names would report an update every run for
    /// anybody who has since changed their login.
    fn identity(&self) -> (&'static str, u64) {
        (self.kind(), self.id.unwrap_or_default())
    }

    /// Rendering used in field diffs and summaries.
    fn label(&self) -> String {
        match (&self.user, &self.team) {
            (Some(login), _) => format!("@{login}"),
            (_, Some(slug)) => format!("team {slug}"),
            _ => "unknown".into(),
        }
    }
}

/// Which refs may deploy to an environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DeploymentBranchPolicy {
    /// Only branches with protection rules or a matching ruleset.
    Protected(ProtectedKeyword),

    /// Explicit branch and tag name patterns.
    Custom {
        /// Branch name patterns, for example `main` or `release/*`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        branches: Vec<String>,
        /// Tag name patterns, for example `v*`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    },
}

/// The only word accepted where a policy keyword is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedKeyword {
    /// Restrict deployments to protected branches.
    Protected,
}

impl DeploymentBranchPolicy {
    /// The `{protected_branches, custom_branch_policies}` pair, which is all the
    /// environment endpoint carries — the patterns themselves live behind a
    /// second endpoint.
    fn as_body(&self) -> Value {
        let custom = matches!(self, Self::Custom { .. });
        json!({ "protected_branches": !custom, "custom_branch_policies": custom })
    }

    /// The patterns this policy declares, in canonical order.
    pub fn patterns(&self) -> Vec<Pattern> {
        let Self::Custom { branches, tags } = self else {
            return Vec::new();
        };
        let mut patterns: Vec<Pattern> = branches
            .iter()
            .map(|name| Pattern::branch(name.trim()))
            .chain(tags.iter().map(|name| Pattern::tag(name.trim())))
            .collect();
        patterns.sort();
        patterns.dedup();
        patterns
    }

    /// Rendering used in field diffs.
    fn label(&self) -> String {
        match self {
            Self::Protected(_) => "protected".into(),
            Self::Custom { .. } => {
                let names: Vec<String> = self
                    .patterns()
                    .iter()
                    .map(|pattern| pattern.label())
                    .collect();
                format!("custom ({})", names.join(", "))
            }
        }
    }
}

/// A single branch or tag pattern in a custom deployment branch policy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pattern {
    /// `branch` or `tag`, as the API spells it.
    pub r#type: String,
    /// The pattern itself.
    pub name: String,
}

impl Pattern {
    /// A branch pattern.
    pub fn branch(name: impl Into<String>) -> Self {
        Self {
            r#type: "branch".into(),
            name: name.into(),
        }
    }

    /// A tag pattern.
    pub fn tag(name: impl Into<String>) -> Self {
        Self {
            r#type: "tag".into(),
            name: name.into(),
        }
    }

    /// Body for `POST .../deployment-branch-policies`.
    pub fn as_body(&self) -> Value {
        json!({ "name": self.name, "type": self.r#type })
    }

    /// Rendering used in field diffs.
    fn label(&self) -> String {
        format!("{} {}", self.r#type, self.name)
    }
}

impl Environment {
    /// Build an environment.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            wait_timer: None,
            prevent_self_review: None,
            reviewers: None,
            deployment_branch_policy: None,
            variables: None,
        }
    }

    /// The comparable form.
    ///
    /// `wait_timer: 0` is normalised away entirely rather than to `Some(0)`:
    /// the two sides then agree whether the file says nothing, says zero, or
    /// the server reports no rule.
    pub fn normalized(&self) -> Self {
        let reviewers = self.reviewers.as_ref().map(|reviewers| {
            let mut reviewers = reviewers.clone();
            reviewers.sort_by(|left, right| left.identity().cmp(&right.identity()));
            reviewers
        });

        // With nobody to review there is no `required_reviewers` rule for the
        // flag to live on, so an explicit `true` would otherwise be reported as
        // an update on every single run.
        let prevent_self_review = match &reviewers {
            Some(reviewers) if reviewers.is_empty() => None,
            _ => self.prevent_self_review,
        };

        Self {
            name: self.name.trim().to_string(),
            wait_timer: self.wait_timer.filter(|timer| *timer > 0),
            prevent_self_review,
            reviewers,
            deployment_branch_policy: self.deployment_branch_policy.clone(),
            // Variables belong to the `Variables` resource; they are carried
            // here only so the configuration can nest them, and comparing them
            // in two places would produce the same change twice.
            variables: None,
        }
    }

    /// The environment as it will be once the change has been applied.
    pub fn applied(&self, current: &Self) -> Self {
        Self {
            name: self.name.clone(),
            wait_timer: self.wait_timer.or(current.wait_timer),
            prevent_self_review: self.prevent_self_review.or(current.prevent_self_review),
            reviewers: self.reviewers.clone().or_else(|| current.reviewers.clone()),
            deployment_branch_policy: self
                .deployment_branch_policy
                .clone()
                .or_else(|| current.deployment_branch_policy.clone()),
            variables: None,
        }
    }

    /// Body for `PUT .../environments/{name}`.
    ///
    /// Every managed field is sent, because the endpoint replaces the
    /// environment wholesale; unmanaged ones are filled from `current` so that
    /// omitting a field in the file leaves it alone rather than clearing it.
    pub fn as_body(&self, current: &Self) -> Value {
        let effective = self.applied(current);
        let mut body = serde_json::Map::new();

        // Always sent: there is no way to say "leave the timer as it is", so
        // the effective value has to be restated on every write.
        body.insert(
            "wait_timer".into(),
            json!(effective.wait_timer.unwrap_or(0)),
        );

        let reviewers = effective.reviewers.clone().unwrap_or_default();
        body.insert(
            "prevent_self_review".into(),
            json!(effective.prevent_self_review.unwrap_or(false) && !reviewers.is_empty()),
        );
        body.insert(
            "reviewers".into(),
            Value::Array(reviewers.iter().map(Reviewer::as_body).collect()),
        );

        // `Nullable`: the outer `None` means unmanaged and is only reachable
        // when the current state has no policy either, in which case `null` is
        // what the server already holds.
        body.insert(
            "deployment_branch_policy".into(),
            match effective.deployment_branch_policy.flatten() {
                Some(policy) => policy.as_body(),
                None => Value::Null,
            },
        );

        Value::Object(body)
    }

    /// Fields reported when the environment is created.
    pub fn as_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        if let Some(timer) = self.wait_timer {
            fields.push(FieldDiff::added("wait_timer", timer.to_string()));
        }
        if let Some(flag) = self.prevent_self_review {
            fields.push(FieldDiff::added("prevent_self_review", flag.to_string()));
        }
        if let Some(reviewers) = &self.reviewers {
            fields.push(FieldDiff::added("reviewers", render_reviewers(reviewers)));
        }
        if let Some(policy) = &self.deployment_branch_policy {
            fields.push(FieldDiff::added(
                "deployment_branch_policy",
                render_policy(policy.as_ref()),
            ));
        }
        fields
    }

    /// Differences against what exists, or an empty vector when they agree.
    ///
    /// Only fields the configuration declares are compared: an omitted field is
    /// unmanaged and must never be reset to a default.
    pub fn diff_against(&self, current: &Self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();

        if let Some(timer) = self.wait_timer.filter(|timer| *timer > 0)
            && Some(timer) != current.wait_timer
        {
            fields.push(FieldDiff::changed(
                "wait_timer",
                current.wait_timer.unwrap_or(0).to_string(),
                timer.to_string(),
            ));
        } else if self.wait_timer == Some(0) && current.wait_timer.is_some() {
            fields.push(FieldDiff::changed(
                "wait_timer",
                current.wait_timer.unwrap_or(0).to_string(),
                "0",
            ));
        }

        if let Some(reviewers) = &self.reviewers {
            let current_reviewers = current.reviewers.clone().unwrap_or_default();
            let desired: Vec<_> = reviewers.iter().map(Reviewer::identity).collect();
            let existing: Vec<_> = current_reviewers.iter().map(Reviewer::identity).collect();
            if desired != existing {
                fields.push(FieldDiff::changed(
                    "reviewers",
                    render_reviewers(&current_reviewers),
                    render_reviewers(reviewers),
                ));
            }
        }

        // Only compared where reviewers exist on both sides, because with none
        // the flag has no rule to live on and the API never reports it back.
        let reviewers_exist = !self
            .reviewers
            .clone()
            .or_else(|| current.reviewers.clone())
            .unwrap_or_default()
            .is_empty();
        if let Some(flag) = self.prevent_self_review
            && reviewers_exist
            && Some(flag) != current.prevent_self_review
        {
            fields.push(FieldDiff::changed(
                "prevent_self_review",
                current.prevent_self_review.unwrap_or(false).to_string(),
                flag.to_string(),
            ));
        }

        if let Some(policy) = &self.deployment_branch_policy {
            let current_policy = current.deployment_branch_policy.clone().flatten();
            if policy.as_ref() != current_policy.as_ref() {
                fields.push(FieldDiff::changed(
                    "deployment_branch_policy",
                    render_policy(current_policy.as_ref()),
                    render_policy(policy.as_ref()),
                ));
            }
        }

        fields
    }
}

/// Rendering for a reviewer list.
fn render_reviewers(reviewers: &[Reviewer]) -> String {
    if reviewers.is_empty() {
        return "none".into();
    }
    reviewers
        .iter()
        .map(Reviewer::label)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Rendering for a branch policy, including the `null` state.
fn render_policy(policy: Option<&DeploymentBranchPolicy>) -> String {
    policy.map_or_else(|| "any branch".into(), DeploymentBranchPolicy::label)
}

/// An environment as the API returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentState {
    /// The environment name, as GitHub spells it.
    pub name: String,
    /// Protection rules, one entry per rule that is set.
    #[serde(default)]
    pub protection_rules: Vec<ProtectionRule>,
    /// The branch policy flags, absent when any branch may deploy.
    #[serde(default)]
    pub deployment_branch_policy: Option<BranchPolicyFlags>,
}

/// One entry of the `protection_rules` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtectionRule {
    /// A delay before deployments may proceed.
    WaitTimer {
        /// The delay, in minutes.
        #[serde(default)]
        wait_timer: u32,
    },
    /// An approval requirement.
    RequiredReviewers {
        /// Whether the triggering user may approve their own deployment.
        #[serde(default)]
        prevent_self_review: bool,
        /// Who may approve.
        #[serde(default)]
        reviewers: Vec<ReviewerState>,
    },
    /// A marker that a branch policy is in force; the policy itself is reported
    /// by `deployment_branch_policy`, so nothing is carried here.
    BranchPolicy {},
}

/// A reviewer as the API returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewerState {
    /// `User` or `Team`.
    pub r#type: String,
    /// The actor.
    pub reviewer: ActorState,
}

/// The identity of a reviewing actor.
#[derive(Debug, Clone, Deserialize)]
pub struct ActorState {
    /// Numeric identifier.
    pub id: u64,
    /// User login, for a `User`.
    #[serde(default)]
    pub login: Option<String>,
    /// Team slug, for a `Team`.
    #[serde(default)]
    pub slug: Option<String>,
}

/// The two flags the environment endpoint carries for a branch policy.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct BranchPolicyFlags {
    /// Restrict deployments to protected branches.
    #[serde(default)]
    pub protected_branches: bool,
    /// Restrict deployments to explicit patterns.
    #[serde(default)]
    pub custom_branch_policies: bool,
}

/// The envelope the environments endpoint returns.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnvironmentPage {
    /// The environments on the page.
    #[serde(default)]
    pub environments: Vec<EnvironmentState>,
}

impl EnvironmentState {
    /// Whether the second endpoint has to be consulted for patterns.
    pub fn has_custom_branch_policies(&self) -> bool {
        self.deployment_branch_policy
            .is_some_and(|flags| flags.custom_branch_policies)
    }

    /// Fold `protection_rules` back into the flat configuration shape.
    ///
    /// Custom policies come back without their patterns, which live behind a
    /// second endpoint; the caller fills those in.
    pub fn as_environment(&self) -> Environment {
        let mut environment = Environment::new(self.name.trim());

        for rule in &self.protection_rules {
            match rule {
                ProtectionRule::WaitTimer { wait_timer } => {
                    environment.wait_timer = Some(*wait_timer).filter(|timer| *timer > 0);
                }
                ProtectionRule::RequiredReviewers {
                    prevent_self_review,
                    reviewers,
                } => {
                    let mut reviewers: Vec<Reviewer> =
                        reviewers.iter().map(ReviewerState::as_reviewer).collect();
                    reviewers.sort_by(|left, right| left.identity().cmp(&right.identity()));
                    environment.prevent_self_review = Some(*prevent_self_review);
                    environment.reviewers = Some(reviewers);
                }
                ProtectionRule::BranchPolicy {} => {}
            }
        }

        // No `required_reviewers` rule means nobody reviews, which is a state,
        // not an absence — the file that declares `reviewers: []` agrees with
        // it and must not report a change.
        if environment.reviewers.is_none() {
            environment.reviewers = Some(Vec::new());
            environment.prevent_self_review = None;
        }

        environment.deployment_branch_policy = Some(match self.deployment_branch_policy {
            Some(flags) if flags.custom_branch_policies => Some(DeploymentBranchPolicy::Custom {
                branches: Vec::new(),
                tags: Vec::new(),
            }),
            Some(flags) if flags.protected_branches => Some(DeploymentBranchPolicy::Protected(
                ProtectedKeyword::Protected,
            )),
            _ => None,
        });

        environment
    }
}

impl ReviewerState {
    /// The configuration form, named rather than numbered.
    pub fn as_reviewer(&self) -> Reviewer {
        Reviewer {
            user: if self.r#type == "User" {
                self.reviewer.login.clone()
            } else {
                None
            },
            team: if self.r#type == "Team" {
                self.reviewer.slug.clone()
            } else {
                None
            },
            id: Some(self.reviewer.id),
        }
    }
}

/// A branch policy pattern as the API returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct PatternState {
    /// Server identifier, needed to delete it.
    pub id: u64,
    /// The pattern.
    pub name: String,
    /// `branch` or `tag`. Absent on older responses, where everything was a
    /// branch.
    #[serde(default)]
    pub r#type: Option<String>,
}

/// The envelope the deployment-branch-policies endpoint returns.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PatternPage {
    /// The patterns on the page.
    #[serde(default)]
    pub branch_policies: Vec<PatternState>,
}

impl PatternState {
    /// The comparable form.
    pub fn as_pattern(&self) -> Pattern {
        Pattern {
            r#type: self.r#type.clone().unwrap_or_else(|| "branch".into()),
            name: self.name.clone(),
        }
    }
}

/// The key under which an environment is matched.
///
/// GitHub matches environment names case-insensitively, so a case-only change
/// must be an update rather than a create that then fails with a 422.
pub fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Validate the desired environments.
pub fn validate(environments: &[Environment], ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (position, environment) in environments.iter().enumerate() {
        let path = format!("environments.{position}");

        if environment.name.trim().is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::environments::empty_name",
                    "environment name cannot be empty",
                )
                .at(ctx.span(&format!("{path}.name")))
                .labelled("empty name"),
            );
        }

        if let Some(previous) = seen.insert(key(&environment.name), position) {
            findings.push(
                Finding::error(
                    "gh_settings::environments::duplicate",
                    format!(
                        "environment `{}` is declared more than once",
                        environment.name
                    ),
                )
                .at(ctx.span(&format!("{path}.name")))
                .labelled(format!("already declared at environments.{previous}"))
                .help("environment names are case-insensitive on GitHub; remove the duplicate"),
            );
        }

        if let Some(timer) = environment.wait_timer
            && timer > MAX_WAIT_TIMER
        {
            findings.push(
                Finding::error(
                    "gh_settings::environments::wait_timer_too_long",
                    format!("`wait_timer` is {timer} minutes, the maximum is {MAX_WAIT_TIMER}"),
                )
                .at(ctx.span(&format!("{path}.wait_timer")))
                .labelled("too long")
                .help("43200 minutes is 30 days"),
            );
        }

        // A flag with no rule to live on does nothing, silently — worth saying,
        // because the file reads as though it does something.
        if environment.prevent_self_review == Some(true)
            && environment
                .reviewers
                .as_ref()
                .is_some_and(|reviewers| reviewers.is_empty())
        {
            findings.push(
                Finding::warning(
                    "gh_settings::environments::pointless_prevent_self_review",
                    "`prevent_self_review` has no effect without reviewers",
                )
                .at(ctx.span(&format!("{path}.prevent_self_review")))
                .help("add `reviewers`, or remove the flag"),
            );
        }

        for (index, reviewer) in environment.reviewers.iter().flatten().enumerate() {
            let item = format!("{path}.reviewers.{index}");
            match (&reviewer.user, &reviewer.team) {
                (Some(_), Some(_)) => findings.push(
                    Finding::error(
                        "gh_settings::environments::ambiguous_reviewer",
                        "a reviewer is either a user or a team, not both",
                    )
                    .at(ctx.span(&item))
                    .labelled("declares both `user` and `team`"),
                ),
                (None, None) => findings.push(
                    Finding::error(
                        "gh_settings::environments::empty_reviewer",
                        "a reviewer must declare a `user` or a `team`",
                    )
                    .at(ctx.span(&item))
                    .labelled("neither given"),
                ),
                _ => {}
            }
        }

        if let Some(Some(DeploymentBranchPolicy::Custom { branches, tags })) =
            &environment.deployment_branch_policy
            && branches.is_empty()
            && tags.is_empty()
        {
            findings.push(
                Finding::error(
                    "gh_settings::environments::empty_branch_policy",
                    "a custom deployment branch policy needs at least one pattern",
                )
                .at(ctx.span(&format!("{path}.deployment_branch_policy")))
                .help("list `branches` or `tags`, use `protected`, or set the policy to `null`"),
            );
        }

        if let Some(variables) = &environment.variables {
            findings.extend(variables::validate(
                variables,
                &format!("{path}.variables"),
                ctx,
            ));
        }
    }

    findings
}
