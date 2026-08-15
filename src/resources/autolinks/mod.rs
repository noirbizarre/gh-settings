//! Autolink references.
//!
//! # There is no update endpoint
//!
//! GitHub exposes only `GET`, `POST` and `DELETE` for autolinks. Changing an
//! autolink's `url_template` or `is_alphanumeric` flag therefore requires
//! deleting and recreating it.
//!
//! We model that honestly as [`Op::Recreate`] rather than pretending an update
//! happened: the operation is destructive (there is a window in which the
//! autolink does not exist) and the plan says so.
//!
//! # Normalisation
//!
//! `is_alphanumeric` defaults to `true` server-side, so an omitted flag and an
//! explicit `true` must compare equal.

use std::collections::HashMap;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{Finding, Settings};
use crate::diff::diff_keyed;
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

/// The `autolinks` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Autolinks;

/// A single autolink reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Autolink {
    /// The prefix that triggers the link, for example `OPS-`.
    ///
    /// This is the autolink's identity: changing it creates a new autolink rather
    /// than modifying the existing one.
    pub key_prefix: String,

    /// Target URL, containing the `<num>` placeholder.
    ///
    /// For example `https://jira.company.com/browse/<num>`.
    pub url_template: String,

    /// Whether the reference is alphanumeric rather than purely numeric.
    ///
    /// Defaults to `true`, matching GitHub. Set to `false` for systems whose
    /// identifiers are always numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_alphanumeric: Option<bool>,
}

impl Autolink {
    /// Build an autolink.
    pub fn new(key_prefix: impl Into<String>, url_template: impl Into<String>) -> Self {
        Self {
            key_prefix: key_prefix.into(),
            url_template: url_template.into(),
            is_alphanumeric: None,
        }
    }

    /// Set the alphanumeric flag.
    pub fn alphanumeric(mut self, is_alphanumeric: bool) -> Self {
        self.is_alphanumeric = Some(is_alphanumeric);
        self
    }

    /// The effective alphanumeric flag, applying GitHub's default.
    pub fn is_alphanumeric(&self) -> bool {
        self.is_alphanumeric.unwrap_or(true)
    }

    /// A normalised copy, comparable against a normalised counterpart.
    pub fn normalized(&self) -> Self {
        Self {
            key_prefix: self.key_prefix.trim().to_string(),
            url_template: self.url_template.trim().to_string(),
            // Resolve the default so an omitted flag and an explicit `true`
            // compare equal instead of diffing forever.
            is_alphanumeric: Some(self.is_alphanumeric()),
        }
    }

    /// Request body for creating this autolink.
    pub fn as_body(&self) -> Value {
        json!({
            "key_prefix": self.key_prefix,
            "url_template": self.url_template,
            "is_alphanumeric": self.is_alphanumeric(),
        })
    }
}

/// The state of an autolink as GitHub reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct AutolinkState {
    /// Server-assigned identifier, needed for deletion.
    pub id: u64,
    /// Trigger prefix.
    pub key_prefix: String,
    /// Target URL template.
    pub url_template: String,
    /// Whether the reference is alphanumeric.
    #[serde(default = "default_true")]
    pub is_alphanumeric: bool,
}

fn default_true() -> bool {
    true
}

impl AutolinkState {
    /// The comparable form of this autolink.
    pub fn as_autolink(&self) -> Autolink {
        Autolink {
            key_prefix: self.key_prefix.trim().to_string(),
            url_template: self.url_template.trim().to_string(),
            is_alphanumeric: Some(self.is_alphanumeric),
        }
    }
}

/// Desired autolink configuration.
#[derive(Debug, Clone)]
pub struct Desired {
    /// Declared autolinks, normalised.
    pub autolinks: Vec<Autolink>,
    /// Whether unmanaged autolinks should be deleted.
    pub prune: bool,
}

/// Current autolinks, keyed by prefix.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Existing autolinks.
    pub autolinks: HashMap<String, AutolinkState>,
}

/// Payload of an autolink change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The autolink to create, when the operation creates one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autolink: Option<Autolink>,
    /// The identifier to delete, when the operation removes one.
    ///
    /// Both are set for a recreate: delete by id, then create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_id: Option<u64>,
}

#[async_trait]
impl Resource for Autolinks {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Autolinks
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ADMINISTRATION
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        let section = settings.autolinks.as_ref()?;
        Some(Desired {
            autolinks: section.items().iter().map(Autolink::normalized).collect(),
            prune: section.prune(),
        })
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        validate(&desired.autolinks, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let autolinks: Vec<AutolinkState> = client
            .send(Request::list(target.endpoint("autolinks")))
            .await?;

        Ok(Current {
            autolinks: autolinks
                .into_iter()
                .map(|autolink| (autolink.key_prefix.trim().to_string(), autolink))
                .collect(),
        })
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
                .autolinks
                .iter()
                .map(|autolink| (autolink.key_prefix.clone(), autolink.clone())),
            current
                .autolinks
                .iter()
                .map(|(prefix, state)| (prefix.clone(), state.clone())),
        );

        let mut changes = Vec::new();

        for (prefix, autolink) in diff.created {
            changes.push(
                Change::new(ResourceId::Autolinks, Op::Create, &prefix)
                    .summary(format!("create autolink {prefix}"))
                    .fields(vec![
                        FieldDiff::added("url_template", &autolink.url_template),
                        FieldDiff::added("is_alphanumeric", autolink.is_alphanumeric().to_string()),
                    ])
                    .payload(Payload {
                        autolink: Some(autolink),
                        delete_id: None,
                    }),
            );
        }

        for (prefix, desired_link, state) in diff.matched {
            let existing = state.as_autolink();
            if existing == desired_link {
                continue;
            }

            let mut fields = Vec::new();
            if existing.url_template != desired_link.url_template {
                fields.push(FieldDiff::changed(
                    "url_template",
                    &existing.url_template,
                    &desired_link.url_template,
                ));
            }
            if existing.is_alphanumeric() != desired_link.is_alphanumeric() {
                fields.push(FieldDiff::changed(
                    "is_alphanumeric",
                    existing.is_alphanumeric().to_string(),
                    desired_link.is_alphanumeric().to_string(),
                ));
            }

            // Recreate, not update: the API has no update endpoint for autolinks.
            changes.push(
                Change::new(ResourceId::Autolinks, Op::Recreate, &prefix)
                    .summary(format!("recreate autolink {prefix} (no update endpoint)"))
                    .fields(fields)
                    .payload(Payload {
                        autolink: Some(desired_link),
                        delete_id: Some(state.id),
                    }),
            );
        }

        if prune {
            for (prefix, state) in diff.deleted {
                changes.push(
                    Change::new(ResourceId::Autolinks, Op::Delete, &prefix)
                        .summary(format!("delete autolink {prefix}"))
                        .payload(Payload {
                            autolink: None,
                            delete_id: Some(state.id),
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
            panic!("autolink change carried an undecodable payload: {error}")
        });

        // Delete first: GitHub rejects a second autolink with the same prefix, so
        // a recreate that created first would always fail with a 422.
        if let Some(id) = payload.delete_id {
            client
                .execute(Request::delete(target.endpoint(&format!("autolinks/{id}"))))
                .await?;
        }

        if let Some(autolink) = payload.autolink {
            client
                .execute(Request::post(
                    target.endpoint("autolinks"),
                    autolink.as_body(),
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
        if current.autolinks.is_empty() {
            return Ok(None);
        }

        let mut autolinks: Vec<Autolink> = current
            .autolinks
            .values()
            .map(AutolinkState::as_autolink)
            .collect();
        autolinks.sort_by(|a, b| a.key_prefix.cmp(&b.key_prefix));

        Ok(Some(serde_json::to_value(autolinks).unwrap_or(Value::Null)))
    }
}

/// Validate the desired autolinks.
pub fn validate(autolinks: &[Autolink], ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();

    for (position, autolink) in autolinks.iter().enumerate() {
        let path = format!("autolinks.{position}");

        if autolink.key_prefix.is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::autolinks::empty_prefix",
                    "`key_prefix` cannot be empty",
                )
                .at(ctx.span(&format!("{path}.key_prefix"))),
            );
        }

        if let Some(previous) = seen.insert(&autolink.key_prefix, position) {
            findings.push(
                Finding::error(
                    "gh_settings::autolinks::duplicate",
                    format!(
                        "autolink prefix `{}` is declared more than once",
                        autolink.key_prefix
                    ),
                )
                .at(ctx.span(&format!("{path}.key_prefix")))
                .labelled(format!("already declared at autolinks.{previous}")),
            );
        }

        if !autolink.url_template.contains("<num>") {
            findings.push(
                Finding::error(
                    "gh_settings::autolinks::missing_placeholder",
                    "`url_template` must contain the `<num>` placeholder",
                )
                .at(ctx.span(&format!("{path}.url_template")))
                .labelled("no `<num>` found")
                .help("for example: https://jira.company.com/browse/<num>"),
            );
        }

        if !autolink.url_template.contains("://") {
            findings.push(
                Finding::error(
                    "gh_settings::autolinks::relative_url",
                    "`url_template` must be an absolute URL",
                )
                .at(ctx.span(&format!("{path}.url_template")))
                .labelled("missing scheme")
                .help("prefix it with `https://`"),
            );
        }

        // A prefix that is a prefix of another is ambiguous for GitHub's matcher.
        for (other_position, other) in autolinks.iter().enumerate() {
            if other_position == position || other.key_prefix.is_empty() {
                continue;
            }
            if autolink.key_prefix.starts_with(&other.key_prefix)
                && autolink.key_prefix != other.key_prefix
            {
                findings.push(
                    Finding::warning(
                        "gh_settings::autolinks::ambiguous_prefix",
                        format!(
                            "`{}` starts with `{}`, which GitHub may match first",
                            autolink.key_prefix, other.key_prefix
                        ),
                    )
                    .at(ctx.span(&format!("{path}.key_prefix")))
                    .help("use prefixes that cannot shadow one another"),
                );
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests;
