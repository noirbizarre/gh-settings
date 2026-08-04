//! Authentication introspection.
//!
//! This backs `gh settings doctor` and the pre-flight check in `sync`. The guiding
//! rule (plan §6b) is: **never fabricate certainty**. Classic tokens advertise
//! their scopes in a response header and can be reported exactly; fine-grained and
//! App tokens cannot, and are reported as `Unknown` rather than guessed at.

use serde::Deserialize;

use crate::github::client::{GitHubClient, Request};
use crate::github::{GitHubError, Result};

/// What kind of credential we are running with.
///
/// This matters because it determines what is *possible*, not merely what is
/// currently permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// The OAuth token minted by `gh auth login`.
    OAuth,
    /// A classic personal access token (`ghp_`).
    ClassicPat,
    /// A fine-grained personal access token (`github_pat_`).
    FineGrainedPat,
    /// A GitHub App installation token (`ghs_`) outside of Actions.
    AppInstallation,
    /// The automatic `GITHUB_TOKEN` of a GitHub Actions run.
    ///
    /// Structurally incapable of managing repository settings: the workflow
    /// `permissions:` block has no `administration` key, so it cannot be granted.
    ActionsGitHubToken,
    /// We could not tell.
    Unknown,
}

impl TokenKind {
    /// Human label used in `doctor` output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::OAuth => "gh OAuth token",
            Self::ClassicPat => "classic personal access token",
            Self::FineGrainedPat => "fine-grained personal access token",
            Self::AppInstallation => "GitHub App installation token",
            Self::ActionsGitHubToken => "Actions GITHUB_TOKEN",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this credential can ever hold `Administration: write`.
    ///
    /// Only [`Self::ActionsGitHubToken`] is a definitive no, and that is the whole
    /// point of reporting the kind at all.
    pub fn can_hold_administration(&self) -> bool {
        !matches!(self, Self::ActionsGitHubToken)
    }

    /// Classify a token from its prefix and the surrounding environment.
    ///
    /// `ghs_` is ambiguous between an App installation token and the Actions
    /// token, so the environment breaks the tie.
    pub fn detect(token: &str, in_actions: bool) -> Self {
        match token {
            _ if token.starts_with("github_pat_") => Self::FineGrainedPat,
            _ if token.starts_with("ghp_") => Self::ClassicPat,
            _ if token.starts_with("ghs_") && in_actions => Self::ActionsGitHubToken,
            _ if token.starts_with("ghs_") => Self::AppInstallation,
            _ if token.starts_with("gho_") => Self::OAuth,
            _ => Self::Unknown,
        }
    }
}

/// What we know about the scopes attached to the current credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scopes {
    /// Exact list, read from the `X-OAuth-Scopes` response header.
    Known(Vec<String>),
    /// The credential does not advertise its scopes.
    ///
    /// Fine-grained PATs and App tokens land here. We say so instead of guessing.
    Unknown,
}

impl Scopes {
    /// Whether a given classic scope is definitely granted.
    ///
    /// Returns `None` when the scopes are unknown, so callers can distinguish
    /// "definitely missing" from "cannot tell".
    pub fn grants(&self, scope: &str) -> Option<bool> {
        match self {
            Self::Known(scopes) => Some(scopes.iter().any(|granted| granted == scope)),
            Self::Unknown => None,
        }
    }
}

/// The result of introspecting the current authentication.
#[derive(Debug, Clone)]
pub struct AuthStatus {
    /// Host we are authenticated against, e.g. `github.com`.
    pub hostname: String,
    /// Login of the authenticated account, when it could be determined.
    pub account: Option<String>,
    /// Credential kind.
    pub token_kind: TokenKind,
    /// Known or unknown scopes.
    pub scopes: Scopes,
    /// Whether the token holds admin rights on the target repository.
    ///
    /// Read from `permissions.admin` on `GET /repos/{owner}/{repo}`. `None` when
    /// the repository could not be read at all.
    pub admin_on_target: Option<bool>,
}

impl AuthStatus {
    /// Whether writing repository settings is known to be impossible.
    ///
    /// Deliberately conservative: it only returns `true` when we are *certain*,
    /// so that `sync` never refuses to run on a false negative.
    pub fn administration_is_impossible(&self) -> bool {
        !self.token_kind.can_hold_administration()
    }
}

/// The subset of `gh auth status --json hosts` we consume.
///
/// The real shape is an object keyed by hostname, each holding a list of
/// accounts:
///
/// ```json
/// {"hosts": {"github.com": [{"login": "…", "active": true, "scopes": "repo, …"}]}}
/// ```
///
/// Every field is optional here so that a future `gh` adding or renaming keys
/// degrades to a partial report rather than to no report at all.
#[derive(Debug, Default, Deserialize)]
struct GhAuthStatus {
    #[serde(default)]
    hosts: std::collections::BTreeMap<String, Vec<GhAuthAccount>>,
}

#[derive(Debug, Default, Deserialize)]
struct GhAuthAccount {
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    active: bool,
    /// Comma-separated scopes. Present for classic tokens, absent otherwise.
    #[serde(default)]
    scopes: Option<String>,
}

/// What `gh auth status` told us.
#[derive(Debug, Default)]
pub struct GhAuth {
    /// Host we are authenticated against.
    pub hostname: String,
    /// Login of the active account.
    pub account: Option<String>,
    /// Scopes, when `gh` reported them.
    pub scopes: Option<Vec<String>>,
}

/// The `permissions` block of a repository payload.
#[derive(Debug, Deserialize)]
struct RepoPermissions {
    #[serde(default)]
    permissions: Option<Permissions>,
}

#[derive(Debug, Deserialize)]
struct Permissions {
    #[serde(default)]
    admin: bool,
}

/// Whether we are running inside GitHub Actions.
pub fn in_github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
}

/// Parse the comma-separated `X-OAuth-Scopes` header.
///
/// An empty header means "no scopes", which is meaningfully different from an
/// absent header ("this credential does not report scopes").
fn parse_scopes(header: &str) -> Vec<String> {
    header
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_string)
        .collect()
}

/// Classify the credential and, when possible, enumerate its scopes.
///
/// `token` is the raw token, used only for prefix classification; it is never
/// logged or stored.
pub async fn introspect(
    client: &dyn GitHubClient,
    token: Option<&str>,
    auth: &GhAuth,
) -> Result<AuthStatus> {
    let token_kind = token
        .map(|token| TokenKind::detect(token, in_github_actions()))
        .unwrap_or(TokenKind::Unknown);

    // `gh auth status` already knows the scopes for classic tokens, so prefer
    // it and save a round trip. Fall back to the `X-OAuth-Scopes` response
    // header, which is the only other place they are exposed.
    let scopes = match &auth.scopes {
        Some(scopes) => Scopes::Known(scopes.clone()),
        None => {
            let response = client.request(Request::get("")).await?;
            match response.header("x-oauth-scopes") {
                Some(header) => Scopes::Known(parse_scopes(header)),
                // Fine-grained and App tokens do not advertise scopes at all.
                None => Scopes::Unknown,
            }
        }
    };

    Ok(AuthStatus {
        hostname: auth.hostname.clone(),
        account: auth.account.clone(),
        token_kind,
        scopes,
        admin_on_target: None,
    })
}

/// Probe whether the credential has admin rights on a repository.
///
/// This is the only reliable signal for fine-grained and App tokens, which do not
/// advertise scopes. A `403`/`404` yields `None` ("cannot tell"), never `false`.
pub async fn probe_admin(
    client: &dyn GitHubClient,
    target: &crate::github::Target,
) -> Option<bool> {
    use crate::github::client::GitHubClientExt;

    match client
        .send::<RepoPermissions>(Request::get(target.endpoint("")))
        .await
    {
        Ok(repo) => repo.permissions.map(|permissions| permissions.admin),
        Err(_) => None,
    }
}

/// Read the account, hostname and scopes from `gh auth status --json hosts`.
///
/// Failures here are not fatal: `doctor` degrades to reporting what it does
/// know, which is more useful than refusing to say anything.
pub async fn gh_auth_status(program: &std::ffi::OsStr) -> Result<GhAuth> {
    let output = tokio::process::Command::new(program)
        .args(["auth", "status", "--json", "hosts"])
        .output()
        .await
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                GitHubError::GhNotFound
            } else {
                GitHubError::Spawn {
                    args: "auth status --json hosts".into(),
                    source,
                }
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let status: GhAuthStatus = serde_json::from_str(stdout.trim()).unwrap_or_default();

    // Prefer the active account, on whichever host it lives.
    let active = status
        .hosts
        .iter()
        .flat_map(|(host, accounts)| accounts.iter().map(move |account| (host, account)))
        .find(|(_, account)| account.active)
        .or_else(|| {
            status
                .hosts
                .iter()
                .flat_map(|(host, accounts)| accounts.iter().map(move |account| (host, account)))
                .next()
        });

    Ok(match active {
        Some((host, account)) => GhAuth {
            hostname: account.host.clone().unwrap_or_else(|| host.clone()),
            account: account.login.clone(),
            scopes: account.scopes.as_deref().map(parse_scopes),
        },
        None => GhAuth {
            hostname: "github.com".to_string(),
            account: None,
            scopes: None,
        },
    })
}

/// Read the token via `gh auth token`, so we can classify it by prefix.
pub async fn gh_auth_token(program: &std::ffi::OsStr) -> Option<String> {
    let output = tokio::process::Command::new(program)
        .args(["auth", "token"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Read the installed `gh` version, or `None` when `gh` cannot be run.
///
/// Takes the program the same way its neighbours do. It lived in `cli::doctor`
/// with `"gh"` hard-coded, which both broke the rule that this module is the
/// only place that spawns `gh` and meant a relocated `gh` was found by the two
/// calls beside it and not by this one.
pub async fn gh_version(program: &std::ffi::OsStr) -> Option<String> {
    let output = tokio::process::Command::new(program)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("ghp_abc", false, TokenKind::ClassicPat)]
    #[case("github_pat_abc", false, TokenKind::FineGrainedPat)]
    #[case("gho_abc", false, TokenKind::OAuth)]
    #[case("ghs_abc", false, TokenKind::AppInstallation)]
    #[case("ghs_abc", true, TokenKind::ActionsGitHubToken)]
    #[case("whatever", false, TokenKind::Unknown)]
    fn classifies_tokens(
        #[case] token: &str,
        #[case] in_actions: bool,
        #[case] expected: TokenKind,
    ) {
        assert_eq!(TokenKind::detect(token, in_actions), expected);
    }

    #[test]
    fn only_the_actions_token_is_structurally_incapable() {
        assert!(!TokenKind::ActionsGitHubToken.can_hold_administration());
        for kind in [
            TokenKind::OAuth,
            TokenKind::ClassicPat,
            TokenKind::FineGrainedPat,
            TokenKind::AppInstallation,
            TokenKind::Unknown,
        ] {
            assert!(kind.can_hold_administration(), "{kind:?}");
        }
    }

    #[test]
    fn parses_the_scope_header() {
        assert_eq!(parse_scopes("repo, read:org"), vec!["repo", "read:org"]);
        assert_eq!(parse_scopes(""), Vec::<String>::new());
        assert_eq!(parse_scopes("  repo  "), vec!["repo"]);
    }

    #[test]
    fn unknown_scopes_do_not_pretend_to_know() {
        assert_eq!(Scopes::Unknown.grants("repo"), None);
        assert_eq!(
            Scopes::Known(vec!["repo".into()]).grants("repo"),
            Some(true)
        );
        assert_eq!(
            Scopes::Known(vec!["repo".into()]).grants("admin:org"),
            Some(false)
        );
    }
}
