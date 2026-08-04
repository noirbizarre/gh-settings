//! Source document identity, so a byte offset knows what it indexes into.
//!
//! A [`miette::SourceSpan`] is an offset and a length and nothing else. That is
//! sufficient while a configuration is exactly one file, and silently wrong the
//! moment it is not: an offset computed against one document is still a *valid*
//! index into another, so a span used against the wrong text produces a
//! confident underline over unrelated characters rather than an error.
//!
//! [`FileSpan`] pairs an offset with the document it came from, and [`Sources`]
//! owns the documents. Findings carry the identity; the text is looked up once,
//! at render time.

use std::sync::Arc;

use miette::SourceSpan;

/// Identity of one configuration document.
///
/// An index rather than a pointer or a path. A [`Finding`](crate::config::Finding)
/// is data — cloned, sorted and serialised — whereas the document text is
/// rendering context needed once, so handing every finding an `Arc` to the same
/// file would widen the hot struct for a value dereferenced only at the end.
///
/// A path would not do either: two inheritance chains can reach the same file,
/// and a document fetched from another repository has no local path at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(u32);

impl SourceId {
    /// The document the user actually invoked the tool on.
    ///
    /// Always index zero: [`Sources::root`] registers it first and anything
    /// inherited can only be appended. Fixing it as a constant lets a report
    /// choose a primary document without searching.
    pub const ROOT: SourceId = SourceId(0);

    /// Position in the owning [`Sources`].
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// One configuration document.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Identity, and index into the owning [`Sources`].
    pub id: SourceId,
    /// Display name, as it should appear in a diagnostic.
    pub name: String,
    /// The text, retained so diagnostics can quote it.
    ///
    /// Shared rather than owned per clone: a report is cloned around the CLI
    /// and copying every file each time buys nothing.
    pub text: Arc<str>,
}

/// Every document that contributed to one configuration.
///
/// Ordered, with the root first. Inheritance appends, so a document's position
/// is also its distance from the file the user invoked us on.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    files: Vec<SourceFile>,
}

impl Sources {
    /// Register the root document, returning the registry and its id.
    pub fn root(name: impl Into<String>, text: impl AsRef<str>) -> (Self, SourceId) {
        let mut sources = Self::default();
        let id = sources.push(name, text);
        debug_assert_eq!(
            id,
            SourceId::ROOT,
            "the root document must be registered first"
        );
        (sources, id)
    }

    /// Register a document, returning its id.
    pub fn push(&mut self, name: impl Into<String>, text: impl AsRef<str>) -> SourceId {
        let id = SourceId(self.files.len() as u32);
        self.files.push(SourceFile {
            id,
            name: name.into(),
            text: Arc::from(text.as_ref()),
        });
        id
    }

    /// Look up a document.
    ///
    /// # Panics
    ///
    /// If the id did not come from this registry. Ids are minted here and never
    /// constructed by callers, so that is a programming error rather than
    /// anything a user can provoke.
    pub fn get(&self, id: SourceId) -> &SourceFile {
        self.files
            .get(id.index())
            .unwrap_or_else(|| panic!("{id:?} does not belong to this registry"))
    }

    /// The root document.
    pub fn root_file(&self) -> &SourceFile {
        self.get(SourceId::ROOT)
    }

    /// Every document, root first.
    pub fn iter(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }

    /// How many documents contributed.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether no document has been registered.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Build the miette source for one document, ready to render.
    pub fn named(&self, id: SourceId) -> miette::NamedSource<String> {
        let file = self.get(id);
        miette::NamedSource::new(&file.name, file.text.to_string()).with_language("yaml")
    }
}

/// A byte span, qualified by the document it indexes into.
///
/// The whole point of this module: an offset is meaningless without the text it
/// was computed against, and until this type existed nothing said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSpan {
    /// Which document.
    pub source: SourceId,
    /// Where within it.
    pub span: SourceSpan,
}

impl FileSpan {
    /// Pair a span with its document.
    pub fn new(source: SourceId, span: SourceSpan) -> Self {
        Self { source, span }
    }

    /// Byte offset within the document.
    pub fn offset(&self) -> usize {
        self.span.offset()
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.span.len()
    }

    /// Whether the span covers nothing.
    pub fn is_empty(&self) -> bool {
        self.span.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_root_document_is_always_the_first_one() {
        let (sources, id) = Sources::root(".github/settings.yml", "version: 1\n");
        assert_eq!(id, SourceId::ROOT);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources.root_file().name, ".github/settings.yml");
    }

    #[test]
    fn inherited_documents_are_appended_in_order() {
        let (mut sources, root) = Sources::root("local.yml", "version: 1\n");
        let base = sources.push("acme/.github@v1", "labels: []\n");
        let deeper = sources.push("acme/other@v2", "topics: []\n");

        assert_eq!(root, SourceId::ROOT);
        assert_ne!(base, root);
        assert_ne!(deeper, base);
        assert_eq!(sources.len(), 3);
        assert_eq!(sources.get(base).name, "acme/.github@v1");
    }

    #[test]
    fn a_document_keeps_its_own_text() {
        // The failure this module exists to prevent: two documents whose
        // offsets overlap but whose contents do not.
        let (mut sources, root) = Sources::root("a.yml", "aaaaaaaaaaaa");
        let other = sources.push("b.yml", "bbbb");

        assert_eq!(&*sources.get(root).text, "aaaaaaaaaaaa");
        assert_eq!(&*sources.get(other).text, "bbbb");
    }

    #[test]
    fn a_span_carries_the_document_it_indexes_into() {
        let (mut sources, root) = Sources::root("a.yml", "aaaaaaaaaaaa");
        let other = sources.push("b.yml", "bbbb");

        let here = FileSpan::new(root, SourceSpan::new(8.into(), 2usize));
        let there = FileSpan::new(other, SourceSpan::new(0.into(), 2usize));

        // The same offset would be a valid index into either document. Only the
        // source makes them distinguishable.
        assert_ne!(here.source, there.source);
        assert_eq!(here.offset(), 8);
    }

    #[test]
    fn the_named_source_carries_the_documents_own_name_and_text() {
        use miette::SourceCode;

        let (mut sources, _) = Sources::root("a.yml", "first document\n");
        let other = sources.push("b.yml", "second document\n");

        let named = sources.named(other);
        assert_eq!(named.name(), "b.yml");

        let contents = named
            .read_span(&SourceSpan::new(0.into(), 6usize), 0, 0)
            .expect("readable span");
        assert_eq!(
            String::from_utf8_lossy(contents.data()).trim_end(),
            "second"
        );
    }
}
