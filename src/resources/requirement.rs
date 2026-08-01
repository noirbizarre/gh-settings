//! Permission requirements, declared once per resource.
//!
//! Plan §6b: this single declaration is the source of truth for the docs page, the
//! `doctor` capability table, the `sync` pre-flight check and the context attached
//! to a `403`. Nothing about permissions is written in prose anywhere else, so the
//! four cannot drift apart.
//!
//! # Verification status
//!
//! Fine-grained permission mappings are taken from GitHub's REST reference. Where
//! we could not confirm a mapping from first-party documentation it is marked
//! [`Confidence::Unverified`] and reported as such, rather than asserted.

use serde::Serialize;

/// Access level on a fine-grained permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Read access suffices.
    Read,
    /// Write access is required.
    Write,
}

impl Access {
    /// Label used in tables.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// How sure we are that a mapping is correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Confirmed against GitHub's REST reference.
    Documented,
    /// Inferred; to be confirmed empirically before being asserted in docs.
    Unverified,
}

/// A single fine-grained permission requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FineGrained {
    /// Permission name as it appears in the token UI, e.g. `Administration`.
    pub name: &'static str,
    /// Required level.
    pub access: Access,
    /// Whether this mapping is confirmed.
    pub confidence: Confidence,
}

impl FineGrained {
    /// A documented requirement.
    pub const fn documented(name: &'static str, access: Access) -> Self {
        Self {
            name,
            access,
            confidence: Confidence::Documented,
        }
    }

    /// An inferred requirement, to be confirmed empirically.
    pub const fn unverified(name: &'static str, access: Access) -> Self {
        Self {
            name,
            access,
            confidence: Confidence::Unverified,
        }
    }
}

/// Everything a resource needs in order to be manageable.
#[derive(Debug, Clone, Serialize)]
pub struct Requirement {
    /// Fine-grained personal access token permissions.
    pub fine_grained: &'static [FineGrained],
    /// Classic personal access token scopes.
    pub classic: &'static [&'static str],
    /// Whether the Actions `GITHUB_TOKEN` can manage this resource at all.
    ///
    /// `false` for everything requiring `Administration: write`, because the
    /// workflow `permissions:` block has no `administration` key — it is not a
    /// permission someone forgot to grant, it cannot be granted.
    pub github_token_capable: bool,
    /// When `github_token_capable` is `false`, why.
    pub github_token_note: Option<&'static str>,
}

impl Requirement {
    /// The standard requirement for anything behind `Administration: write`.
    pub const ADMINISTRATION: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Administration", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: false,
        github_token_note: Some(
            "requires Administration: write, which cannot be granted to GITHUB_TOKEN",
        ),
    };

    /// The requirement for label management, which lives under `Issues`.
    pub const ISSUES: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Issues", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: true,
        github_token_note: None,
    };

    /// Whether any mapping still needs empirical confirmation.
    pub fn has_unverified(&self) -> bool {
        self.fine_grained
            .iter()
            .any(|permission| permission.confidence == Confidence::Unverified)
    }

    /// Render the fine-grained permissions as `Name: level`, for tables.
    pub fn fine_grained_summary(&self) -> String {
        self.fine_grained
            .iter()
            .map(|permission| format!("{}: {}", permission.name, permission.access.label()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Render the classic scopes as a comma-separated list.
    pub fn classic_summary(&self) -> String {
        self.classic.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Indirection so the assertions below are not folded into constants.
    fn requirement(requirement: &'static Requirement) -> &'static Requirement {
        requirement
    }

    #[test]
    fn administration_is_not_grantable_to_the_actions_token() {
        let administration = requirement(&Requirement::ADMINISTRATION);
        assert!(!administration.github_token_capable);
        assert!(
            administration.github_token_note.is_some(),
            "the reason must be shown to the user, not just the verdict"
        );
    }

    #[test]
    fn labels_are_reachable_with_the_actions_token() {
        let issues = requirement(&Requirement::ISSUES);
        assert!(issues.github_token_capable);
        assert!(issues.github_token_note.is_none());
    }

    #[test]
    fn every_requirement_demands_metadata_read() {
        // Fine-grained tokens are useless without it, so forgetting it in a new
        // resource would produce a table that cannot actually work.
        for requirement in [&Requirement::ADMINISTRATION, &Requirement::ISSUES] {
            assert!(
                requirement
                    .fine_grained
                    .iter()
                    .any(|p| p.name == "Metadata" && p.access == Access::Read),
                "missing Metadata: read"
            );
        }
    }

    #[test]
    fn summarises_for_tables() {
        assert_eq!(
            Requirement::ADMINISTRATION.fine_grained_summary(),
            "Metadata: read, Administration: write"
        );
        assert_eq!(Requirement::ADMINISTRATION.classic_summary(), "repo");
    }

    #[test]
    fn tracks_unverified_mappings() {
        const UNVERIFIED: &[FineGrained] =
            &[FineGrained::unverified("Administration", Access::Write)];
        let requirement = Requirement {
            fine_grained: UNVERIFIED,
            classic: &["repo"],
            github_token_capable: false,
            github_token_note: None,
        };
        assert!(requirement.has_unverified());
        assert!(!Requirement::ISSUES.has_unverified());
    }
}
