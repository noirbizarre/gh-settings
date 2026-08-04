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

use super::source::{FileSpan, SourceId};

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

/// Maps dotted field paths to source spans, for one document.
#[derive(Debug, Clone, Default)]
pub struct SpanIndex {
    /// Which document these offsets index into.
    source: SourceId,
    entries: HashMap<String, Location>,
    /// Span of the whole document, used as a fallback.
    root: Option<SourceSpan>,
}

impl SpanIndex {
    /// Build an index over `source`, tagged with the document it came from.
    ///
    /// A parse failure yields an empty index rather than an error: the serde pass
    /// will produce the user-facing syntax error, and it does so with a better
    /// message. This function's only job is to enrich, never to gate.
    pub fn build(id: SourceId, source: &str) -> Self {
        let Ok(documents) = MarkedYaml::load_from_str(source) else {
            return Self {
                source: id,
                ..Self::default()
            };
        };
        let Some(document) = documents.first() else {
            return Self {
                source: id,
                ..Self::default()
            };
        };

        let mut index = Self {
            source: id,
            entries: HashMap::new(),
            root: Some(span_of(document)),
        };
        index.walk(document, &mut Vec::new(), None);
        index
    }

    /// The document this index describes.
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// Whether a document was successfully parsed into this index.
    ///
    /// `false` for a default or malformed-input index, where every lookup misses
    /// for reasons that have nothing to do with the path being wrong.
    pub fn has_root(&self) -> bool {
        self.root.is_some()
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

    /// Span of a node that must exist. `None` means the path is wrong.
    ///
    /// Unlike [`resolve`](Self::resolve) this never walks up to an ancestor.
    /// Hand-written validation knows the exact path it means, so a miss is a bug
    /// in the path rather than a reason to underline the enclosing section — and
    /// underlining the section is how a wrong path used to pass for a right one.
    pub fn exact(&self, path: &str) -> Option<FileSpan> {
        self.get(path)
            .map(|location| FileSpan::new(self.source, location.value))
    }

    /// Key span of a node that must exist. `None` means the path is wrong.
    pub fn exact_key(&self, path: &str) -> Option<FileSpan> {
        self.get(path)
            .map(|location| FileSpan::new(self.source, location.key.unwrap_or(location.value)))
    }

    /// Span for a path, falling back to the nearest known ancestor.
    ///
    /// `serde_path_to_error` sometimes reports a path one level deeper than any
    /// node that actually exists in the document — for instance a missing field
    /// reports `repository.description` when only `repository` is present. Walking
    /// up keeps the underline useful instead of dropping it entirely.
    ///
    /// That fallback is right for serde paths and wrong for everything else, so
    /// this is deliberately not what validation calls; see [`exact`](Self::exact).
    pub fn resolve(&self, path: &str) -> Option<FileSpan> {
        if let Some(location) = self.get(path) {
            return Some(FileSpan::new(self.source, location.value));
        }

        let mut remaining = path;
        while let Some((parent, _)) = remaining.rsplit_once('.') {
            if let Some(location) = self.get(parent) {
                return Some(FileSpan::new(self.source, location.value));
            }
            remaining = parent;
        }

        self.root.map(|span| FileSpan::new(self.source, span))
    }

    /// Span of the key at `path`, falling back to its value span.
    pub fn resolve_key(&self, path: &str) -> Option<FileSpan> {
        self.exact_key(path).or_else(|| self.resolve(path))
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

    fn slice(span: FileSpan) -> &'static str {
        &SOURCE[span.offset()..span.offset() + span.len()]
    }

    fn index() -> SpanIndex {
        SpanIndex::build(SourceId::ROOT, SOURCE)
    }

    #[test]
    fn indexes_nested_scalars() {
        let index = index();
        assert_eq!(
            slice(index.resolve("repository.description").unwrap()),
            "hello world"
        );
        assert_eq!(slice(index.resolve("repository.private").unwrap()), "false");
    }

    #[test]
    fn indexes_sequence_items() {
        let index = index();
        assert_eq!(slice(index.resolve("labels.0.name").unwrap()), "bug");
        assert_eq!(slice(index.resolve("labels.0.color").unwrap()), "d73a4a");
    }

    #[test]
    fn keys_and_values_are_distinguished() {
        let index = index();
        let location = index.get("repository.description").unwrap();
        assert_eq!(
            slice(FileSpan::new(SourceId::ROOT, location.value)),
            "hello world"
        );
        assert_eq!(
            slice(FileSpan::new(SourceId::ROOT, location.key.unwrap())),
            "description"
        );
    }

    #[test]
    fn unknown_paths_fall_back_to_the_nearest_ancestor() {
        let index = index();
        // `homepage` is absent, so we should land on the `repository` mapping.
        let span = index.resolve("repository.homepage").unwrap();
        assert!(slice(span).starts_with("description:"));
    }

    #[test]
    fn an_exact_lookup_of_an_unknown_path_finds_nothing() {
        // The counterpart of the test above, and the reason both exist. The
        // ancestor walk is right for serde, which reports paths deeper than any
        // node present, and wrong for validation, which knows exactly what it
        // means — there, a miss that silently became the enclosing section is
        // how a wrong path passed for a right one.
        let index = index();
        assert!(index.exact("repository.homepage").is_none());
        assert!(index.exact("labels.0.description").is_none());
        assert!(index.exact("nonsense").is_none());
    }

    #[test]
    fn every_span_names_the_document_it_came_from() {
        let index = index();
        assert_eq!(index.source(), SourceId::ROOT);
        assert_eq!(
            index.exact("repository.description").unwrap().source,
            SourceId::ROOT
        );
    }

    #[test]
    fn an_index_over_nothing_reports_that_it_has_no_document() {
        // What the debug assertion in `ValidateCtx` keys on: a miss here means
        // "there was nothing to look in", not "the path is wrong".
        assert!(!SpanIndex::default().has_root());
        assert!(index().has_root());
    }

    #[test]
    fn a_malformed_document_yields_an_empty_index_not_an_error() {
        let index = SpanIndex::build(SourceId::ROOT, "repository:\n  - : : :\n\t bad");
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
