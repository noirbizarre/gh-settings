//! The resource abstraction.
//!
//! Every GitHub feature is an independent [`Resource`]. Adding support for a new
//! setting means writing one module here and adding one line to the registry —
//! never touching the engine (ADR-001).
//!
//! [`Resource`] is deliberately *not* object safe: associated `Desired`/`Current`
//! types are what make each resource strongly typed and its `diff` a pure,
//! trivially testable function. Object safety is recovered by [`ErasedResource`],
//! which is blanket-implemented for every `Resource`, so the engine can still hold
//! a `Vec<Box<dyn ErasedResource>>`.

pub mod change;
pub mod requirement;

pub mod autolinks;
pub mod environments;
pub mod labels;
pub mod repository;
pub mod rulesets;
pub mod topics;
pub mod variables;

pub use change::{Change, Counts, FieldDiff, Op};
pub use requirement::{Access, Capability, Requirement};

use std::fmt;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::{Finding, Settings, SpanIndex};
use crate::github::{GitHubClient, Result as GitHubResult, Target};

/// Stable identifier of a resource.
///
/// Used for ordering, for `--only`, for plan output headings and as the key of the
/// JSON plan artifact, so the string values are part of the public interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceId {
    /// Repository metadata: description, homepage, merge and security settings.
    Repository,
    /// Repository topics.
    Topics,
    /// Issue and pull request labels.
    Labels,
    /// Autolink references.
    Autolinks,
    /// Repository rulesets.
    Rulesets,
    /// Deployment environments and their protection rules.
    Environments,
    /// Actions variables, at repository and environment scope.
    Variables,
}

impl ResourceId {
    /// Every resource, in declaration order.
    pub const ALL: &'static [ResourceId] = &[
        Self::Repository,
        Self::Topics,
        Self::Labels,
        Self::Autolinks,
        Self::Rulesets,
        Self::Environments,
        Self::Variables,
    ];

    /// The identifier as it appears in `--only`, plan output and JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Topics => "topics",
            Self::Labels => "labels",
            Self::Autolinks => "autolinks",
            Self::Rulesets => "rulesets",
            Self::Environments => "environments",
            Self::Variables => "variables",
        }
    }

    /// Heading used in human-readable plan output.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Repository => "Repository",
            Self::Topics => "Topics",
            Self::Labels => "Labels",
            Self::Autolinks => "Autolinks",
            Self::Rulesets => "Rulesets",
            Self::Environments => "Environments",
            Self::Variables => "Variables",
        }
    }

    /// Parse an identifier, for `--only`.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|id| id.as_str().eq_ignore_ascii_case(value))
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ResourceId {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| {
            let names: Vec<&str> = ResourceId::ALL.iter().map(|id| id.as_str()).collect();
            match crate::config::suggest(value, &names) {
                Some(best) => format!("unknown resource `{value}`, did you mean `{best}`?"),
                None => format!(
                    "unknown resource `{value}`, expected one of: {}",
                    names.join(", ")
                ),
            }
        })
    }
}

/// Whether unmanaged items may be deleted.
///
/// Off by default (ADR-005): running `sync` against an existing repository must
/// never silently destroy configuration that predates adoption of this tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneOpts {
    /// Global override from `--prune` / `--no-prune`.
    pub force: Option<bool>,
}

impl PruneOpts {
    /// Resolve the effective prune setting given a resource's own declaration.
    ///
    /// The command line always wins, so an operator can both opt in to a
    /// destructive run and, more importantly, opt out of one.
    pub fn resolve(&self, declared: bool) -> bool {
        self.force.unwrap_or(declared)
    }
}

/// Context handed to [`Resource::validate`].
///
/// Carries the span indices so a resource can attach a precise source location
/// to its findings without knowing anything about YAML parsing.
pub struct ValidateCtx<'a> {
    /// Span index per contributing document, indexed by
    /// [`SourceId`](crate::config::SourceId).
    indices: &'a [SpanIndex],
    /// Where each logical path physically lives.
    ///
    /// `None` for a single document validated directly, where every logical
    /// path is its own physical path in the only document there is.
    provenance: Option<&'a crate::config::Provenance>,
}

impl<'a> ValidateCtx<'a> {
    /// Build a context over a single document, with no rewriting.
    pub fn new(spans: &'a SpanIndex) -> Self {
        Self {
            indices: std::slice::from_ref(spans),
            provenance: None,
        }
    }

    /// Build a context over every contributing document.
    pub fn resolved(indices: &'a [SpanIndex], provenance: &'a crate::config::Provenance) -> Self {
        Self {
            indices,
            provenance: Some(provenance),
        }
    }

    /// Find the document and physical path behind a logical one.
    ///
    /// There is deliberately no fallback across documents. Guessing which file a
    /// path belongs to is how a span from one document came to be rendered
    /// against another, and a guess that is usually right is worse than none —
    /// it hides the times it is wrong.
    fn locate(&self, path: &str) -> Option<(&SpanIndex, String)> {
        let (source, physical) = match self.provenance {
            Some(provenance) => provenance.resolve(path)?,
            None => (crate::config::SourceId::ROOT, path.to_string()),
        };
        Some((self.indices.get(source.index())?, physical))
    }

    /// Whether any document was parsed at all.
    ///
    /// A miss against an index with no document means "there was nothing to look
    /// in", which is not the same as the path being wrong.
    fn any_document_parsed(&self) -> bool {
        self.indices.iter().any(SpanIndex::has_root)
    }

    /// Span of a configuration path, for attaching to a [`Finding`].
    ///
    /// `None` when no document declares the path — which is a bug in the caller,
    /// not a user error, since a validation path is computed from a value that
    /// was just read out of the document.
    pub fn span(&self, path: &str) -> Option<crate::config::FileSpan> {
        let found = self
            .locate(path)
            .and_then(|(index, physical)| index.exact(&physical));
        debug_assert!(
            found.is_some() || !self.any_document_parsed(),
            "no node at `{path}`: the validation path does not match the document. \
             Underlining the enclosing section instead is how this stayed hidden."
        );
        found
    }

    /// Span of a configuration *key*.
    pub fn key_span(&self, path: &str) -> Option<crate::config::FileSpan> {
        let found = self
            .locate(path)
            .and_then(|(index, physical)| index.exact_key(&physical));
        debug_assert!(
            found.is_some() || !self.any_document_parsed(),
            "no node at `{path}`: the validation path does not match the document"
        );
        found
    }

    /// Whether any document declares this path.
    pub fn contains(&self, path: &str) -> bool {
        self.locate(path)
            .is_some_and(|(index, physical)| index.contains(&physical))
    }
}

/// One GitHub feature, managed declaratively.
#[async_trait]
pub trait Resource: Send + Sync {
    /// Desired state, projected from the configuration file.
    type Desired: Send + Sync;
    /// Current state, read from GitHub and already normalised.
    type Current: Send + Sync;

    /// Stable identifier.
    fn id(&self) -> ResourceId;

    /// Permissions this resource needs. Drives docs, `doctor` and the `sync`
    /// pre-flight from a single declaration (plan §6b).
    fn requirement(&self) -> &'static Requirement;

    /// Resources that must be applied before this one.
    ///
    /// The registry topologically sorts on this. No v1 resource declares a
    /// dependency, but environments and their variables will, and retrofitting
    /// ordering after the fact is far more expensive than carrying it now.
    fn depends_on(&self) -> &'static [ResourceId] {
        &[]
    }

    /// Project the desired state out of the configuration.
    ///
    /// `None` means the section is absent, which is *not* the same as an empty
    /// section: an absent section is unmanaged, an empty one may prune everything.
    fn desired(&self, settings: &Settings) -> Option<Self::Desired>;

    /// Check the desired state in isolation. No network.
    fn validate(&self, _desired: &Self::Desired, _ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        Vec::new()
    }

    /// Enrich the desired state with information only GitHub can supply.
    ///
    /// Exists so that [`Self::diff`] can stay pure. Rulesets need it: bypass
    /// actors are declared by slug (`{ team: engineering }`) because identifiers
    /// are neither stable nor meaningful to a human, but the API wants a numeric
    /// `actor_id`. Resolving during the diff would make it async and untestable;
    /// resolving here keeps the diff a pure function and surfaces a bad slug as a
    /// single up-front error rather than mid-apply.
    ///
    /// The default is the identity, so no other resource pays for this.
    async fn prepare(
        &self,
        _client: &dyn GitHubClient,
        _target: &Target,
        desired: Self::Desired,
    ) -> GitHubResult<Self::Desired> {
        Ok(desired)
    }

    /// Read the current state from GitHub, normalised for comparison.
    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current>;

    /// Compute the changes needed to reach the desired state.
    ///
    /// Pure, synchronous and total: this is where the overwhelming majority of the
    /// test suite lives.
    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        prune: &PruneOpts,
    ) -> Vec<Change>;

    /// Apply a single change.
    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()>;

    /// Render the current state as a configuration section, for `export`.
    ///
    /// `None` omits the section entirely, which keeps exported files free of
    /// noise like `topics: []`.
    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>>;
}

/// The plan for a single resource.
#[derive(Debug, Clone)]
pub struct ResourcePlan {
    /// Which resource this covers.
    pub id: ResourceId,
    /// Changes required, in application order.
    pub changes: Vec<Change>,
}

impl ResourcePlan {
    /// Whether anything needs doing.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Object-safe view of a [`Resource`].
///
/// The engine only ever sees this. It is blanket-implemented, so implementing
/// [`Resource`] is sufficient and no resource author ever writes an impl here.
#[async_trait]
pub trait ErasedResource: Send + Sync {
    /// Stable identifier.
    fn id(&self) -> ResourceId;

    /// Required permissions.
    fn requirement(&self) -> &'static Requirement;

    /// Ordering constraints.
    fn depends_on(&self) -> &'static [ResourceId];

    /// Whether the configuration manages this resource at all.
    fn is_managed(&self, settings: &Settings) -> bool;

    /// Validate the configuration section, without touching the network.
    fn validate(&self, settings: &Settings, ctx: &ValidateCtx<'_>) -> Vec<Finding>;

    /// Read current state and diff it against the configuration.
    async fn plan(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        settings: &Settings,
        prune: &PruneOpts,
    ) -> GitHubResult<ResourcePlan>;

    /// Apply a single change.
    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()>;

    /// Render current state as a configuration section.
    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>>;
}

#[async_trait]
impl<R> ErasedResource for R
where
    R: Resource,
{
    fn id(&self) -> ResourceId {
        Resource::id(self)
    }

    fn requirement(&self) -> &'static Requirement {
        Resource::requirement(self)
    }

    fn depends_on(&self) -> &'static [ResourceId] {
        Resource::depends_on(self)
    }

    fn is_managed(&self, settings: &Settings) -> bool {
        self.desired(settings).is_some()
    }

    fn validate(&self, settings: &Settings, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        match self.desired(settings) {
            Some(desired) => Resource::validate(self, &desired, ctx),
            None => Vec::new(),
        }
    }

    async fn plan(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        settings: &Settings,
        prune: &PruneOpts,
    ) -> GitHubResult<ResourcePlan> {
        let id = Resource::id(self);
        // An unmanaged section produces no changes at all: absence of
        // configuration must never be read as "delete everything".
        let Some(desired) = self.desired(settings) else {
            return Ok(ResourcePlan {
                id,
                changes: Vec::new(),
            });
        };
        let desired = self.prepare(client, target, desired).await?;
        let current = self.current(client, target).await?;
        Ok(ResourcePlan {
            id,
            changes: self.diff(&desired, &current, prune),
        })
    }

    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()> {
        Resource::apply(self, client, target, change).await
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        Resource::export(self, client, target).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn identifiers_round_trip() {
        for id in ResourceId::ALL {
            assert_eq!(ResourceId::parse(id.as_str()), Some(*id));
        }
    }

    #[test]
    fn identifier_parsing_is_case_insensitive() {
        assert_eq!(ResourceId::parse("LABELS"), Some(ResourceId::Labels));
    }

    #[test]
    fn unknown_identifiers_suggest() {
        let error = "lables".parse::<ResourceId>().unwrap_err();
        assert!(error.contains("did you mean `labels`?"), "{error}");
    }

    #[test]
    fn unknown_identifiers_list_candidates_when_nothing_is_close() {
        let error = "zzzz".parse::<ResourceId>().unwrap_err();
        assert!(error.contains("expected one of"), "{error}");
    }

    #[test]
    fn prune_defaults_to_the_declared_value() {
        let opts = PruneOpts::default();
        assert!(!opts.resolve(false));
        assert!(opts.resolve(true));
    }

    #[test]
    fn the_command_line_overrides_the_configuration_in_both_directions() {
        assert!(PruneOpts { force: Some(true) }.resolve(false));
        assert!(!PruneOpts { force: Some(false) }.resolve(true));
    }

    mod validate_ctx {
        use super::ValidateCtx;
        use crate::config::{Provenance, SourceId, Sources, SpanIndex};
        use pretty_assertions::assert_eq;

        const BASE: &str = "repository:\n  description: from the base\n  homepage: https://base\n";
        const LOCAL: &str = "repository:\n  description: from the local file\n";

        /// Two documents, as inheritance would produce, with provenance
        /// recording which one each path actually came from.
        fn inherited() -> (Sources, Vec<SpanIndex>, Provenance) {
            let (mut sources, root) = Sources::root("local.yml", LOCAL);
            let base = sources.push("acme/.github@v1", BASE);

            let indices = vec![SpanIndex::build(root, LOCAL), SpanIndex::build(base, BASE)];

            let mut provenance = Provenance::merged();
            // The local file overrode the description; the homepage is the
            // base's alone.
            provenance.record("repository.description", root, "repository.description");
            provenance.record("repository.homepage", base, "repository.homepage");

            (sources, indices, provenance)
        }

        #[test]
        fn a_path_the_local_file_overrode_points_at_the_local_file() {
            let (sources, indices, provenance) = inherited();
            let ctx = ValidateCtx::resolved(&indices, &provenance);

            let span = ctx.span("repository.description").expect("declared twice");
            assert_eq!(span.source, SourceId::ROOT);

            let text = &sources.get(span.source).text[span.offset()..span.offset() + span.len()];
            assert_eq!(text, "from the local file");
        }

        #[test]
        fn a_path_only_the_base_declares_points_at_the_base() {
            let (sources, indices, provenance) = inherited();
            let ctx = ValidateCtx::resolved(&indices, &provenance);

            let span = ctx
                .span("repository.homepage")
                .expect("declared in the base");
            assert_ne!(span.source, SourceId::ROOT);

            // The offset is only meaningful against the base's own text. Read
            // against the local file it would be a valid index into unrelated
            // characters, which is the failure this design exists to prevent.
            let text = &sources.get(span.source).text[span.offset()..span.offset() + span.len()];
            assert_eq!(text, "https://base");
        }

        #[test]
        fn an_unrecorded_path_resolves_to_nothing_rather_than_guessing() {
            // Both documents have a `repository` node, so a fallback across
            // documents would find *something* and be right often enough to hide
            // the times it was wrong.
            //
            // Asserted through `contains`, which takes the same path as `span`
            // without its debug assertion — in a debug build `span` panics here,
            // which is the intended treatment of a caller that invented a path.
            let (_, indices, provenance) = inherited();
            let ctx = ValidateCtx::resolved(&indices, &provenance);
            assert!(!ctx.contains("repository.private"));
        }

        #[test]
        fn a_single_document_context_still_resolves_against_itself() {
            let spans = SpanIndex::build(SourceId::ROOT, "topics:\n  - rust\n");
            let ctx = ValidateCtx::new(&spans);
            assert_eq!(ctx.span("topics.0").unwrap().source, SourceId::ROOT);
        }
    }
}
