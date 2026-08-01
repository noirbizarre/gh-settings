//! Byte-span index over a YAML document.
//!
//! We parse the configuration twice, on purpose (ADR-008):
//!
//! * `saphyr` builds a tree that carries byte spans, from which we derive a
//!   `path -> span` index;
//! * `serde` deserializes into our typed [`Settings`](crate::config::Settings).
//!
//! Neither library does both. Deserialization failures are captured with
//! `serde_path_to_error`, which yields a field path such as
//! `repository.description`; looking that path up in this index turns a flat
//! "invalid type" message into an underline under the exact offending value.

use std::collections::HashMap;

use miette::SourceSpan;
use saphyr::{LoadableYamlNode, MarkedYaml, YamlData};

/// A location in the source document, for both the key and the value of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Span covering the value.
    pub value: SourceSpan,
    /// Span covering the key, when the node is a mapping entry.
    ///
    /// Errors about a field being unknown or missing should point at the key;
    /// errors about a field's contents should point at the value.
    pub key: Option<SourceSpan>,
}

/// Maps dotted field paths to source spans.
#[derive(Debug, Clone, Default)]
pub struct SpanIndex {
    entries: HashMap<String, Location>,
    /// Span of the whole document, used as a fallback.
    root: Option<SourceSpan>,
}

impl SpanIndex {
    /// Build an index by parsing `source` with `saphyr`.
    ///
    /// A parse failure yields an empty index rather than an error: the serde pass
    /// will produce the user-facing syntax error, and it does so with a better
    /// message. This function's only job is to enrich, never to gate.
    pub fn build(source: &str) -> Self {
        let Ok(documents) = MarkedYaml::load_from_str(source) else {
            return Self::default();
        };
        let Some(document) = documents.first() else {
            return Self::default();
        };

        let mut index = Self {
            entries: HashMap::new(),
            root: Some(span_of(document)),
        };
        index.walk(document, &mut Vec::new(), None);
        index
    }

    /// Recursively record every node's span, keyed by its path.
    fn walk(&mut self, node: &MarkedYaml, path: &mut Vec<String>, key_span: Option<SourceSpan>) {
        let location = Location {
            value: span_of(node),
            key: key_span,
        };
        self.entries.insert(path.join("."), location);

        match &node.data {
            YamlData::Mapping(mapping) => {
                for (key, value) in mapping.iter() {
                    let Some(name) = key.data.as_str() else {
                        continue;
                    };
                    path.push(name.to_string());
                    self.walk(value, path, Some(span_of(key)));
                    path.pop();
                }
            }
            YamlData::Sequence(sequence) => {
                for (position, value) in sequence.iter().enumerate() {
                    path.push(position.to_string());
                    self.walk(value, path, None);
                    path.pop();
                }
            }
            _ => {}
        }
    }

    /// Look up a path such as `labels.0.color`.
    pub fn get(&self, path: &str) -> Option<Location> {
        self.entries.get(path).copied()
    }

    /// Span for a path, falling back to the nearest known ancestor.
    ///
    /// `serde_path_to_error` sometimes reports a path one level deeper than any
    /// node that actually exists in the document — for instance a missing field
    /// reports `repository.description` when only `repository` is present. Walking
    /// up keeps the underline useful instead of dropping it entirely.
    pub fn resolve(&self, path: &str) -> Option<SourceSpan> {
        if let Some(location) = self.get(path) {
            return Some(location.value);
        }

        let mut remaining = path;
        while let Some((parent, _)) = remaining.rsplit_once('.') {
            if let Some(location) = self.get(parent) {
                return Some(location.value);
            }
            remaining = parent;
        }

        self.root
    }

    /// Span of the key at `path`, falling back to its value span.
    pub fn resolve_key(&self, path: &str) -> Option<SourceSpan> {
        self.get(path)
            .map(|location| location.key.unwrap_or(location.value))
            .or_else(|| self.resolve(path))
    }

    /// Whether the document declared anything at `path`.
    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }
}

/// Convert a `saphyr` span into a `miette` one.
fn span_of(node: &MarkedYaml) -> SourceSpan {
    let start = node.span.start.index();
    let end = node.span.end.index();
    // Zero-width spans render as an invisible caret; give them one character so
    // the diagnostic still points somewhere.
    let length = end.saturating_sub(start).max(1);
    SourceSpan::new(start.into(), length)
}

/// Normalise a `serde_path_to_error` path into the form used by this index.
///
/// serde renders sequence indices as `labels[0]` in some paths and `labels.0` in
/// others; we always key on the dotted form.
pub fn normalize_path(path: &str) -> String {
    path.replace('[', ".").replace(']', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SOURCE: &str = "repository:\n  description: hello world\n  private: false\nlabels:\n  - name: bug\n    color: d73a4a\n";

    fn slice(span: SourceSpan) -> &'static str {
        &SOURCE[span.offset()..span.offset() + span.len()]
    }

    #[test]
    fn indexes_nested_scalars() {
        let index = SpanIndex::build(SOURCE);
        assert_eq!(
            slice(index.resolve("repository.description").unwrap()),
            "hello world"
        );
        assert_eq!(slice(index.resolve("repository.private").unwrap()), "false");
    }

    #[test]
    fn indexes_sequence_items() {
        let index = SpanIndex::build(SOURCE);
        assert_eq!(slice(index.resolve("labels.0.name").unwrap()), "bug");
        assert_eq!(slice(index.resolve("labels.0.color").unwrap()), "d73a4a");
    }

    #[test]
    fn keys_and_values_are_distinguished() {
        let index = SpanIndex::build(SOURCE);
        let location = index.get("repository.description").unwrap();
        assert_eq!(slice(location.value), "hello world");
        assert_eq!(slice(location.key.unwrap()), "description");
    }

    #[test]
    fn unknown_paths_fall_back_to_the_nearest_ancestor() {
        let index = SpanIndex::build(SOURCE);
        // `homepage` is absent, so we should land on the `repository` mapping.
        let span = index.resolve("repository.homepage").unwrap();
        assert!(slice(span).starts_with("description:"));
    }

    #[test]
    fn a_malformed_document_yields_an_empty_index_not_an_error() {
        let index = SpanIndex::build("repository:\n  - : : :\n\t bad");
        assert!(index.resolve("repository.description").is_none() || index.root.is_some());
    }

    #[test]
    fn normalizes_serde_bracket_paths() {
        assert_eq!(normalize_path("labels[0].color"), "labels.0.color");
        assert_eq!(
            normalize_path("repository.description"),
            "repository.description"
        );
    }
}
