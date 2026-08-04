//! The plan and its serialisable artifact.
//!
//! `plan --out` writes a [`PlanArtifact`]; `sync --plan` reads one back. Applying
//! a saved plan re-reads the current state and refuses to proceed if it no longer
//! matches, so a reviewed plan cannot silently apply something else (ADR-010).

use serde::{Deserialize, Serialize};

use crate::github::Target;
use crate::resources::{Change, Counts, ResourceId, ResourcePlan};

/// The complete set of changes required to reach the desired state.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The repository this plan targets.
    pub target: Target,
    /// Per-resource plans, in application order.
    pub resources: Vec<ResourcePlan>,
    /// Configurations this plan inherited from, and the commits they were read
    /// at.
    ///
    /// Recorded so that a base moving between planning and applying is reported
    /// as the base moving, rather than as the repository having drifted.
    pub bases: Vec<BaseRecord>,
}

impl Plan {
    /// An empty plan.
    pub fn new(target: Target) -> Self {
        Self {
            target,
            resources: Vec::new(),
            bases: Vec::new(),
        }
    }

    /// Record a resource's plan, skipping it when there is nothing to do.
    pub fn push(&mut self, resource: ResourcePlan) {
        if !resource.is_empty() {
            self.resources.push(resource);
        }
    }

    /// Every change, in application order.
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.resources
            .iter()
            .flat_map(|resource| resource.changes.iter())
    }

    /// Tally of the whole plan.
    pub fn counts(&self) -> Counts {
        Counts::of(self.changes())
    }

    /// Whether there is nothing to do.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Whether any change destroys existing state.
    pub fn has_destructive(&self) -> bool {
        self.changes().any(Change::is_destructive)
    }

    /// The resources this plan touches.
    pub fn touched(&self) -> Vec<ResourceId> {
        self.resources.iter().map(|resource| resource.id).collect()
    }

    /// Convert to the serialisable artifact.
    pub fn to_artifact(&self) -> PlanArtifact {
        PlanArtifact {
            version: ARTIFACT_VERSION,
            repository: self.target.slug(),
            counts: self.counts(),
            changes: self.changes().cloned().collect(),
            bases: self.bases.clone(),
        }
    }

    /// A stable fingerprint of the plan's contents.
    ///
    /// Used to detect drift between planning and applying. Deliberately derived
    /// from the changes only, never from timestamps, so a plan is reproducible.
    pub fn fingerprint(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.target.slug().hash(&mut hasher);
        // Inherited documents are part of the input, so a base that moved makes
        // this a different plan even when the resulting changes coincide.
        for base in &self.bases {
            base.reference.hash(&mut hasher);
            base.commit.hash(&mut hasher);
        }
        for change in self.changes() {
            change.resource.as_str().hash(&mut hasher);
            change.op.verb().hash(&mut hasher);
            change.key.hash(&mut hasher);
            change.payload.to_string().hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }
}

/// Version of the JSON plan format.
///
/// Part of the public interface: tooling reads these files.
pub const ARTIFACT_VERSION: u32 = 1;

/// The serialisable form of a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanArtifact {
    /// Format version.
    pub version: u32,
    /// The repository the plan was computed against, as `owner/repo`.
    pub repository: String,
    /// Tally, for quick inspection without walking the changes.
    pub counts: Counts,
    /// Every change, in application order.
    pub changes: Vec<Change>,
    /// Configurations this plan inherited from.
    ///
    /// Defaulted, so a plan written before inheritance existed still loads and
    /// the format stays at version 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<BaseRecord>,
}

/// An inherited configuration, as recorded in a saved plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BaseRecord {
    /// The reference as written, e.g. `acme/.github@v1`.
    pub reference: String,
    /// The commit the document was read at, when it could be determined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl PlanArtifact {
    /// Rebuild a [`Plan`] from an artifact.
    ///
    /// Changes are regrouped by resource, preserving their recorded order.
    pub fn to_plan(&self) -> Result<Plan, ArtifactError> {
        if self.version != ARTIFACT_VERSION {
            return Err(ArtifactError::UnsupportedVersion(self.version));
        }

        let target: Target = self
            .repository
            .parse()
            .map_err(|_| ArtifactError::InvalidRepository(self.repository.clone()))?;

        let mut plan = Plan::new(target);
        let mut current: Option<ResourcePlan> = None;

        for change in &self.changes {
            match &mut current {
                Some(resource) if resource.id == change.resource => {
                    resource.changes.push(change.clone());
                }
                _ => {
                    if let Some(finished) = current.take() {
                        plan.push(finished);
                    }
                    current = Some(ResourcePlan {
                        id: change.resource,
                        changes: vec![change.clone()],
                    });
                }
            }
        }

        if let Some(finished) = current {
            plan.push(finished);
        }

        Ok(plan)
    }
}

/// Failures reading a saved plan.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum ArtifactError {
    /// The file was written by an incompatible version.
    #[error("plan file format version {0} is not supported")]
    #[diagnostic(
        code(gh_settings::plan::unsupported_version),
        help("regenerate the plan with this version of gh-settings")
    )]
    UnsupportedVersion(u32),

    /// The recorded repository is not an `owner/repo` pair.
    #[error("plan file records an invalid repository `{0}`")]
    #[diagnostic(code(gh_settings::plan::invalid_repository))]
    InvalidRepository(String),

    /// The plan was computed against a different repository.
    #[error("plan file was computed for {expected}, but you are targeting {actual}")]
    #[diagnostic(
        code(gh_settings::plan::wrong_repository),
        help("regenerate the plan against this repository")
    )]
    WrongRepository {
        /// The repository the plan was made for.
        expected: String,
        /// The repository being targeted now.
        actual: String,
    },

    /// The repository changed between planning and applying.
    #[error("the repository has changed since this plan was computed")]
    #[diagnostic(
        code(gh_settings::plan::drift),
        help("re-run `gh settings plan` and review the new plan before applying")
    )]
    Drift,

    /// An inherited configuration changed since the plan was written.
    ///
    /// Distinguished from `Drift` because the repository being configured has
    /// not changed at all, and saying it had would send people to look in the
    /// wrong place — a shared base file is usually owned by someone else.
    #[error("the inherited configuration `{reference}` changed since this plan was written")]
    #[diagnostic(
        code(gh_settings::plan::base_moved),
        help("re-run `gh settings plan` to pick up the new base, or pin `extends` to a commit")
    )]
    BaseMoved {
        /// The reference as written.
        reference: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Op;
    use pretty_assertions::assert_eq;

    fn change(resource: ResourceId, op: Op, key: &str) -> Change {
        Change::new(resource, op, key)
    }

    fn plan_with(changes: Vec<Change>) -> Plan {
        let mut plan = Plan::new(Target::new("o", "r"));
        let mut by_resource: Vec<ResourcePlan> = Vec::new();
        for change in changes {
            match by_resource.last_mut() {
                Some(last) if last.id == change.resource => last.changes.push(change),
                _ => by_resource.push(ResourcePlan {
                    id: change.resource,
                    changes: vec![change],
                }),
            }
        }
        for resource in by_resource {
            plan.push(resource);
        }
        plan
    }

    #[test]
    fn an_empty_resource_plan_is_not_recorded() {
        let mut plan = Plan::new(Target::new("o", "r"));
        plan.push(ResourcePlan {
            id: ResourceId::Labels,
            changes: Vec::new(),
        });
        assert!(plan.is_empty());
    }

    #[test]
    fn counts_span_every_resource() {
        let plan = plan_with(vec![
            change(ResourceId::Labels, Op::Create, "a"),
            change(ResourceId::Topics, Op::Delete, "b"),
        ]);
        let counts = plan.counts();
        assert_eq!(counts.create, 1);
        assert_eq!(counts.delete, 1);
        assert!(plan.has_destructive());
    }

    #[test]
    fn artifacts_round_trip() {
        let plan = plan_with(vec![
            change(ResourceId::Labels, Op::Create, "a"),
            change(ResourceId::Labels, Op::Update, "b"),
            change(ResourceId::Topics, Op::Create, "rust"),
        ]);

        let artifact = plan.to_artifact();
        let json = serde_json::to_string(&artifact).unwrap();
        let parsed: PlanArtifact = serde_json::from_str(&json).unwrap();
        let restored = parsed.to_plan().unwrap();

        assert_eq!(restored.target, plan.target);
        assert_eq!(restored.touched(), plan.touched());
        assert_eq!(restored.counts(), plan.counts());
    }

    #[test]
    fn round_tripping_preserves_the_fingerprint() {
        // This is what makes drift detection trustworthy.
        let plan = plan_with(vec![change(ResourceId::Labels, Op::Create, "a")]);
        let restored = plan.to_artifact().to_plan().unwrap();
        assert_eq!(restored.fingerprint(), plan.fingerprint());
    }

    #[test]
    fn different_plans_fingerprint_differently() {
        let first = plan_with(vec![change(ResourceId::Labels, Op::Create, "a")]);
        let second = plan_with(vec![change(ResourceId::Labels, Op::Create, "b")]);
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn the_fingerprint_is_stable_across_runs() {
        let build = || plan_with(vec![change(ResourceId::Labels, Op::Create, "a")]);
        assert_eq!(build().fingerprint(), build().fingerprint());
    }

    #[test]
    fn a_plan_written_before_inheritance_existed_still_loads() {
        // `bases` is defaulted, which is what keeps the format at version 1.
        // Built by serialising a current plan, so the rest of the shape cannot
        // drift out from under the assertion.
        let json = serde_json::to_string(&Plan::new(Target::new("o", "r")).to_artifact())
            .expect("serialises");
        assert!(!json.contains("bases"), "{json}");

        let artifact: PlanArtifact = serde_json::from_str(&json).expect("loads");
        assert!(artifact.bases.is_empty());
        assert_eq!(artifact.version, ARTIFACT_VERSION);
    }

    #[test]
    fn a_base_at_a_different_commit_makes_a_different_plan() {
        // The inherited document is part of the input, so the fingerprint has
        // to move with it even when the resulting changes happen to coincide.
        let mut first = Plan::new(Target::new("o", "r"));
        first.bases.push(BaseRecord {
            reference: "acme/.github@v1".into(),
            commit: Some("aaaa".into()),
        });

        let mut second = Plan::new(Target::new("o", "r"));
        second.bases.push(BaseRecord {
            reference: "acme/.github@v1".into(),
            commit: Some("bbbb".into()),
        });

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn rejects_an_unknown_artifact_version() {
        let artifact = PlanArtifact {
            version: 99,
            repository: "o/r".into(),
            counts: Counts::default(),
            changes: Vec::new(),
            bases: Vec::new(),
        };
        assert!(matches!(
            artifact.to_plan(),
            Err(ArtifactError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn rejects_a_malformed_repository() {
        let artifact = PlanArtifact {
            version: ARTIFACT_VERSION,
            repository: "not-a-repo".into(),
            counts: Counts::default(),
            changes: Vec::new(),
            bases: Vec::new(),
        };
        assert!(matches!(
            artifact.to_plan(),
            Err(ArtifactError::InvalidRepository(_))
        ));
    }

    #[test]
    fn regrouping_preserves_change_order() {
        let plan = plan_with(vec![
            change(ResourceId::Labels, Op::Create, "first"),
            change(ResourceId::Labels, Op::Create, "second"),
        ]);
        let restored = plan.to_artifact().to_plan().unwrap();
        let keys: Vec<&str> = restored.changes().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["first", "second"]);
    }
}
