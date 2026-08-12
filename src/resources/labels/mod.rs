//! Label management.
//!
//! Labels are the only v1 resource reachable with the Actions `GITHUB_TOKEN`,
//! because they live under `Issues: write` rather than `Administration: write`.
//!
//! # Normalisation
//!
//! GitHub returns colours without the leading `#` and lowercased, and reports an
//! absent description as `""` rather than `null`. Comparing raw values would
//! therefore produce a permanent diff, so both sides are normalised first
//! (ADR-002).

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{Finding, Settings};
use crate::diff::diff_keyed;
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{Change, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx};

pub mod model;

pub use model::{Label, LabelState};

/// The `labels` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Labels;

/// Desired label configuration.
#[derive(Debug, Clone)]
pub struct Desired {
    /// Labels declared in the configuration, already normalised.
    pub labels: Vec<Label>,
    /// Whether labels absent from the configuration should be deleted.
    pub prune: bool,
}

/// Current label state on GitHub, normalised.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Labels that exist, keyed by their normalised name.
    pub labels: HashMap<String, Label>,
}

/// Payload carried by a label change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The label to write.
    pub label: Label,
    /// Existing name, when the change renames a label.
    ///
    /// Renames go through `PATCH .../labels/{old}` with a `new_name` field, so we
    /// have to remember where to send the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

#[async_trait]
impl Resource for Labels {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Labels
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ISSUES
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        let section = settings.labels.as_ref()?;
        Some(Desired {
            labels: section.items().iter().map(Label::normalized).collect(),
            prune: section.prune(),
        })
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(&desired.labels, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        // Decoded as `LabelState`, not `Label`: the API payload carries `id`,
        // `url` and `default`, which the strict configuration type rejects.
        let labels: Vec<LabelState> = client
            .send(Request::list(target.endpoint("labels")))
            .await?;

        Ok(Current {
            labels: labels
                .into_iter()
                .map(|label| (model::key(&label.name), label.as_label()))
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

        // A rename declares the *old* name, so those labels are keyed by the name
        // they currently have, not the one they will end up with.
        let desired_entries = desired
            .labels
            .iter()
            .map(|label| (model::key(label.lookup_name()), label.clone()));

        let diff = diff_keyed(
            desired_entries,
            current
                .labels
                .iter()
                .map(|(key, label)| (key.clone(), label.clone())),
        );

        let mut changes = Vec::new();

        for (_, label) in diff.created {
            if label.new_name.is_some() {
                // The label we were asked to rename does not exist. Creating it
                // under the new name is the intent-preserving outcome.
                let renamed = label.applied();
                changes.push(
                    Change::new(ResourceId::Labels, Op::Create, renamed.name.clone())
                        .summary(format!("create label {}", renamed.name))
                        .fields(renamed.as_fields())
                        .payload(Payload {
                            label: renamed,
                            from: None,
                        }),
                );
                continue;
            }
            changes.push(
                Change::new(ResourceId::Labels, Op::Create, label.name.clone())
                    .summary(format!("create label {}", label.name))
                    .fields(label.as_fields())
                    .payload(Payload { label, from: None }),
            );
        }

        for (_, desired_label, current_label) in diff.matched {
            let fields = desired_label.diff_against(&current_label);
            if fields.is_empty() {
                continue;
            }
            let target_label = desired_label.applied();
            let summary = match &desired_label.new_name {
                Some(new_name) => {
                    format!("rename label {} to {}", desired_label.name, new_name)
                }
                None => format!("update label {}", desired_label.name),
            };
            changes.push(
                Change::new(ResourceId::Labels, Op::Update, desired_label.name.clone())
                    .summary(summary)
                    .fields(fields)
                    .payload(Payload {
                        label: target_label,
                        from: Some(current_label.name.clone()),
                    }),
            );
        }

        if prune {
            for (_, label) in diff.deleted {
                changes.push(
                    Change::new(ResourceId::Labels, Op::Delete, label.name.clone())
                        .summary(format!("delete label {}", label.name))
                        .payload(Payload { label, from: None }),
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
            .unwrap_or_else(|error| panic!("label change carried an undecodable payload: {error}"));

        match change.op {
            Op::Create => {
                client
                    .execute(Request::post(
                        target.endpoint("labels"),
                        payload.label.as_create_body(),
                    ))
                    .await
            }
            Op::Update | Op::Recreate => {
                let existing = payload.from.as_deref().unwrap_or(&payload.label.name);
                client
                    .execute(Request::patch(
                        target.endpoint(&format!("labels/{}", urlencode(existing))),
                        payload.label.as_update_body(existing),
                    ))
                    .await
            }
            Op::Delete => {
                client
                    .execute(Request::delete(
                        target.endpoint(&format!("labels/{}", urlencode(&payload.label.name))),
                    ))
                    .await
            }
        }
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;
        if current.labels.is_empty() {
            return Ok(None);
        }

        let mut labels: Vec<&Label> = current.labels.values().collect();
        labels.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Some(Value::Array(
            labels
                .into_iter()
                .map(|label| {
                    let mut object = serde_json::Map::new();
                    object.insert("name".into(), json!(label.name));
                    object.insert("color".into(), json!(label.color));
                    if let Some(description) = &label.description {
                        object.insert("description".into(), json!(description));
                    }
                    Value::Object(object)
                })
                .collect(),
        )))
    }
}

/// Percent-encode a label name for use in a path segment.
///
/// Label names routinely contain spaces (`good first issue`) and `/`
/// (`area/docs`), both of which would otherwise corrupt the endpoint.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests;
