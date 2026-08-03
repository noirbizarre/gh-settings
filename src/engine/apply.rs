//! Applying a plan.
//!
//! Changes are applied strictly in plan order so that the sequence a user
//! reviewed is the sequence that executes.

use crate::github::{GitHubClient, GitHubError, Target};
use crate::resources::{Change, Counts, ResourceId};

use super::plan::Plan;
use super::registry::Registry;

/// How to apply a plan.
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// Keep going after a failure instead of stopping at the first one.
    ///
    /// Off by default: stopping leaves the repository in a state the user can
    /// reason about, whereas ploughing on can compound a permissions problem into
    /// dozens of identical failures.
    pub continue_on_error: bool,
    /// Report what would happen without performing any request.
    pub dry_run: bool,
}

/// What happened to a single change.
#[derive(Debug)]
pub enum ApplyOutcome {
    /// Applied successfully.
    Applied(Change),
    /// Failed.
    Failed {
        /// The change that failed.
        change: Change,
        /// Why.
        error: GitHubError,
    },
    /// Not attempted because an earlier change failed.
    Skipped(Change),
}

impl ApplyOutcome {
    /// The change this outcome concerns.
    pub fn change(&self) -> &Change {
        match self {
            Self::Applied(change) | Self::Skipped(change) => change,
            Self::Failed { change, .. } => change,
        }
    }

    /// Whether the change succeeded.
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied(_))
    }

    /// Whether the change failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// The result of applying a plan.
#[derive(Debug)]
pub struct ApplyReport {
    /// Outcome of every change, in plan order.
    pub outcomes: Vec<ApplyOutcome>,
}

impl ApplyReport {
    /// A report for a run that had nothing to do.
    ///
    /// Distinct from "no outcomes because we failed early": this is success
    /// with an empty plan, and it exists so `--format json` can say so rather
    /// than falling back to human output.
    pub fn empty() -> Self {
        Self {
            outcomes: Vec::new(),
        }
    }

    /// Whether every change succeeded.
    pub fn is_success(&self) -> bool {
        !self.outcomes.iter().any(ApplyOutcome::is_failed)
    }

    /// Tally of the changes that were actually applied.
    pub fn applied_counts(&self) -> Counts {
        Counts::of(
            self.outcomes
                .iter()
                .filter(|outcome| outcome.is_applied())
                .map(ApplyOutcome::change),
        )
    }

    /// Every failure.
    pub fn failures(&self) -> impl Iterator<Item = (&Change, &GitHubError)> {
        self.outcomes.iter().filter_map(|outcome| match outcome {
            ApplyOutcome::Failed { change, error } => Some((change, error)),
            _ => None,
        })
    }

    /// Number of changes that were skipped after a failure.
    pub fn skipped(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ApplyOutcome::Skipped(_)))
            .count()
    }

    /// Whether any failure was a permission problem.
    ///
    /// Drives the "you probably need a different token" hint, which is by far the
    /// most common cause of a failed run in CI.
    pub fn has_permission_failure(&self) -> bool {
        self.failures()
            .any(|(_, error)| error.is_permission_denied())
    }

    /// The resources that failed for permission reasons.
    pub fn permission_denied_resources(&self) -> Vec<ResourceId> {
        let mut resources: Vec<ResourceId> = self
            .failures()
            .filter(|(_, error)| error.is_permission_denied())
            .map(|(change, _)| change.resource)
            .collect();
        resources.sort();
        resources.dedup();
        resources
    }
}

/// Apply a plan.
pub async fn run(
    registry: &Registry,
    client: &dyn GitHubClient,
    target: &Target,
    plan: &Plan,
    options: &ApplyOptions,
) -> ApplyReport {
    let mut outcomes = Vec::new();
    let mut halted = false;

    for resource_plan in &plan.resources {
        let Some(resource) = registry.get(resource_plan.id) else {
            // Can only happen when applying a saved plan produced by a build that
            // knew a resource this one does not.
            for change in &resource_plan.changes {
                outcomes.push(ApplyOutcome::Skipped(change.clone()));
            }
            continue;
        };

        for change in &resource_plan.changes {
            if halted {
                outcomes.push(ApplyOutcome::Skipped(change.clone()));
                continue;
            }

            if options.dry_run {
                outcomes.push(ApplyOutcome::Applied(change.clone()));
                continue;
            }

            match resource.apply(client, target, change).await {
                Ok(()) => outcomes.push(ApplyOutcome::Applied(change.clone())),
                Err(error) => {
                    tracing::error!(
                        target: "gh_settings::apply",
                        resource = %change.resource,
                        key = %change.key,
                        %error,
                        "change failed"
                    );
                    outcomes.push(ApplyOutcome::Failed {
                        change: change.clone(),
                        error,
                    });
                    if !options.continue_on_error {
                        halted = true;
                    }
                }
            }
        }
    }

    ApplyReport { outcomes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Method;
    use crate::resources::Op;
    use pretty_assertions::assert_eq;

    fn change(resource: ResourceId, key: &str) -> Change {
        Change::new(resource, Op::Create, key)
    }

    fn failure(status: u16) -> GitHubError {
        GitHubError::Api {
            method: Method::Post,
            endpoint: "repos/o/r/labels".into(),
            status,
            message: "nope".into(),
            body: String::new(),
        }
    }

    fn report(outcomes: Vec<ApplyOutcome>) -> ApplyReport {
        ApplyReport { outcomes }
    }

    #[test]
    fn an_all_applied_report_succeeds() {
        let report = report(vec![ApplyOutcome::Applied(change(ResourceId::Labels, "a"))]);
        assert!(report.is_success());
        assert_eq!(report.applied_counts().create, 1);
    }

    #[test]
    fn a_failure_makes_the_report_fail() {
        let report = report(vec![ApplyOutcome::Failed {
            change: change(ResourceId::Labels, "a"),
            error: failure(422),
        }]);
        assert!(!report.is_success());
        assert_eq!(report.failures().count(), 1);
    }

    #[test]
    fn skipped_changes_are_counted_separately() {
        let report = report(vec![
            ApplyOutcome::Failed {
                change: change(ResourceId::Labels, "a"),
                error: failure(422),
            },
            ApplyOutcome::Skipped(change(ResourceId::Labels, "b")),
        ]);
        assert_eq!(report.skipped(), 1);
        assert_eq!(report.applied_counts().total(), 0);
    }

    #[test]
    fn permission_failures_are_singled_out() {
        // 403 means "wrong token", which needs a completely different message
        // from "your configuration is wrong".
        let report = report(vec![ApplyOutcome::Failed {
            change: change(ResourceId::Repository, "settings"),
            error: failure(403),
        }]);
        assert!(report.has_permission_failure());
        assert_eq!(
            report.permission_denied_resources(),
            vec![ResourceId::Repository]
        );
    }

    #[test]
    fn a_validation_failure_is_not_a_permission_failure() {
        let report = report(vec![ApplyOutcome::Failed {
            change: change(ResourceId::Labels, "a"),
            error: failure(422),
        }]);
        assert!(!report.has_permission_failure());
    }

    #[test]
    fn permission_denied_resources_are_deduplicated() {
        let report = report(vec![
            ApplyOutcome::Failed {
                change: change(ResourceId::Repository, "a"),
                error: failure(403),
            },
            ApplyOutcome::Failed {
                change: change(ResourceId::Repository, "b"),
                error: failure(403),
            },
        ]);
        assert_eq!(report.permission_denied_resources().len(), 1);
    }

    #[test]
    fn stopping_on_error_is_the_default() {
        assert!(!ApplyOptions::default().continue_on_error);
    }
}
