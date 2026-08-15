//! Repository metadata.
//!
//! A singleton resource: rather than a collection of items, it produces at most
//! two changes — one `PATCH /repos/{owner}/{repo}` covering the plain fields, and
//! one per security feature, which GitHub exposes through a differently shaped
//! sub-object.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::{Finding, Settings};
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

pub mod model;

pub use model::{
    MergeCommitMessage, MergeCommitTitle, RepositorySettings, SecuritySettings,
    SquashCommitMessage, SquashCommitTitle,
};

/// The `repository` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Repository;

/// Current repository state, as reported by the API.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Current {
    /// Description, `""` when unset.
    #[serde(default)]
    pub description: Option<String>,
    /// Homepage, `""` when unset.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Whether the repository is private.
    #[serde(default)]
    pub private: bool,
    /// Whether issues are enabled.
    #[serde(default)]
    pub has_issues: bool,
    /// Whether the wiki is enabled.
    #[serde(default)]
    pub has_wiki: bool,
    /// Whether projects are enabled.
    #[serde(default)]
    pub has_projects: bool,
    /// Whether discussions are enabled.
    #[serde(default)]
    pub has_discussions: bool,
    /// Whether this is a template repository.
    #[serde(default)]
    pub is_template: bool,
    /// Whether web-based commits must be signed off.
    #[serde(default)]
    pub web_commit_signoff_required: bool,
    /// Whether merge commits are allowed.
    #[serde(default)]
    pub allow_merge_commit: bool,
    /// Whether squash merging is allowed.
    #[serde(default)]
    pub allow_squash_merge: bool,
    /// Whether rebase merging is allowed.
    #[serde(default)]
    pub allow_rebase_merge: bool,
    /// Whether auto-merge is available.
    #[serde(default)]
    pub allow_auto_merge: bool,
    /// Whether branch updates are allowed.
    #[serde(default)]
    pub allow_update_branch: bool,
    /// Whether head branches are deleted on merge.
    #[serde(default)]
    pub delete_branch_on_merge: bool,
    /// Squash commit title source.
    #[serde(default)]
    pub squash_merge_commit_title: Option<String>,
    /// Squash commit message source.
    #[serde(default)]
    pub squash_merge_commit_message: Option<String>,
    /// Merge commit title source.
    #[serde(default)]
    pub merge_commit_title: Option<String>,
    /// Merge commit message source.
    #[serde(default)]
    pub merge_commit_message: Option<String>,
    /// Default branch name.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// Whether anonymous Git read access is enabled.
    ///
    /// `Option`, unlike its neighbours: GitHub Enterprise Server reports the
    /// field and github.com omits it entirely. Defaulting it to `false` would
    /// make every github.com repository look like it had the feature turned off,
    /// which is a different claim from not having it.
    #[serde(default)]
    pub anonymous_access_enabled: Option<bool>,
    /// Whether the repository is archived.
    #[serde(default)]
    pub archived: bool,
    /// Security and analysis features.
    #[serde(default)]
    pub security_and_analysis: Option<Map<String, Value>>,
}

impl Current {
    /// A normalised copy, safe to compare against a normalised counterpart.
    ///
    /// Every other resource normalises what it reads inside `current()`, which
    /// is what the trait promises. This one did not, and normalised two of its
    /// fields inside the diff instead — so a stray space in `default_branch`, or
    /// a merge-commit enum GitHub spelled in a different case, was a difference
    /// that could never be resolved and a plan that never came out empty.
    pub fn normalized(mut self) -> Self {
        fn text(value: Option<String>) -> Option<String> {
            value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        }

        // The API spells these in SCREAMING_SNAKE_CASE, and so does the schema.
        // Upper-casing costs nothing and removes a whole class of phantom diff.
        fn enumeration(value: Option<String>) -> Option<String> {
            text(value).map(|value| value.to_uppercase())
        }

        self.description = text(self.description);
        self.homepage = text(self.homepage);
        self.default_branch = text(self.default_branch);
        self.squash_merge_commit_title = enumeration(self.squash_merge_commit_title);
        self.squash_merge_commit_message = enumeration(self.squash_merge_commit_message);
        self.merge_commit_title = enumeration(self.merge_commit_title);
        self.merge_commit_message = enumeration(self.merge_commit_message);
        self
    }

    /// Whether a security feature is enabled.
    ///
    /// The API shape is `{"secret_scanning": {"status": "enabled"}}`; an absent
    /// key means the feature is unavailable for this repository, which we treat
    /// as disabled.
    pub fn security_enabled(&self, feature: &str) -> bool {
        self.security_and_analysis
            .as_ref()
            .and_then(|features| features.get(feature))
            .and_then(|feature| feature.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|status| status == "enabled")
    }
}

/// Payload of a repository change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    /// A `PATCH /repos/{owner}/{repo}` body.
    Settings(Value),
    /// A `PATCH` targeting `security_and_analysis`.
    Security(Value),
}

#[async_trait]
impl Resource for Repository {
    type Desired = RepositorySettings;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Repository
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ADMINISTRATION
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        settings.repository.clone()
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(desired, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let current: Current = client.send(Request::get(target.endpoint(""))).await?;
        Ok(current.normalized())
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        _prune: &PruneOpts,
    ) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut body = Map::new();
        let mut fields = Vec::new();

        // Nullable text fields, where absent and null differ.
        if let Some(diff) = model::nullable_field(
            "description",
            &desired.description,
            current.description.as_ref(),
            model::normalize_description,
        ) {
            body.insert(
                "description".into(),
                json!(desired.description.clone().flatten().unwrap_or_default()),
            );
            fields.push(diff);
        }

        if let Some(diff) = model::nullable_field(
            "homepage",
            &desired.homepage,
            current.homepage.as_ref(),
            model::normalize_homepage,
        ) {
            body.insert(
                "homepage".into(),
                json!(desired.homepage.clone().flatten().unwrap_or_default()),
            );
            fields.push(diff);
        }

        // Plain booleans.
        let booleans: [(&str, Option<bool>, bool); 13] = [
            ("private", desired.private, current.private),
            ("has_issues", desired.has_issues, current.has_issues),
            ("has_wiki", desired.has_wiki, current.has_wiki),
            ("has_projects", desired.has_projects, current.has_projects),
            (
                "has_discussions",
                desired.has_discussions,
                current.has_discussions,
            ),
            ("is_template", desired.is_template, current.is_template),
            (
                "web_commit_signoff_required",
                desired.web_commit_signoff_required,
                current.web_commit_signoff_required,
            ),
            (
                "allow_merge_commit",
                desired.allow_merge_commit,
                current.allow_merge_commit,
            ),
            (
                "allow_squash_merge",
                desired.allow_squash_merge,
                current.allow_squash_merge,
            ),
            (
                "allow_rebase_merge",
                desired.allow_rebase_merge,
                current.allow_rebase_merge,
            ),
            (
                "allow_auto_merge",
                desired.allow_auto_merge,
                current.allow_auto_merge,
            ),
            (
                "allow_update_branch",
                desired.allow_update_branch,
                current.allow_update_branch,
            ),
            (
                "delete_branch_on_merge",
                desired.delete_branch_on_merge,
                current.delete_branch_on_merge,
            ),
        ];

        for (name, desired_value, current_value) in booleans {
            let Some(desired_value) = desired_value else {
                continue;
            };
            if desired_value != current_value {
                body.insert(name.into(), json!(desired_value));
                fields.push(FieldDiff::changed(
                    name,
                    current_value.to_string(),
                    desired_value.to_string(),
                ));
            }
        }

        // Archiving is one-way through the API, so only `true` is ever applied.
        if desired.archived == Some(true) && !current.archived {
            body.insert("archived".into(), json!(true));
            fields.push(FieldDiff::changed("archived", "false", "true"));
        }

        // Not in the array above because the current value is genuinely optional:
        // github.com does not report this field at all. Sending it anyway is the
        // only way to manage it on Enterprise Server, and it was previously
        // accepted by the schema, documented, and then silently ignored.
        if let Some(desired_value) = desired.anonymous_access_enabled
            && current.anonymous_access_enabled != Some(desired_value)
        {
            body.insert("anonymous_access_enabled".into(), json!(desired_value));
            fields.push(FieldDiff::changed(
                "anonymous_access_enabled",
                current
                    .anonymous_access_enabled
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "(not reported)".into()),
                desired_value.to_string(),
            ));
        }

        // Enum-valued merge commit options.
        let enums: [(&str, Option<String>, Option<&String>); 4] = [
            (
                "squash_merge_commit_title",
                desired
                    .squash_merge_commit_title
                    .map(|value| enum_value(&value)),
                current.squash_merge_commit_title.as_ref(),
            ),
            (
                "squash_merge_commit_message",
                desired
                    .squash_merge_commit_message
                    .map(|value| enum_value(&value)),
                current.squash_merge_commit_message.as_ref(),
            ),
            (
                "merge_commit_title",
                desired.merge_commit_title.map(|value| enum_value(&value)),
                current.merge_commit_title.as_ref(),
            ),
            (
                "merge_commit_message",
                desired.merge_commit_message.map(|value| enum_value(&value)),
                current.merge_commit_message.as_ref(),
            ),
        ];

        for (name, desired_value, current_value) in enums {
            let Some(desired_value) = desired_value else {
                continue;
            };
            if Some(&desired_value) != current_value {
                body.insert(name.into(), json!(desired_value));
                fields.push(FieldDiff::changed(
                    name,
                    current_value.cloned().unwrap_or_else(|| "(unset)".into()),
                    &desired_value,
                ));
            }
        }

        // Both sides normalised: `current` was trimmed on the way in, so trim
        // what the user wrote too rather than comparing a trimmed value against
        // an untrimmed one.
        if let Some(default_branch) = desired.default_branch.as_deref().map(str::trim)
            && !default_branch.is_empty()
            && Some(default_branch) != current.default_branch.as_deref()
        {
            body.insert("default_branch".into(), json!(default_branch));
            fields.push(FieldDiff::changed(
                "default_branch",
                current
                    .default_branch
                    .clone()
                    .unwrap_or_else(|| "(unknown)".into()),
                default_branch,
            ));
        }

        if !body.is_empty() {
            let summary = if fields.len() == 1 {
                format!("update repository {}", fields[0].field)
            } else {
                format!("update repository ({} fields)", fields.len())
            };
            changes.push(
                Change::new(ResourceId::Repository, Op::Update, "settings")
                    .summary(summary)
                    .fields(fields)
                    .payload(Payload::Settings(Value::Object(body))),
            );
        }

        // Security features live under a differently shaped sub-object and are
        // rejected when sent alongside ordinary fields, so they get their own
        // change.
        if let Some(security) = &desired.security {
            let mut security_body = Map::new();
            let mut security_fields = Vec::new();

            for (feature, enabled) in security.declared() {
                let current_enabled = current.security_enabled(feature);
                if enabled != current_enabled {
                    security_body.insert(
                        feature.into(),
                        json!({ "status": if enabled { "enabled" } else { "disabled" } }),
                    );
                    security_fields.push(FieldDiff::changed(
                        feature,
                        current_enabled.to_string(),
                        enabled.to_string(),
                    ));
                }
            }

            if !security_body.is_empty() {
                changes.push(
                    Change::new(ResourceId::Repository, Op::Update, "security")
                        .summary(format!(
                            "update repository security ({} features)",
                            security_fields.len()
                        ))
                        .fields(security_fields)
                        .payload(Payload::Security(
                            json!({ "security_and_analysis": Value::Object(security_body) }),
                        )),
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
            panic!("repository change carried an undecodable payload: {error}")
        });

        let body = match payload {
            Payload::Settings(body) | Payload::Security(body) => body,
        };

        client
            .execute(Request::patch(target.endpoint(""), body))
            .await
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;

        let settings = RepositorySettings {
            description: Some(
                current
                    .description
                    .as_deref()
                    .and_then(model::normalize_description),
            ),
            homepage: Some(
                current
                    .homepage
                    .as_deref()
                    .and_then(model::normalize_homepage),
            ),
            topics: None,
            private: Some(current.private),
            has_issues: Some(current.has_issues),
            has_wiki: Some(current.has_wiki),
            has_projects: Some(current.has_projects),
            has_discussions: Some(current.has_discussions),
            is_template: Some(current.is_template),
            web_commit_signoff_required: Some(current.web_commit_signoff_required),
            allow_merge_commit: Some(current.allow_merge_commit),
            allow_squash_merge: Some(current.allow_squash_merge),
            allow_rebase_merge: Some(current.allow_rebase_merge),
            allow_auto_merge: Some(current.allow_auto_merge),
            allow_update_branch: Some(current.allow_update_branch),
            delete_branch_on_merge: Some(current.delete_branch_on_merge),
            squash_merge_commit_title: current
                .squash_merge_commit_title
                .as_deref()
                .and_then(parse_enum),
            squash_merge_commit_message: current
                .squash_merge_commit_message
                .as_deref()
                .and_then(parse_enum),
            merge_commit_title: current.merge_commit_title.as_deref().and_then(parse_enum),
            merge_commit_message: current.merge_commit_message.as_deref().and_then(parse_enum),
            default_branch: current.default_branch.clone(),
            // Only present when GitHub reported it, which means Enterprise
            // Server. Exporting `false` on github.com would invent a setting.
            anonymous_access_enabled: current.anonymous_access_enabled,
            // Never export `archived`: doing so would put a one-way, destructive
            // flag into a file people copy between repositories.
            archived: None,
            security: export_security(&current),
        };

        Ok(Some(serde_json::to_value(settings).unwrap_or(Value::Null)))
    }
}

/// Export the security block, omitting it when the API reported nothing.
fn export_security(current: &Current) -> Option<SecuritySettings> {
    let features = current.security_and_analysis.as_ref()?;
    if features.is_empty() {
        return None;
    }
    Some(SecuritySettings {
        advanced_security: features
            .contains_key("advanced_security")
            .then(|| current.security_enabled("advanced_security")),
        secret_scanning: features
            .contains_key("secret_scanning")
            .then(|| current.security_enabled("secret_scanning")),
        secret_scanning_push_protection: features
            .contains_key("secret_scanning_push_protection")
            .then(|| current.security_enabled("secret_scanning_push_protection")),
        dependabot_security_updates: features
            .contains_key("dependabot_security_updates")
            .then(|| current.security_enabled("dependabot_security_updates")),
        secret_scanning_validity_checks: features
            .contains_key("secret_scanning_validity_checks")
            .then(|| current.security_enabled("secret_scanning_validity_checks")),
    })
}

/// Serialize an enum to the SCREAMING_SNAKE_CASE form the API uses.
fn enum_value<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Parse an API enum value back into a configuration enum.
fn parse_enum<T: for<'de> Deserialize<'de>>(value: &str) -> Option<T> {
    serde_json::from_value(Value::String(value.to_string())).ok()
}

#[cfg(test)]
mod tests;
