//! The root configuration type.
//!
//! This is the public contract (ADR-007): the published JSON Schema is generated
//! from it, so field names, documentation and defaults here are user-visible.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::resources::autolinks::Autolink;
use crate::resources::labels::Label;
use crate::resources::repository::RepositorySettings;
use crate::resources::rulesets::Ruleset;

use super::prunable::Prunable;

/// The schema major version this file targets.
///
/// Optional in v1 for compatibility with existing `safe-settings` files, which
/// have no version field at all; required from v2.
pub const CURRENT_VERSION: u32 = 1;

/// A parsed `.github/settings.yml`.
///
/// Every section is optional, and an absent section means *unmanaged*: nothing is
/// read, diffed or written for it. This is what makes adopting the tool
/// incremental — you can start by managing labels alone and nothing else moves.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "gh-settings configuration",
    description = "Declarative GitHub repository settings, managed by the `gh settings` CLI extension."
)]
pub struct Settings {
    /// Schema major version this file targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,

    /// Repository metadata: description, homepage, features, merge and security
    /// settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositorySettings>,

    /// Repository topics.
    ///
    /// Also accepted under `repository.topics` for `safe-settings`
    /// compatibility; declaring both is an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Prunable<String>>,

    /// Issue and pull request labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Prunable<Label>>,

    /// Autolink references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autolinks: Option<Prunable<Autolink>>,

    /// Repository rulesets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rulesets: Option<Prunable<Ruleset>>,
}

impl Settings {
    /// Whether the file declares nothing at all.
    pub fn is_empty(&self) -> bool {
        self.repository.is_none()
            && self.topics.is_none()
            && self.labels.is_none()
            && self.autolinks.is_none()
            && self.rulesets.is_none()
    }

    /// Cross-section checks that no individual resource can perform.
    pub fn validate(&self, ctx: &crate::resources::ValidateCtx<'_>) -> Vec<super::Finding> {
        let mut findings = Vec::new();

        if let Some(version) = self.version
            && version != CURRENT_VERSION
        {
            findings.push(
                super::Finding::error(
                    "gh_settings::config::unsupported_version",
                    format!("unsupported schema version {version}"),
                )
                .at(ctx.span("version"))
                .labelled(format!("this build supports version {CURRENT_VERSION}"))
                .help("upgrade gh-settings, or set `version: 1`"),
            );
        }

        // `topics` and `repository.topics` are two spellings of one setting; if
        // both were honoured, which one wins would be arbitrary.
        if self.topics.is_some()
            && self
                .repository
                .as_ref()
                .is_some_and(|repository| repository.topics.is_some())
        {
            findings.push(
                super::Finding::error(
                    "gh_settings::config::conflicting_topics",
                    "topics are declared both at the top level and under `repository`",
                )
                .at(ctx.key_span("repository.topics"))
                .labelled("conflicts with the top-level `topics`")
                .help("keep the top-level `topics`; `repository.topics` exists only for safe-settings compatibility"),
            );
        }

        if self.version.is_none() && !self.is_empty() {
            findings.push(
                super::Finding::warning(
                    "gh_settings::config::missing_version",
                    "no `version` declared",
                )
                .help(format!(
                    "add `version: {CURRENT_VERSION}`; it will be required in a future release"
                )),
            );
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SpanIndex;
    use crate::resources::ValidateCtx;
    use pretty_assertions::assert_eq;

    fn findings(source: &str) -> Vec<String> {
        let settings: Settings = serde_yaml_ng::from_str(source).unwrap();
        let spans = SpanIndex::build(source);
        let ctx = ValidateCtx::new(&spans);
        settings
            .validate(&ctx)
            .into_iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn an_empty_document_declares_nothing() {
        let settings: Settings = serde_yaml_ng::from_str("{}").unwrap();
        assert!(settings.is_empty());
    }

    #[test]
    fn accepts_the_current_version() {
        assert!(
            !findings("version: 1\nlabels: []\n")
                .contains(&"gh_settings::config::unsupported_version".to_string())
        );
    }

    #[test]
    fn rejects_a_future_version() {
        assert!(
            findings("version: 99\nlabels: []\n")
                .contains(&"gh_settings::config::unsupported_version".to_string())
        );
    }

    #[test]
    fn warns_when_the_version_is_missing() {
        assert!(
            findings("labels: []\n").contains(&"gh_settings::config::missing_version".to_string())
        );
    }

    #[test]
    fn does_not_nag_about_the_version_of_an_empty_file() {
        assert!(findings("{}").is_empty());
    }

    #[test]
    fn rejects_topics_declared_twice() {
        let codes = findings("version: 1\ntopics: [a]\nrepository:\n  topics: [b]\n");
        assert!(codes.contains(&"gh_settings::config::conflicting_topics".to_string()));
    }

    #[test]
    fn accepts_safe_settings_style_topics_alone() {
        let codes = findings("version: 1\nrepository:\n  topics: [rust]\n");
        assert!(!codes.contains(&"gh_settings::config::conflicting_topics".to_string()));
    }

    #[test]
    fn round_trips_through_yaml() {
        let source = "version: 1\ntopics:\n  - rust\nlabels:\n  - name: bug\n    color: d73a4a\n";
        let settings: Settings = serde_yaml_ng::from_str(source).unwrap();
        let emitted = serde_yaml_ng::to_string(&settings).unwrap();
        let reparsed: Settings = serde_yaml_ng::from_str(&emitted).unwrap();
        assert_eq!(reparsed.topics.unwrap().items(), ["rust"]);
    }
}
