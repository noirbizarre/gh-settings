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
    /// Four permissions, because the two scopes are not the same permission and
    /// neither of them lets us find the environments in the first place:
    ///
    /// * `/repos/{o}/{r}/actions/variables` is **Variables**;
    /// * `/repos/{o}/{r}/environments/{name}/variables` is **Environments** —
    ///   the scope, not the payload, decides;
    /// * `GET /repos/{o}/{r}/environments`, which we call to discover the
    ///   environment scopes at all, is **Actions: read**.
    ///
    /// The earlier note here guessed that the environment-scoped endpoints sat
    /// under `Actions: write`. They do not; the reference lists them under
    /// Environments, and the `Actions` entry is read-only and for a different
    /// request. See ADR-020: these categories do not nest.
    ///
    /// This is not the secrets exclusion of ADR-009: variable values are
    /// readable, so they diff, export and round-trip like anything else.
    pub const VARIABLES: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Actions", Access::Read),
            FineGrained::documented("Variables", Access::Write),
            FineGrained::documented("Environments", Access::Write),
        ],
        classic: &["repo"],
        // The workflow `permissions:` block has no `variables` key, so this is
        // not a grant somebody forgot to make — it is one that cannot be made.
        // The same is true of `environments`; naming one suffices to explain
        // the refusal, and naming both would say the same thing twice.
        github_token_capable: false,
        github_token_note: Some(
            "requires Variables: write, which cannot be granted to GITHUB_TOKEN",
        ),
    };

    /// Deployment environments.
    ///
    /// Cannot share [`Self::ADMINISTRATION`], because managing an environment
    /// is spread across three permissions that do not contain one another:
    ///
    /// * `PUT`/`DELETE /repos/{o}/{r}/environments/{name}` and the deployment
    ///   branch policies are **Administration: write**;
    /// * `GET /repos/{o}/{r}/environments` — the read every plan starts with —
    ///   is **Actions: read**;
    /// * `GET .../environments/{name}/variables`, which `export` emits under
    ///   this section (ADR-018), is **Environments: read**.
    ///
    /// So a token holding `Administration: write` and nothing else cannot even
    /// *list* the environments it is allowed to write. That is the kind of
    /// surprise this declaration exists to spell out (ADR-020).
    pub const ENVIRONMENTS: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Actions", Access::Read),
            FineGrained::documented("Environments", Access::Read),
            FineGrained::documented("Administration", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: false,
        // Byte-identical to ADMINISTRATION's on purpose: the docs generator
        // groups resources by this string, and a paraphrase would split the
        // sentence in two.
        github_token_note: Some(
            "requires Administration: write, which cannot be granted to GITHUB_TOKEN",
        ),
    };

    /// GitHub Pages.
    ///
    /// Three permissions, and the third was a surprise: `PUT /repos/{o}/{r}/pages`
    /// answers `X-Accepted-GitHub-Permissions: pages=write,administration=write`,
    /// and a comma in that header means *and*. GitHub's published table lists
    /// the Pages writes under both permissions without saying whether it means
    /// both or either; the header says both. Confirmed by
    /// `live_declared_permissions_match_what_github_accepts`.
    ///
    /// `github_token_capable` stays `true` even so, because the Actions token
    /// is a different permission system from a fine-grained PAT: `pages` is a
    /// key in the workflow `permissions:` block and `actions/configure-pages`
    /// enables a site with `pages: write` alone. Claiming otherwise would make
    /// `sync` refuse a workflow that works today, and a false refusal cannot be
    /// overruled — there is no flag for it. Being wrong in that direction is
    /// the expensive one.
    pub const PAGES: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Pages", Access::Write),
            FineGrained::documented("Administration", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: true,
        github_token_note: None,
    };

    /// GitHub Actions general settings.
    ///
    /// The older endpoints under `/repos/{o}/{r}/actions/permissions` are
    /// documented as `Administration: write`, and the classic scope is `repo`.
    ///
    /// The endpoints GitHub added in 2025 — artifact and log retention, fork PR
    /// contributor approval, private-repository fork PR workflows — document
    /// something else: "the `repo` scope or the *Actions policies* fine-grained
    /// permission". *Actions policies* has no entry in the published
    /// fine-grained permissions table, and it is not the same thing as the
    /// `Actions` permission [`VARIABLES`](Self::VARIABLES) already names. Rather
    /// than pick whichever of the two we would rather be true, it is declared
    /// [`unverified`](FineGrained::unverified): `doctor` then says *unknown*,
    /// which is what we actually know. ADR-020 — these categories do not nest —
    /// is precisely why the guess would be unsafe.
    ///
    /// `live_declared_permissions_match_what_github_accepts` reads
    /// `X-Accepted-GitHub-Permissions` off each of the seven endpoints and will
    /// settle it.
    pub const ACTIONS: Requirement = Requirement {
        fine_grained: &[
            FineGrained::documented("Metadata", Access::Read),
            FineGrained::documented("Administration", Access::Write),
            FineGrained::unverified("Actions policies", Access::Write),
        ],
        classic: &["repo"],
        github_token_capable: false,
        // Byte-identical to ADMINISTRATION's, which the docs generator groups on.
        github_token_note: Some(
            "requires Administration: write, which cannot be granted to GITHUB_TOKEN",
        ),
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
            &Requirement::ENVIRONMENTS,
            &Requirement::PAGES,
            &Requirement::ACTIONS,
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

    /// Assert a requirement carries exactly this permission at this level.
    fn demands(requirement: &Requirement, name: &str, access: Access) -> bool {
        requirement
            .fine_grained
            .iter()
            .any(|p| p.name == name && p.access == access)
    }

    #[test]
    fn environment_scoped_variables_need_the_environments_permission() {
        // The scope decides the permission, not the payload:
        // `actions/variables` is Variables, `environments/{name}/variables` is
        // Environments. Declaring only the former was wrong (ADR-020).
        assert!(demands(&Requirement::VARIABLES, "Variables", Access::Write));
        assert!(demands(
            &Requirement::VARIABLES,
            "Environments",
            Access::Write
        ));
    }

    #[test]
    fn listing_environments_needs_actions_read() {
        // `GET /repos/{o}/{r}/environments` sits under Actions, and both
        // resources that manage environments start from it. Administration:
        // write does not include it — these categories do not nest.
        assert!(demands(&Requirement::ENVIRONMENTS, "Actions", Access::Read));
        assert!(demands(&Requirement::VARIABLES, "Actions", Access::Read));
    }

    #[test]
    fn environments_cannot_be_written_without_administration() {
        assert!(demands(
            &Requirement::ENVIRONMENTS,
            "Administration",
            Access::Write
        ));
    }

    #[test]
    fn environments_repeats_the_administration_note_verbatim() {
        // The docs generator groups resources into one sentence by exact string
        // equality, so a paraphrase here silently splits the paragraph in two.
        assert_eq!(
            Requirement::ENVIRONMENTS.github_token_note,
            Requirement::ADMINISTRATION.github_token_note
        );
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

    #[test]
    fn every_mapping_is_settled_against_github() {
        // Every one of these was checked against
        // `X-Accepted-GitHub-Permissions` by
        // `live_declared_permissions_match_what_github_accepts`, which is why
        // nothing is `Unverified` any more and the docs carry no footnote. A
        // new mapping that cannot be confirmed belongs here as an exception,
        // with a reason.
        //
        // `ACTIONS` is that exception. GitHub documents the 2025 Actions policy
        // endpoints against an "Actions policies" permission that appears in no
        // published table, and guessing which existing permission it is would be
        // a claim we cannot support (ADR-020). It is asserted below instead.
        for requirement in [
            &Requirement::ADMINISTRATION,
            &Requirement::ISSUES,
            &Requirement::VARIABLES,
            &Requirement::ENVIRONMENTS,
            &Requirement::PAGES,
            &Requirement::CONTENTS,
        ] {
            assert!(
                !requirement.has_unverified(),
                "{} should be settled against the reference",
                requirement.fine_grained_summary()
            );
        }
    }

    #[test]
    fn the_actions_policy_permission_is_declared_unverified() {
        // Not an oversight: "Actions policies" is the name GitHub's own endpoint
        // documentation uses, and it is in none of the published permission
        // tables. `doctor` says "unknown" for it, which is the truth.
        assert!(Requirement::ACTIONS.has_unverified());
        assert!(
            Requirement::ACTIONS
                .fine_grained
                .iter()
                .any(|permission| permission.name == "Actions policies"
                    && permission.confidence == Confidence::Unverified)
        );
    }

    #[test]
    fn pages_writes_also_need_administration() {
        // `pages=write,administration=write`, and a comma in that header means
        // "and". The published table would not have told us.
        assert!(Requirement::PAGES.fine_grained.iter().any(|permission| {
            permission.name == "Administration" && permission.access == Access::Write
        }));
    }

    #[test]
    fn pages_stays_reachable_with_the_actions_token() {
        // Even though it needs Administration: write as a fine-grained PAT. The
        // Actions token is a different permission system, `pages` is a key in
        // the workflow `permissions:` block, and a false refusal cannot be
        // overruled.
        assert!(requirement(&Requirement::PAGES).github_token_capable);
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
