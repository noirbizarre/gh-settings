//! Permission requirements, declared once per resource.
//!
//! Plan §6b: this single declaration is the source of truth for the docs page, the
//! `doctor` capability table, the `sync` pre-flight check and the context attached
//! to a `403`. Nothing about permissions is written in prose anywhere else, so the
//! four cannot drift apart.
//!
//! [`Requirement::verdict`] is the one place a credential is judged. `doctor`
//! reports it, and `sync` refuses on it — if the two disagreed, `doctor` would be
//! telling users something `sync` does not act on, which is worse than either
//! being wrong alone.
//!
//! # Verification status
//!
//! Fine-grained permission mappings are taken from GitHub's REST reference. Where
//! we could not confirm a mapping from first-party documentation it is marked
//! [`Confidence::Unverified`] and reported as such, rather than asserted.

use serde::Serialize;

use crate::github::auth::{AuthStatus, TokenKind};

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

    /// Actions variables, at repository and at environment scope.
    ///
    /// The fine-grained permission is spelled `Variables` in the token UI, and
    /// it is separate from `Actions` — a token that can rerun workflows cannot
    /// necessarily write variables.
    ///
    /// Marked unverified: GitHub's reference has described the *environment*-
    /// scoped endpoints as sitting under `Actions: write` in places, and we
    /// could not confirm from first-party documentation which is authoritative.
    /// Per ADR-015 that is reported as uncertainty rather than asserted.
    ///
    /// This is not the secrets exclusion of ADR-009: variable values are
    /// readable, so they diff, export and round-trip like anything else.
    pub const VARIABLES: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::unverified("Variables", Access::Write),
        ],
        classic: &["repo"],
        // The workflow `permissions:` block has no `variables` key, so this is
        // not a grant somebody forgot to make — it is one that cannot be made.
        github_token_capable: false,
        github_token_note: Some(
            "requires Variables: write, which cannot be granted to GITHUB_TOKEN",
        ),
    };

    /// GitHub Pages.
    ///
    /// The odd one out among the repository-level settings: `pages` *is* a key
    /// in the workflow `permissions:` block, so unlike `administration` and
    /// `variables` this is a grant an Actions workflow can actually make.
    pub const PAGES: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Pages", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: true,
        github_token_note: None,
    };

    /// What it takes to read a configuration inherited from another repository.
    /// Not a resource requirement: it is needed while *loading* the
    /// configuration, before any resource is consulted. Listed here because it
    /// is a permission, and this is where permissions are declared.
    ///
    /// The Actions `GITHUB_TOKEN` is scoped to the repository running the
    /// workflow, so it cannot read a base held anywhere else — the same shape of
    /// problem as `Administration: write`, and worth saying before someone
    /// spends an afternoon on a `404`.
    pub const CONTENTS: Requirement = Requirement {
        fine_grained: &[FineGrained::documented("Contents", Access::Read)],
        classic: &["repo"],
        github_token_capable: false,
        github_token_note: Some(
            "requires Contents: read on the *other* repository, which GITHUB_TOKEN does not have",
        ),
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

    /// Judge a credential against this requirement.
    ///
    /// The only place a token is assessed. `doctor` renders the result and
    /// `sync` refuses to start on [`Capability::Impossible`], so a change here
    /// moves both at once, by construction.
    ///
    /// Every branch that cannot be established from evidence returns
    /// [`Capability::Unknown`]. That is not a gap to be filled in later: a
    /// wrong "impossible" blocks a token that would have worked, and the user
    /// has no way to appeal it. Refusing only when certain is the whole design.
    pub fn verdict(&self, auth: Option<&AuthStatus>) -> Capability {
        let Some(auth) = auth else {
            return Capability::Unknown;
        };

        match auth.token_kind {
            // The one case we can state with certainty. The workflow
            // `permissions:` block has no `administration` key, so this is not
            // a scope the user forgot to grant.
            TokenKind::ActionsGitHubToken if !self.github_token_capable => Capability::Impossible(
                self.github_token_note
                    .unwrap_or("not available to GITHUB_TOKEN"),
            ),
            TokenKind::ActionsGitHubToken => Capability::Manageable,

            // Classic tokens advertise their scopes, so we can be exact.
            _ => match self
                .classic
                .iter()
                .map(|scope| auth.scopes.grants(scope))
                .collect::<Option<Vec<bool>>>()
            {
                Some(granted) if granted.iter().all(|granted| *granted) => Capability::Manageable,
                Some(_) => Capability::Impossible("missing the `repo` scope"),
                // Fine-grained and App tokens do not report scopes. Fall back to
                // whether the token has admin rights on this repository, and say
                // "unknown" when even that could not be established.
                None => match auth.admin_on_target {
                    Some(true) => Capability::Manageable,
                    Some(false) if !self.github_token_capable => {
                        Capability::Impossible("the token has no admin rights on this repository")
                    }
                    _ => Capability::Unknown,
                },
            },
        }
    }
}

/// Whether a resource can be managed with the current credential.
///
/// Lives beside [`Requirement`] rather than in `output` because it is a verdict,
/// not a rendering: `sync` acts on it without printing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// It can be managed.
    Manageable,
    /// It definitely cannot, with the reason.
    Impossible(&'static str),
    /// We cannot tell — reported honestly rather than guessed.
    Unknown,
}

impl Capability {
    /// Whether this verdict is certain enough to refuse a write on.
    ///
    /// [`Capability::Unknown`] deliberately answers `false`: an unintrospectable
    /// token must be allowed to try, so that GitHub's own error is what the user
    /// sees rather than our guess about it.
    pub fn is_certainly_impossible(&self) -> bool {
        matches!(self, Self::Impossible(_))
    }

    /// The reason, when there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Impossible(reason) => Some(reason),
            _ => None,
        }
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
    fn pages_are_reachable_with_the_actions_token() {
        // `pages: write` is a real key in the workflow `permissions:` block, so
        // a Pages-only CI workflow needs no extra credential.
        let pages = requirement(&Requirement::PAGES);
        assert!(pages.github_token_capable);
        assert!(pages.github_token_note.is_none());
    }

    #[test]
    fn every_requirement_demands_metadata_read() {
        // Fine-grained tokens are useless without it, so forgetting it in a new
        // resource would produce a table that cannot actually work.
        for requirement in [
            &Requirement::ADMINISTRATION,
            &Requirement::ISSUES,
            &Requirement::VARIABLES,
            &Requirement::PAGES,
        ] {
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

    mod verdict {
        use super::{AuthStatus, Capability, Requirement, TokenKind};
        use crate::github::auth::Scopes;
        use pretty_assertions::assert_eq;

        fn auth(
            token_kind: TokenKind,
            scopes: Scopes,
            admin_on_target: Option<bool>,
        ) -> AuthStatus {
            AuthStatus {
                hostname: "github.com".into(),
                account: Some("tester".into()),
                token_kind,
                scopes,
                admin_on_target,
            }
        }

        fn classic(scopes: &[&str]) -> AuthStatus {
            auth(
                TokenKind::ClassicPat,
                Scopes::Known(scopes.iter().map(|s| (*s).to_string()).collect()),
                None,
            )
        }

        #[test]
        fn a_classic_token_with_the_scope_can_manage_everything() {
            assert_eq!(
                Requirement::ADMINISTRATION.verdict(Some(&classic(&["repo"]))),
                Capability::Manageable
            );
        }

        #[test]
        fn a_classic_token_without_the_scope_is_certainly_blocked() {
            // Scopes are advertised, so this is one of the few cases we may
            // state outright.
            let verdict = Requirement::ADMINISTRATION.verdict(Some(&classic(&["gist"])));
            assert!(verdict.is_certainly_impossible());
            assert_eq!(verdict.reason(), Some("missing the `repo` scope"));
        }

        #[test]
        fn the_actions_token_cannot_reach_administration() {
            let verdict = Requirement::ADMINISTRATION.verdict(Some(&auth(
                TokenKind::ActionsGitHubToken,
                Scopes::Unknown,
                None,
            )));
            assert_eq!(
                verdict.reason(),
                Requirement::ADMINISTRATION.github_token_note,
                "the verdict must carry the resource's own explanation"
            );
        }

        #[test]
        fn the_actions_token_can_still_manage_labels() {
            // The documented labels-only CI workflow depends on this.
            assert_eq!(
                Requirement::ISSUES.verdict(Some(&auth(
                    TokenKind::ActionsGitHubToken,
                    Scopes::Unknown,
                    None
                ))),
                Capability::Manageable
            );
        }

        #[test]
        fn an_unintrospectable_token_is_unknown_rather_than_blocked() {
            // A fine-grained token reports no scopes, and the repository read
            // failed, so `admin_on_target` is `None`. Anything other than
            // `Unknown` here would have `sync` refuse a token that may well
            // work, with no way for the user to overrule it.
            let verdict = Requirement::ADMINISTRATION.verdict(Some(&auth(
                TokenKind::FineGrainedPat,
                Scopes::Unknown,
                None,
            )));
            assert_eq!(verdict, Capability::Unknown);
            assert!(
                !verdict.is_certainly_impossible(),
                "an unknown verdict must never block a write"
            );
        }

        #[test]
        fn no_credential_at_all_is_unknown() {
            let verdict = Requirement::ADMINISTRATION.verdict(None);
            assert_eq!(verdict, Capability::Unknown);
            assert!(!verdict.is_certainly_impossible());
        }

        #[test]
        fn the_admin_probe_settles_a_token_that_reports_no_scopes() {
            assert_eq!(
                Requirement::ADMINISTRATION.verdict(Some(&auth(
                    TokenKind::FineGrainedPat,
                    Scopes::Unknown,
                    Some(true)
                ))),
                Capability::Manageable
            );
        }

        #[test]
        fn a_token_without_admin_rights_cannot_manage_administration() {
            let verdict = Requirement::ADMINISTRATION.verdict(Some(&auth(
                TokenKind::FineGrainedPat,
                Scopes::Unknown,
                Some(false),
            )));
            assert!(verdict.is_certainly_impossible());

            // ...but labels do not need admin, so the same credential is fine.
            assert_eq!(
                Requirement::ISSUES.verdict(Some(&auth(
                    TokenKind::FineGrainedPat,
                    Scopes::Unknown,
                    Some(false)
                ))),
                Capability::Unknown
            );
        }
    }
}
