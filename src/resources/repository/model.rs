//! Repository metadata model.
//!
//! # Absent is not null
//!
//! Three states must be distinguished for nullable fields:
//!
//! | YAML | Meaning |
//! |---|---|
//! | key omitted | unmanaged — never touched |
//! | `description: null` | managed — clear it |
//! | `description: "x"` | managed — set it |
//!
//! `Option<String>` cannot express that, so nullable fields use
//! `Option<Option<String>>` with an explicit deserializer. Getting this wrong
//! would mean a partial configuration file silently wipes the description of
//! every repository it is applied to.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::Finding;
use crate::resources::{FieldDiff, ValidateCtx};

/// Deserialize into a "double option", preserving the absent/null distinction.
///
/// Shared with any other resource that has a nullable field — environments'
/// `deployment_branch_policy` is the other one — because getting it wrong is
/// always the same bug: an absent field read as an explicit clear.
pub fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A nullable, optional field.
///
/// `None` = unmanaged, `Some(None)` = explicitly cleared, `Some(Some(v))` = set.
pub type Nullable<T> = Option<Option<T>>;

/// The `repository` configuration section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositorySettings {
    /// Short description shown under the repository name.
    ///
    /// Set to `null` to clear it.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Nullable<String>,

    /// Project website shown next to the description.
    ///
    /// Set to `null` to clear it.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub homepage: Nullable<String>,

    /// Topics.
    ///
    /// Accepted here only for `safe-settings` compatibility; prefer the
    /// top-level `topics` section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,

    /// Whether the repository is private.
    ///
    /// Changing this is possible but consequential — it can delete forks and
    /// break published links — so `plan` always flags it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,

    /// Whether the issue tracker is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_issues: Option<bool>,

    /// Whether the wiki is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_wiki: Option<bool>,

    /// Whether repository projects are enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_projects: Option<bool>,

    /// Whether the discussions tab is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_discussions: Option<bool>,

    /// Whether the repository is a template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,

    /// Whether merge commits are allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_merge_commit: Option<bool>,

    /// Whether squash merging is allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_squash_merge: Option<bool>,

    /// Whether rebase merging is allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_rebase_merge: Option<bool>,

    /// Whether auto-merge is available on pull requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_auto_merge: Option<bool>,

    /// Whether updating a pull request branch is allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_update_branch: Option<bool>,

    /// Whether head branches are deleted automatically after merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_branch_on_merge: Option<bool>,

    /// Default commit title for squash merges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squash_merge_commit_title: Option<SquashCommitTitle>,

    /// Default commit message for squash merges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squash_merge_commit_message: Option<SquashCommitMessage>,

    /// Default commit title for merge commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit_title: Option<MergeCommitTitle>,

    /// Default commit message for merge commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit_message: Option<MergeCommitMessage>,

    /// The default branch.
    ///
    /// Renames the branch; it must already exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,

    /// Whether anonymous Git read access is enabled (GitHub Enterprise only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_access_enabled: Option<bool>,

    /// Whether the repository is archived.
    ///
    /// Archiving is effectively one-way through the API: GitHub allows setting
    /// this to `true`, but unarchiving must be done in the web UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,

    /// Security and analysis features.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<SecuritySettings>,
}

/// Squash merge commit title source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SquashCommitTitle {
    /// The pull request title.
    PrTitle,
    /// The commit title, when the pull request has a single commit.
    CommitOrPrTitle,
}

/// Squash merge commit message source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SquashCommitMessage {
    /// The pull request body.
    PrBody,
    /// The commit messages of the branch.
    CommitMessages,
    /// No message.
    Blank,
}

/// Merge commit title source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeCommitTitle {
    /// The pull request title.
    PrTitle,
    /// The default `Merge pull request #N` title.
    MergeMessage,
}

/// Merge commit message source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeCommitMessage {
    /// The pull request body.
    PrBody,
    /// The pull request title.
    PrTitle,
    /// No message.
    Blank,
}

/// Security and analysis features.
///
/// The API models these as `{ status: "enabled" | "disabled" }` objects rather
/// than booleans; the configuration exposes plain booleans and the resource
/// translates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecuritySettings {
    /// Dependency graph advanced security (private repositories).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced_security: Option<bool>,

    /// Secret scanning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_scanning: Option<bool>,

    /// Secret scanning push protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_scanning_push_protection: Option<bool>,

    /// Automatic Dependabot security fixes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependabot_security_updates: Option<bool>,

    /// Secret scanning validity checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_scanning_validity_checks: Option<bool>,
}

impl SecuritySettings {
    /// Every declared feature, as `(api_key, enabled)` pairs.
    pub fn declared(&self) -> Vec<(&'static str, bool)> {
        [
            ("advanced_security", self.advanced_security),
            ("secret_scanning", self.secret_scanning),
            (
                "secret_scanning_push_protection",
                self.secret_scanning_push_protection,
            ),
            (
                "dependabot_security_updates",
                self.dependabot_security_updates,
            ),
            (
                "secret_scanning_validity_checks",
                self.secret_scanning_validity_checks,
            ),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect()
    }
}

/// Normalise a homepage URL for comparison.
///
/// GitHub stores what it is given but reports an unset homepage as `""`, so an
/// omitted and an empty homepage must compare equal.
pub fn normalize_homepage(homepage: &str) -> Option<String> {
    let trimmed = homepage.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Normalise a description for comparison.
pub fn normalize_description(description: &str) -> Option<String> {
    let trimmed = description.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Render a nullable value for plan output.
pub fn render(value: Option<&String>) -> String {
    match value {
        Some(value) => format!("{value:?}"),
        None => "(none)".to_string(),
    }
}

/// Diff a nullable string field.
///
/// Returns `None` when the field is unmanaged, which is the whole point of the
/// double option.
pub fn nullable_field(
    name: &str,
    desired: &Nullable<String>,
    current: Option<&String>,
    normalize: fn(&str) -> Option<String>,
) -> Option<FieldDiff> {
    let desired = desired.as_ref()?;
    let desired = desired.as_deref().and_then(normalize);
    let current = current.map(String::as_str).and_then(normalize);

    if desired == current {
        return None;
    }

    Some(FieldDiff {
        field: name.to_string(),
        before: Some(render(current.as_ref())),
        after: Some(render(desired.as_ref())),
    })
}

/// Validate the repository section.
pub fn validate(settings: &RepositorySettings, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    // GitHub caps descriptions at 350 characters and rejects longer ones with a
    // 422 that does not say which field was at fault.
    const MAX_DESCRIPTION: usize = 350;
    if let Some(Some(description)) = &settings.description
        && description.chars().count() > MAX_DESCRIPTION
    {
        findings.push(
            Finding::error(
                "gh_settings::repository::description_too_long",
                format!(
                    "description is {} characters, the maximum is {MAX_DESCRIPTION}",
                    description.chars().count()
                ),
            )
            .at(ctx.span("repository.description"))
            .labelled("too long"),
        );
    }

    if let Some(Some(homepage)) = &settings.homepage
        && !homepage.trim().is_empty()
        && !homepage.contains("://")
    {
        findings.push(
            Finding::warning(
                "gh_settings::repository::homepage_scheme",
                "homepage has no URL scheme",
            )
            .at(ctx.span("repository.homepage"))
            .labelled("missing `https://`")
            .help("GitHub rewrites scheme-less homepages, which shows up as a permanent diff"),
        );
    }

    // At least one merge strategy must remain enabled; GitHub rejects the
    // all-disabled state with an opaque error.
    let strategies = [
        settings.allow_merge_commit,
        settings.allow_squash_merge,
        settings.allow_rebase_merge,
    ];
    if strategies.iter().all(|strategy| *strategy == Some(false)) {
        findings.push(
            Finding::error(
                "gh_settings::repository::no_merge_strategy",
                "all merge strategies are disabled",
            )
            .at(ctx.key_span("repository.allow_merge_commit"))
            .labelled("at least one must stay enabled")
            .help(
                "enable one of `allow_merge_commit`, `allow_squash_merge` or `allow_rebase_merge`",
            ),
        );
    }

    if settings.archived == Some(false) {
        findings.push(
            Finding::warning(
                "gh_settings::repository::unarchive",
                "`archived: false` cannot unarchive a repository",
            )
            .at(ctx.span("repository.archived"))
            .help("unarchiving is only possible from the GitHub web interface"),
        );
    }

    if let Some(Some(description)) = &settings.description
        && description.contains('\n')
    {
        findings.push(
            Finding::error(
                "gh_settings::repository::multiline_description",
                "description cannot span multiple lines",
            )
            .at(ctx.span("repository.description"))
            .labelled("contains a newline"),
        );
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> RepositorySettings {
        serde_norway::from_str(source).unwrap()
    }

    #[test]
    fn an_omitted_field_is_unmanaged() {
        assert_eq!(parse("homepage: x").description, None);
    }

    #[test]
    fn an_explicit_null_means_clear_it() {
        // The distinction that keeps partial files from wiping metadata.
        assert_eq!(parse("description: null").description, Some(None));
    }

    #[test]
    fn a_value_means_set_it() {
        assert_eq!(
            parse("description: hello").description,
            Some(Some("hello".to_string()))
        );
    }

    #[test]
    fn an_unmanaged_field_never_diffs() {
        let current = "existing".to_string();
        assert_eq!(
            nullable_field("description", &None, Some(&current), normalize_description),
            None
        );
    }

    #[test]
    fn clearing_a_set_field_diffs() {
        let current = "existing".to_string();
        let diff = nullable_field(
            "description",
            &Some(None),
            Some(&current),
            normalize_description,
        )
        .unwrap();
        assert_eq!(diff.before.as_deref(), Some("\"existing\""));
        assert_eq!(diff.after.as_deref(), Some("(none)"));
    }

    #[test]
    fn clearing_an_already_empty_field_does_not_diff() {
        // GitHub reports an unset description as "", not null; without
        // normalisation this would be a permanent diff.
        let current = String::new();
        assert_eq!(
            nullable_field(
                "description",
                &Some(None),
                Some(&current),
                normalize_description
            ),
            None
        );
    }

    #[test]
    fn whitespace_only_differences_do_not_diff() {
        let current = "hello".to_string();
        assert_eq!(
            nullable_field(
                "description",
                &Some(Some("  hello  ".into())),
                Some(&current),
                normalize_description
            ),
            None
        );
    }

    #[test]
    fn security_settings_list_only_declared_features() {
        let security = SecuritySettings {
            secret_scanning: Some(true),
            advanced_security: Some(false),
            ..Default::default()
        };
        assert_eq!(
            security.declared(),
            vec![("advanced_security", false), ("secret_scanning", true)]
        );
    }

    mod validation {
        use super::*;
        use crate::config::SpanIndex;

        fn codes(source: &str) -> Vec<String> {
            let settings = parse(source);
            let spans = SpanIndex::default();
            let ctx = ValidateCtx::new(&spans);
            validate(&settings, &ctx)
                .into_iter()
                .map(|f| f.code)
                .collect()
        }

        #[test]
        fn accepts_a_reasonable_section() {
            assert!(codes("description: hello\nhomepage: https://example.com").is_empty());
        }

        #[test]
        fn rejects_disabling_every_merge_strategy() {
            let codes = codes(
                "allow_merge_commit: false\nallow_squash_merge: false\nallow_rebase_merge: false",
            );
            assert!(codes.contains(&"gh_settings::repository::no_merge_strategy".to_string()));
        }

        #[test]
        fn allows_disabling_all_but_one() {
            let codes = codes(
                "allow_merge_commit: false\nallow_squash_merge: true\nallow_rebase_merge: false",
            );
            assert!(!codes.contains(&"gh_settings::repository::no_merge_strategy".to_string()));
        }

        #[test]
        fn does_not_complain_when_no_strategy_is_managed() {
            assert!(codes("description: hello").is_empty());
        }

        #[test]
        fn warns_about_a_scheme_less_homepage() {
            assert!(
                codes("homepage: example.com")
                    .contains(&"gh_settings::repository::homepage_scheme".to_string())
            );
        }

        #[test]
        fn rejects_an_overlong_description() {
            let source = format!("description: {}", "x".repeat(351));
            assert!(
                codes(&source)
                    .contains(&"gh_settings::repository::description_too_long".to_string())
            );
        }

        #[test]
        fn rejects_a_multiline_description() {
            assert!(
                codes("description: \"a\\nb\"")
                    .contains(&"gh_settings::repository::multiline_description".to_string())
            );
        }

        #[test]
        fn warns_that_unarchiving_is_not_possible() {
            assert!(
                codes("archived: false")
                    .contains(&"gh_settings::repository::unarchive".to_string())
            );
        }
    }
}
