//! Where each configuration path physically lives.
//!
//! A validation path such as `labels.0.color` is *logical*: it names a value in
//! the effective configuration. Where that value was actually written is a
//! separate question, and the two diverge in two ways.
//!
//! [`Prunable`](super::Prunable) accepts both `labels: [...]` and
//! `labels: { prune: true, items: [...] }`, so the physical path may be
//! `labels.items.0.color`. And once a configuration can inherit from another
//! document, item *n* of the merged list may have been written in either file,
//! at a different index.
//!
//! Both were previously handled by probing the document — asking "does
//! `labels.items` exist?" — which cannot work across two documents, because the
//! answer differs per document while the probe returns one answer for all of
//! them. This map records the answer per path instead.

use std::collections::HashMap;

use super::settings::Settings;
use super::source::SourceId;
use super::spans::SpanIndex;

/// Collection sections, which are the ones whose physical path can differ.
const SECTIONS: &[&str] = &["topics", "labels", "autolinks", "rulesets"];

/// Maps logical configuration paths to the document and path that produced them.
///
/// Entries are recorded only where the logical and physical paths differ, or
/// where a path came from a document other than the root. Everything else is
/// resolved by [`Provenance::default_source`].
#[derive(Debug, Clone, Default)]
pub struct Provenance {
    entries: HashMap<String, (SourceId, String)>,
    /// Document to attribute an unmapped path to.
    ///
    /// `Some` only while exactly one document contributed, where an unmapped
    /// path is trivially that document's, unchanged. A merged configuration
    /// leaves this `None`, so a path the merge failed to record resolves to
    /// nothing and trips the assertion in `ValidateCtx` — rather than being
    /// attributed to whichever document happens to have a node there.
    default_source: Option<SourceId>,
}

impl Provenance {
    /// Build the provenance of a single document.
    pub fn for_document(id: SourceId, spans: &SpanIndex, settings: &Settings) -> Self {
        let mut provenance = Self {
            entries: HashMap::new(),
            default_source: Some(id),
        };

        for section in SECTIONS {
            let nested = format!("{section}.items");
            if !spans.contains(&nested) {
                // Bare list form: logical and physical agree, so recording the
                // identity would only cost memory.
                continue;
            }
            for position in 0..section_len(settings, section) {
                provenance.record(
                    format!("{section}.{position}"),
                    id,
                    format!("{nested}.{position}"),
                );
            }
        }

        provenance
    }

    /// Build an empty provenance for a merged configuration.
    ///
    /// No default source: every path a merge produces must be recorded, because
    /// there is no document an unrecorded one could safely belong to.
    pub fn merged() -> Self {
        Self {
            entries: HashMap::new(),
            default_source: None,
        }
    }

    /// Record where a logical path physically lives.
    pub fn record(
        &mut self,
        logical: impl Into<String>,
        source: SourceId,
        physical: impl Into<String>,
    ) {
        self.entries
            .insert(logical.into(), (source, physical.into()));
    }

    /// Resolve a logical path to the document and physical path behind it.
    ///
    /// Matches the longest recorded prefix and rewrites the remainder, so
    /// recording `labels.1` covers `labels.1.color` and everything below it.
    pub fn resolve(&self, path: &str) -> Option<(SourceId, String)> {
        let mut prefix = path;
        loop {
            if let Some((source, physical)) = self.entries.get(prefix) {
                // Truncating on `.` boundaries rather than by byte length is
                // what stops `labels.1` from matching `labels.10`.
                return Some((*source, format!("{physical}{}", &path[prefix.len()..])));
            }
            match prefix.rsplit_once('.') {
                Some((parent, _)) => prefix = parent,
                None => break,
            }
        }

        self.default_source.map(|source| (source, path.to_string()))
    }

    /// Whether anything has been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// How many items a collection section declares.
fn section_len(settings: &Settings, section: &str) -> usize {
    match section {
        "topics" => settings.topics.as_ref().map_or(0, |s| s.items().len()),
        "labels" => settings.labels.as_ref().map_or(0, |s| s.items().len()),
        "autolinks" => settings.autolinks.as_ref().map_or(0, |s| s.items().len()),
        "rulesets" => settings.rulesets.as_ref().map_or(0, |s| s.items().len()),
        other => unreachable!("unknown collection section `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn document(source: &str) -> (SpanIndex, Settings, Provenance) {
        let spans = SpanIndex::build(SourceId::ROOT, source);
        let settings: Settings = serde_norway::from_str(source).expect("valid settings");
        let provenance = Provenance::for_document(SourceId::ROOT, &spans, &settings);
        (spans, settings, provenance)
    }

    #[test]
    fn the_bare_list_form_needs_no_rewriting() {
        let (_, _, provenance) = document("labels:\n  - name: bug\n    color: d73a4a\n");
        assert!(provenance.is_empty());
        assert_eq!(
            provenance.resolve("labels.0.color"),
            Some((SourceId::ROOT, "labels.0.color".to_string()))
        );
    }

    #[test]
    fn the_object_form_is_rewritten_through_items() {
        let (_, _, provenance) =
            document("labels:\n  prune: true\n  items:\n    - name: bug\n      color: d73a4a\n");
        assert_eq!(
            provenance.resolve("labels.0.color"),
            Some((SourceId::ROOT, "labels.items.0.color".to_string()))
        );
    }

    #[test]
    fn item_ten_does_not_resolve_against_item_one() {
        // `labels.1` is a string prefix of `labels.10`. Matching by bytes rather
        // than on `.` boundaries would silently point the eleventh item at the
        // second one's span.
        let mut provenance = Provenance::merged();
        provenance.record("labels.1", SourceId::ROOT, "labels.999");

        assert_eq!(
            provenance.resolve("labels.1.color"),
            Some((SourceId::ROOT, "labels.999.color".to_string()))
        );
        assert_eq!(
            provenance.resolve("labels.10.color"),
            None,
            "item ten was never recorded, so it must resolve to nothing"
        );
    }

    #[test]
    fn a_merged_configuration_has_no_default_document() {
        // An unrecorded path must resolve to nothing rather than being
        // attributed to whichever document happens to have a node there.
        let provenance = Provenance::merged();
        assert_eq!(provenance.resolve("labels.0.color"), None);
    }

    #[test]
    fn the_longest_recorded_prefix_wins() {
        let mut provenance = Provenance::merged();
        provenance.record("labels", SourceId::ROOT, "labels.items");
        provenance.record("labels.3", SourceId::ROOT, "labels.items.7");

        assert_eq!(
            provenance.resolve("labels.3.name"),
            Some((SourceId::ROOT, "labels.items.7.name".to_string()))
        );
        assert_eq!(
            provenance.resolve("labels.2.name"),
            Some((SourceId::ROOT, "labels.items.2.name".to_string()))
        );
    }
}
