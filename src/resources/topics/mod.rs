//! Repository topics.
//!
//! # Normalisation
//!
//! GitHub normalises topics on write: they are lowercased, spaces and underscores
//! become hyphens, and invalid characters are rejected. Comparing what the user
//! wrote against what GitHub stored would therefore produce a permanent diff, so
//! both sides go through [`normalize`] first (ADR-002).
//!
//! Topics are also replace-only: there is no per-topic endpoint, just
//! `PUT /repos/{owner}/{repo}/topics` with the complete list. The diff is
//! nonetheless computed per topic so the plan reads as `+ rust` / `- archived`
//! rather than an opaque "replace all topics".

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{Finding, Settings};
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{Change, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx};

/// The `topics` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Topics;

/// Desired topic configuration.
#[derive(Debug, Clone)]
pub struct Desired {
    /// Normalised topics.
    pub topics: BTreeSet<String>,
    /// Whether topics absent from the configuration should be removed.
    pub prune: bool,
}

/// Current topics, normalised.
#[derive(Debug, Clone, Default)]
pub struct Current {
    /// Topics currently set on the repository.
    pub topics: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct TopicsResponse {
    #[serde(default)]
    names: Vec<String>,
}

/// Payload of a topics change: always the full resulting list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The complete set of topics to write.
    pub names: Vec<String>,
}

/// GitHub's limit on the number of topics per repository.
const MAX_TOPICS: usize = 20;
/// GitHub's limit on the length of a single topic.
const MAX_LENGTH: usize = 50;

/// Normalise a topic the way GitHub does.
pub fn normalize(topic: &str) -> String {
    topic
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| match character {
            ' ' | '_' => '-',
            other => other,
        })
        .collect()
}

/// Whether a normalised topic is acceptable to GitHub.
///
/// Topics may contain lowercase letters, digits and hyphens, must start with a
/// letter or digit, and are at most 50 characters.
pub fn is_valid(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= MAX_LENGTH
        && topic.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && topic
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
}

#[async_trait]
impl Resource for Topics {
    type Desired = Desired;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Topics
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ADMINISTRATION
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        // Both spellings were folded into `topics` when the document was
        // parsed, so there is only one to read here.
        let section = settings.topics.as_ref()?;

        Some(Desired {
            topics: section
                .items()
                .iter()
                .map(|topic| normalize(topic))
                .collect(),
            prune: section.prune(),
        })
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();

        if desired.topics.len() > MAX_TOPICS {
            findings.push(
                Finding::error(
                    "gh_settings::topics::too_many",
                    format!(
                        "{} topics declared, GitHub allows at most {MAX_TOPICS}",
                        desired.topics.len()
                    ),
                )
                .at(ctx.key_span("topics")),
            );
        }

        for topic in &desired.topics {
            if !is_valid(topic) {
                findings.push(
                    Finding::error(
                        "gh_settings::topics::invalid",
                        format!("`{topic}` is not a valid topic"),
                    )
                    .at(ctx.key_span("topics"))
                    .labelled("invalid topic")
                    .help(
                        "topics may contain lowercase letters, digits and hyphens, must start with a letter or digit, and are at most 50 characters",
                    ),
                );
            }
        }

        findings
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let response: TopicsResponse = client
            .send(
                Request::get(target.endpoint("topics"))
                    .header("Accept", "application/vnd.github.mercy-preview+json"),
            )
            .await?;

        Ok(Current {
            topics: response
                .names
                .iter()
                .map(|topic| normalize(topic))
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

        let added: Vec<&String> = desired.topics.difference(&current.topics).collect();
        let removed: Vec<&String> = current.topics.difference(&desired.topics).collect();

        if added.is_empty() && (!prune || removed.is_empty()) {
            return Vec::new();
        }

        // The endpoint replaces the whole list, so compute the final state once
        // and attach it to every change. Applying any one of them converges.
        let final_topics: BTreeSet<String> = if prune {
            desired.topics.clone()
        } else {
            desired.topics.union(&current.topics).cloned().collect()
        };
        let payload = Payload {
            names: final_topics.into_iter().collect(),
        };

        let mut changes = Vec::new();

        for topic in added {
            changes.push(
                Change::new(ResourceId::Topics, Op::Create, topic)
                    .summary(format!("add topic {topic}"))
                    .payload(&payload),
            );
        }

        if prune {
            for topic in removed {
                changes.push(
                    Change::new(ResourceId::Topics, Op::Delete, topic)
                        .summary(format!("remove topic {topic}"))
                        .payload(&payload),
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
            .unwrap_or_else(|error| panic!("topics change carried an undecodable payload: {error}"));

        client
            .execute(Request::put(
                target.endpoint("topics"),
                json!({ "names": payload.names }),
            ))
            .await
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        let current = self.current(client, target).await?;
        if current.topics.is_empty() {
            return Ok(None);
        }
        Ok(Some(json!(current.topics.into_iter().collect::<Vec<_>>())))
    }
}

#[cfg(test)]
mod tests;
