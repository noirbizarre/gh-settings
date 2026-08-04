//! The configuration file.
//!
//! The YAML file is the public contract of this project (ADR-007), so the types
//! in this module are deliberately conservative: unknown fields are rejected with
//! a suggestion rather than silently ignored, and every field carries the
//! documentation that ends up in the published JSON Schema.

pub mod diagnostics;
pub mod discover;
pub mod provenance;
pub mod prunable;
pub mod settings;
pub mod source;
pub mod spans;

pub use diagnostics::{Finding, Report, Severity, suggest};
pub use discover::{ConfigSource, discover};
pub use provenance::Provenance;
pub use prunable::Prunable;
pub use settings::Settings;
pub use source::{FileSpan, SourceFile, SourceId, Sources};
pub use spans::{Location, SpanIndex, normalize_path};

use miette::SourceSpan;

/// A configuration, parsed and indexed.
///
/// Plural in shape even though only one document contributes today: a
/// configuration that inherits from another repository is several documents
/// whose findings must each be rendered against their own text.
#[derive(Debug)]
pub struct Config {
    /// Where the root file came from, for diagnostics.
    pub path: std::path::PathBuf,
    /// Every document that contributed, root first.
    pub sources: Sources,
    /// Span index per document, in the same order as [`Config::sources`].
    pub spans: Vec<SpanIndex>,
    /// Where each logical configuration path physically lives.
    pub provenance: Provenance,
    /// The typed settings.
    pub settings: Settings,
}

impl Config {
    /// Text of the root document.
    pub fn source(&self) -> &str {
        &self.sources.root_file().text
    }

    /// Span index of the root document, which is always the first.
    pub fn root_spans(&self) -> &SpanIndex {
        self.spans
            .first()
            .expect("a parsed configuration always has a root document")
    }
}

/// Errors raised while loading a configuration file.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum ConfigError {
    /// No configuration file could be found.
    #[error("no configuration file found")]
    #[diagnostic(
        code(gh_settings::config::not_found),
        help(
            "create `.github/settings.yml`, or run `gh settings export` to generate one from the current repository"
        )
    )]
    NotFound,

    /// The file could not be read.
    #[error("could not read {path}")]
    #[diagnostic(code(gh_settings::config::unreadable))]
    Unreadable {
        /// Path we tried to read.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The file is not valid YAML, or does not match the schema.
    #[error("{message}")]
    #[diagnostic(code(gh_settings::config::parse))]
    Parse {
        /// What went wrong, in the user's terms.
        message: String,
        /// The file, so the excerpt can be rendered.
        #[source_code]
        source_code: miette::NamedSource<String>,
        /// The offending node.
        #[label("{label}")]
        span: SourceSpan,
        /// Text rendered under the underline.
        label: String,
        /// Actionable hint, typically a "did you mean".
        #[help]
        help: Option<String>,
    },
}

/// Parse a configuration document.
///
/// Deserialization goes through `serde_path_to_error` so a failure yields the
/// field path, which the span index turns into a precise underline (ADR-008).
pub fn parse(path: &std::path::Path, source: &str) -> Result<Config, ConfigError> {
    let (sources, root) = Sources::root(path.display().to_string(), source);
    let spans = SpanIndex::build(root, source);
    let deserializer = serde_norway::Deserializer::from_str(source);

    let settings: Settings = match serde_path_to_error::deserialize(deserializer) {
        Ok(settings) => settings,
        Err(error) => {
            let field_path = normalize_path(&error.path().to_string());
            let inner = error.into_inner();
            let message = inner.to_string();
            let is_unknown_field = message.contains("unknown field");

            // Prefer the span of the field serde blamed; fall back to the
            // location the YAML parser reported, then to the start of the file.
            // Unknown and missing fields are about the *key*, so underline that
            // rather than the value sitting next to it.
            let resolved = if is_unknown_field || message.contains("missing field") {
                spans.resolve_key(&field_path)
            } else {
                spans.resolve(&field_path)
            };

            let span = resolved
                .map(|resolved| resolved.span)
                .or_else(|| {
                    inner
                        .location()
                        .map(|location| SourceSpan::new(location.index().into(), 1usize))
                })
                .unwrap_or_else(|| SourceSpan::new(0.into(), 1usize));

            let help = unknown_field_help(&message);

            return Err(ConfigError::Parse {
                // serde appends its own location, which duplicates the underline,
                // and enumerates every valid field, which buries the actual
                // problem. The `help` carries the suggestion instead.
                message: condense(&message),
                source_code: miette::NamedSource::new(
                    path.display().to_string(),
                    source.to_string(),
                )
                .with_language("yaml"),
                span,
                label: label_for(&message),
                help,
            });
        }
    };

    let provenance = Provenance::for_document(root, &spans, &settings);

    Ok(Config {
        path: path.to_path_buf(),
        sources,
        spans: vec![spans],
        provenance,
        settings,
    })
}

/// Reduce serde's message to the part a human needs.
///
/// Drops the trailing location, which the underline already shows, and the
/// exhaustive `expected one of ...` list, which is unreadable for a struct with
/// twenty fields. The "did you mean" help says what to do instead.
fn condense(message: &str) -> String {
    let message = strip_location(message);
    match message.find(", expected one of ") {
        Some(index) => message[..index].to_string(),
        None => message,
    }
}

/// Strip the trailing `at line N column M` serde appends.
///
/// The underline already says where the problem is; repeating it in prose makes
/// the message longer without making it clearer.
fn strip_location(message: &str) -> String {
    match message.find(" at line ") {
        Some(index) => message[..index].to_string(),
        None => message.to_string(),
    }
}

/// A short label for the underline, derived from serde's message.
///
/// Uses `contains` rather than `starts_with` because serde prefixes nested
/// failures with the containing field, e.g. `repository: unknown field ...`.
fn label_for(message: &str) -> String {
    if message.contains("unknown field") {
        "unknown field".to_string()
    } else if message.contains("missing field") {
        "required field is missing".to_string()
    } else if message.contains("invalid type") {
        "wrong type".to_string()
    } else {
        "invalid".to_string()
    }
}

/// Turn serde's `expected one of` list into a "did you mean" hint.
fn unknown_field_help(message: &str) -> Option<String> {
    // Not `strip_prefix`: serde prefixes nested failures with the containing
    // field, as in `repository: unknown field \`descriptoin\``.
    let index = message.find("unknown field `")?;
    let rest = &message[index + "unknown field `".len()..];
    let (field, rest) = rest.split_once('`')?;
    let expected = rest.split_once("expected one of ")?.1;

    let candidates: Vec<String> = expected
        .split(", ")
        .map(|candidate| candidate.trim().trim_matches('`').to_string())
        .filter(|candidate| !candidate.is_empty())
        .collect();
    let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();

    match suggest(field, &candidate_refs) {
        Some(best) => Some(format!("did you mean `{best}`?")),
        None if !candidate_refs.is_empty() => {
            Some(format!("expected one of: {}", candidate_refs.join(", ")))
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn parse_str(source: &str) -> Result<Config, ConfigError> {
        parse(Path::new("settings.yml"), source)
    }

    #[test]
    fn parses_a_minimal_document() {
        let config = parse_str("repository:\n  description: hello\n").unwrap();
        assert_eq!(
            config.settings.repository.unwrap().description.unwrap(),
            Some("hello".to_string())
        );
    }

    #[test]
    fn an_empty_document_is_valid_and_manages_nothing() {
        let config = parse_str("").unwrap();
        assert!(config.settings.repository.is_none());
        assert!(config.settings.labels.is_none());
    }

    #[test]
    fn rejects_unknown_top_level_sections() {
        let error = parse_str("repositry:\n  description: x\n").unwrap_err();
        let ConfigError::Parse { message, help, .. } = error else {
            panic!("expected a parse error");
        };
        assert!(message.contains("unknown field"), "{message}");
        assert_eq!(help.as_deref(), Some("did you mean `repository`?"));
    }

    #[test]
    fn suggests_the_closest_field_within_a_section() {
        let error = parse_str("repository:\n  descriptoin: x\n").unwrap_err();
        let ConfigError::Parse { help, .. } = error else {
            panic!("expected a parse error");
        };
        assert_eq!(help.as_deref(), Some("did you mean `description`?"));
    }

    #[test]
    fn underlines_the_offending_node() {
        let source = "labels:\n  - name: bug\n    color: 12\n    nope: true\n";
        let error = parse_str(source).unwrap_err();
        let ConfigError::Parse { span, .. } = error else {
            panic!("expected a parse error");
        };
        let excerpt = &source[span.offset()..span.offset() + span.len()];
        assert!(
            excerpt.contains("nope") || excerpt.contains("true") || excerpt.contains("12"),
            "underlined {excerpt:?}"
        );
    }

    #[test]
    fn reports_malformed_yaml() {
        let error = parse_str("repository:\n\tdescription: tabs are illegal\n").unwrap_err();
        assert!(matches!(error, ConfigError::Parse { .. }));
        // A tab where a space belongs is a classic YAML trap; the message must
        // not be swallowed.
    }

    #[test]
    fn strips_the_duplicated_location_suffix() {
        assert_eq!(
            strip_location("unknown field `x` at line 2 column 3"),
            "unknown field `x`"
        );
        assert_eq!(strip_location("boom"), "boom");
    }

    #[test]
    fn condenses_the_exhaustive_field_list() {
        // A twenty-field struct produces an unreadable wall of text; the
        // suggestion in the help is what actually helps.
        assert_eq!(
            condense("unknown field `x`, expected one of `a`, `b`, `c` at line 2 column 3"),
            "unknown field `x`"
        );
    }

    #[test]
    fn labels_are_derived_from_the_failure_kind() {
        assert_eq!(label_for("unknown field `x`"), "unknown field");
        assert_eq!(
            label_for("missing field `name`"),
            "required field is missing"
        );
        assert_eq!(label_for("invalid type: string"), "wrong type");
    }
}
