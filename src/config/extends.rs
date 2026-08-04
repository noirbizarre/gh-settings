//! Inheriting a configuration from another repository.
//!
//! `extends: acme/.github@v1` names a document in another repository, pinned to
//! a ref. The reference is parsed here rather than by serde, so a malformed one
//! is reported in this tool's vocabulary — with a span and a suggestion —
//! instead of serde's.

use std::fmt;

use async_trait::async_trait;

/// Where a base configuration lives.
///
/// The ref is required. An unpinned base is a moving target, and a plan saved
/// against one can be applied against a different document without anything
/// saying so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Owning organisation or user.
    pub owner: String,
    /// Repository name.
    pub repository: String,
    /// Path to the document within that repository.
    pub path: String,
    /// Branch, tag or commit to read it at.
    pub git_ref: String,
}

/// The path used when a reference does not name one.
pub const DEFAULT_PATH: &str = ".github/settings.yml";

/// Why a reference could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    /// No `@ref` was given.
    #[error("no ref: write `{0}@main`, or pin a tag such as `{0}@v1`")]
    MissingRef(String),
    /// The `owner/repo` part was not two segments.
    #[error("expected `owner/repo@ref`, optionally with a path")]
    NotARepository,
    /// Some component was empty.
    #[error("`{0}` is empty")]
    Empty(&'static str),
}

impl std::str::FromStr for Reference {
    type Err = ReferenceError;

    /// Parse `owner/repo[/path/to/file.yml]@ref`.
    ///
    /// Split on the *last* `@`, because a path may in principle contain one
    /// while a ref may not be followed by anything.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();

        let (location, git_ref) = value
            .rsplit_once('@')
            .ok_or_else(|| ReferenceError::MissingRef(value.to_string()))?;

        if git_ref.trim().is_empty() {
            return Err(ReferenceError::Empty("ref"));
        }

        let mut segments = location.splitn(3, '/');
        let owner = segments.next().unwrap_or_default().trim();
        let repository = segments.next().unwrap_or_default().trim();
        let path = segments.next().unwrap_or_default().trim();

        if owner.is_empty() {
            return Err(ReferenceError::Empty("owner"));
        }
        if repository.is_empty() {
            return Err(ReferenceError::NotARepository);
        }

        Ok(Self {
            owner: owner.to_string(),
            repository: repository.to_string(),
            path: if path.is_empty() {
                DEFAULT_PATH.to_string()
            } else {
                path.to_string()
            },
            git_ref: git_ref.trim().to_string(),
        })
    }
}

impl fmt::Display for Reference {
    /// Renders back to what the user wrote, and is what names the document in a
    /// diagnostic.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.repository)?;
        if self.path != DEFAULT_PATH {
            write!(formatter, "/{}", self.path)?;
        }
        write!(formatter, "@{}", self.git_ref)
    }
}

/// Reads a base configuration.
///
/// A port, so that `config` never learns GitHub exists — and so that the merge,
/// the provenance and the validation of an inheriting configuration are all
/// testable without a network or a subprocess.
#[async_trait]
pub trait BaseLoader: Send + Sync {
    /// Fetch the document a reference names, and the commit it was read at.
    ///
    /// The error is a rendered message rather than a typed transport error,
    /// because this module must not depend on the transport.
    async fn load(&self, reference: &Reference) -> Result<LoadedBase, String>;
}

/// A base document, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBase {
    /// The document text.
    pub text: String,
    /// The commit it was read at, when the loader could determine one.
    ///
    /// Recorded in a saved plan so that a base moving between planning and
    /// applying is reported as the base moving, rather than as the repository
    /// having drifted.
    pub commit: Option<String>,
}

/// Check the `extends` key of one document.
///
/// Per document, and at load time, rather than in `Settings::validate`: by the
/// time the merged settings are validated the key has been consumed, and a
/// finding about the *base* declaring `extends` has to carry the base's own
/// span so it renders against the base's text.
pub fn validate(
    settings: &crate::config::Settings,
    spans: &crate::config::SpanIndex,
    is_base: bool,
) -> Vec<crate::config::Finding> {
    use crate::config::Finding;

    let Some(value) = settings.extends.as_deref() else {
        return Vec::new();
    };

    if is_base {
        return vec![
            Finding::error(
                "gh_settings::config::nested_extends",
                "a base configuration may not itself extend another",
            )
            .at(spans.exact_key("extends"))
            .labelled("nested inheritance is not supported")
            .help(
                "flatten the chain: declare in this file everything the repositories that extend it need",
            ),
        ];
    }

    match value.parse::<Reference>() {
        Ok(_) => Vec::new(),
        Err(error) => vec![
            Finding::error(
                "gh_settings::config::invalid_extends",
                format!("`{value}` is not a valid reference"),
            )
            .at(spans.exact("extends"))
            .labelled(error.to_string())
            .help("write `owner/repo@ref`, for example `acme/.github@v1`"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(value: &str) -> Reference {
        value.parse().expect("a valid reference")
    }

    #[test]
    fn a_reference_defaults_to_the_conventional_path() {
        let reference = parse("acme/.github@v1");
        assert_eq!(reference.owner, "acme");
        assert_eq!(reference.repository, ".github");
        assert_eq!(reference.git_ref, "v1");
        assert_eq!(
            reference.path, DEFAULT_PATH,
            "so `acme/.github@v1` reads acme/.github/.github/settings.yml"
        );
    }

    #[test]
    fn a_path_may_be_given_explicitly() {
        let reference = parse("acme/shared/config/base.yml@main");
        assert_eq!(reference.repository, "shared");
        assert_eq!(reference.path, "config/base.yml");
        assert_eq!(reference.git_ref, "main");
    }

    #[test]
    fn the_ref_is_taken_from_the_last_at_sign() {
        let reference = parse("acme/shared/weird@name.yml@v2");
        assert_eq!(reference.path, "weird@name.yml");
        assert_eq!(reference.git_ref, "v2");
    }

    #[test]
    fn a_reference_renders_back_to_what_was_written() {
        for value in ["acme/.github@v1", "acme/shared/config/base.yml@main"] {
            assert_eq!(parse(value).to_string(), value);
        }
    }

    #[test]
    fn an_unpinned_reference_is_rejected() {
        // A moving base means a plan can be applied against a document nobody
        // reviewed, so the ref is required rather than defaulted.
        let error = "acme/.github".parse::<Reference>().unwrap_err();
        assert_eq!(error, ReferenceError::MissingRef("acme/.github".into()));
        assert!(error.to_string().contains("@main"), "{error}");
    }

    #[test]
    fn a_reference_without_a_repository_is_rejected() {
        assert_eq!(
            "acme@v1".parse::<Reference>().unwrap_err(),
            ReferenceError::NotARepository
        );
    }

    #[test]
    fn empty_components_are_rejected() {
        assert_eq!(
            "/repo@v1".parse::<Reference>().unwrap_err(),
            ReferenceError::Empty("owner")
        );
        assert_eq!(
            "acme/repo@".parse::<Reference>().unwrap_err(),
            ReferenceError::Empty("ref")
        );
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(parse("  acme/.github@v1  "), parse("acme/.github@v1"));
    }

    mod validation {
        use crate::config::{Settings, SourceId, SpanIndex};
        use pretty_assertions::assert_eq;

        fn check(source: &str, is_base: bool) -> Vec<String> {
            let mut settings: Settings = serde_norway::from_str(source).expect("valid yaml");
            settings.canonicalize();
            let spans = SpanIndex::build(SourceId::ROOT, source);
            super::super::validate(&settings, &spans, is_base)
                .into_iter()
                .map(|finding| finding.code)
                .collect()
        }

        #[test]
        fn a_well_formed_reference_is_accepted() {
            assert!(check("extends: acme/.github@v1\n", false).is_empty());
        }

        #[test]
        fn a_malformed_reference_is_rejected_with_our_own_diagnostic() {
            // Parsed here rather than by serde, so the user gets a span and an
            // example instead of serde's vocabulary.
            assert_eq!(
                check("extends: acme/.github\n", false),
                ["gh_settings::config::invalid_extends"]
            );
        }

        #[test]
        fn a_base_may_not_extend_another_base() {
            // Single level, by decision. The finding carries the base's own
            // span, so it renders against the base's text rather than the local
            // file's.
            assert_eq!(
                check("extends: other/base@v1\n", true),
                ["gh_settings::config::nested_extends"]
            );
        }

        #[test]
        fn a_document_without_the_key_is_never_faulted() {
            assert!(check("version: 1\n", false).is_empty());
            assert!(check("version: 1\n", true).is_empty());
        }

        #[test]
        fn a_file_that_only_inherits_is_not_empty() {
            // Otherwise it would skip the `missing_version` warning, on the
            // grounds of declaring nothing — while declaring an entire
            // configuration.
            let settings: Settings =
                serde_norway::from_str("extends: acme/.github@v1\n").expect("valid");
            assert!(!settings.is_empty());
        }
    }
}
