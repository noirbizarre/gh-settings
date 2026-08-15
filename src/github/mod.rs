//! The GitHub access layer.
//!
//! Resources never speak HTTP and never spawn processes. They depend on the
//! [`GitHubClient`] port only, which keeps them pure enough to unit test and lets
//! the transport be swapped without touching a single resource (ADR-003).
//!
//! The default adapter shells out to `gh api` ([`GhCliTransport`]). This is a
//! deliberate choice: it inherits authentication (keyring, `GH_TOKEN`, Actions,
//! GitHub Enterprise), pagination and retries from the GitHub CLI, and it makes
//! the whole layer testable by putting a stub `gh` on `PATH`.

pub mod auth;
pub mod base;
pub mod client;
pub mod gh_cli;
pub mod resolver;
pub mod target;

pub use auth::{AuthStatus, TokenKind};
pub use base::GitHubBaseLoader;
pub use client::{GitHubClient, GitHubClientExt, Request, Response};
pub use gh_cli::GhCliTransport;
pub use resolver::Resolver;
pub use target::Target;

use std::fmt;

/// Errors raised by the GitHub access layer.
///
/// Implements [`miette::Diagnostic`] so that the most common failures — a missing
/// `gh`, and above all a `403` — carry an actionable hint instead of a bare
/// status code. Permission failures are the single most frequent support issue
/// (plan §6b), so they point at `doctor`.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum GitHubError {
    /// `gh` is not installed or not on `PATH`.
    #[error("the GitHub CLI (`gh`) was not found on your PATH")]
    #[diagnostic(
        code(gh_settings::github::gh_not_found),
        help("install it from https://cli.github.com, then run `gh auth login`")
    )]
    GhNotFound,

    /// `gh` was found but could not be executed.
    #[error("failed to run `gh {args}`")]
    #[diagnostic(code(gh_settings::github::spawn))]
    Spawn {
        /// The arguments we tried to run, for reproduction.
        args: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The API answered with a non-success status.
    #[error("{method} {endpoint} failed with HTTP {status}: {message}")]
    #[diagnostic(code(gh_settings::github::api), help("{}", api_help(*.status)))]
    Api {
        /// HTTP method used.
        method: Method,
        /// Endpoint requested, without the API base URL.
        endpoint: String,
        /// HTTP status code returned by GitHub.
        status: u16,
        /// The `message` field of GitHub's error body, when present.
        message: String,
        /// The raw response body, kept for `--verbose` reporting.
        body: String,
    },

    /// `gh` failed for a reason unrelated to the HTTP exchange.
    #[error("`gh` exited with status {status}")]
    #[diagnostic(
        code(gh_settings::github::cli),
        help("check that you are authenticated with `gh auth status`")
    )]
    Cli {
        /// Process exit status.
        status: i32,
        /// Captured standard error.
        stderr: String,
    },

    /// The response body was not the JSON shape we expected.
    #[error("could not decode the response of {endpoint}")]
    #[diagnostic(
        code(gh_settings::github::decode),
        help("this usually means the GitHub API changed; please report it")
    )]
    Decode {
        /// Endpoint whose response failed to decode.
        endpoint: String,
        /// The underlying serde failure.
        #[source]
        source: serde_json::Error,
    },

    /// `-R` was given something that is not an `owner/repo` pair.
    #[error("`{0}` is not a valid repository, expected `owner/repo`")]
    #[diagnostic(code(gh_settings::github::invalid_target))]
    InvalidTarget(String),

    /// No repository could be inferred and none was given.
    ///
    /// Returned by `config::discover::infer_target`, whose only non-test caller
    /// discards it for [`ContextError::NoTarget`](crate::cli::ContextError),
    /// which is what a user actually sees. The message and help are kept
    /// identical to that one deliberately: if this variant ever does reach the
    /// surface, it must not read differently.
    #[error("could not determine which repository to act on")]
    #[diagnostic(
        code(gh_settings::github::no_target),
        help("pass `-R owner/repo`, or run from inside a git repository with a GitHub remote")
    )]
    NoTarget,

    /// A team, app or user referenced in the configuration does not exist.
    #[error("no {kind} named `{slug}` could be found")]
    #[diagnostic(
        code(gh_settings::github::unresolved_actor),
        help("check the spelling, and that your token can read the organisation")
    )]
    UnresolvedActor {
        /// The kind of actor: `team`, `app` or `user`.
        kind: &'static str,
        /// The slug that could not be resolved.
        slug: String,
    },
}

/// The hint attached to an API failure, chosen by status code.
fn api_help(status: u16) -> String {
    match status {
        401 => "your credentials were rejected; run `gh auth login`".to_string(),
        403 => "this token is not allowed to change that setting. Run `gh settings doctor` to see \
             what it can manage. Note that the Actions GITHUB_TOKEN cannot manage repository \
             settings at all — use a personal access token or a GitHub App token."
            .to_string(),
        404 => "the repository or resource does not exist, or your token cannot see it".to_string(),
        422 => {
            "GitHub rejected the request body; run with `--verbose` to see the details".to_string()
        }
        429 => "you have been rate limited; wait a moment and try again".to_string(),
        _ => "run with `--debug --debug` to see the request that failed".to_string(),
    }
}

impl GitHubError {
    /// The HTTP status, when this error came from the API.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether this error is a permission problem, which we report with the
    /// required-permission table attached rather than as a bare HTTP failure.
    pub fn is_permission_denied(&self) -> bool {
        matches!(self.status(), Some(401 | 403))
    }

    /// Whether the resource simply does not exist yet.
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }
}

/// HTTP methods we use. GitHub's settings surface needs no others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Read.
    Get,
    /// Create.
    Post,
    /// Partial update.
    Patch,
    /// Full replacement.
    Put,
    /// Removal.
    Delete,
}

impl Method {
    /// The uppercase name, as `gh api -X` expects it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }

    /// Whether this method mutates state. Used to enforce that `plan` never writes.
    pub fn is_write(&self) -> bool {
        !matches!(self, Self::Get)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Convenient result alias for this layer.
pub type Result<T> = std::result::Result<T, GitHubError>;

/// Percent-encode a value for use as a single path segment.
///
/// Several GitHub resources are addressed by a user-chosen name rather than by
/// an identifier, and those names routinely contain characters that would
/// otherwise corrupt the endpoint: label names carry spaces (`good first
/// issue`) and `/` (`area/docs`), and environment names accept both too.
///
/// Only the RFC 3986 unreserved set is left alone, `/` included — a name is one
/// segment, never a path.
pub fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_escapes_everything_outside_the_unreserved_set() {
        assert_eq!(urlencode("good first issue"), "good%20first%20issue");
        assert_eq!(urlencode("area/docs"), "area%2Fdocs");
        assert_eq!(urlencode("a-b_c.d~e9"), "a-b_c.d~e9");
    }

    #[test]
    fn only_get_is_a_read() {
        assert!(!Method::Get.is_write());
        for method in [Method::Post, Method::Patch, Method::Put, Method::Delete] {
            assert!(method.is_write(), "{method} should be a write");
        }
    }

    #[test]
    fn classifies_permission_errors() {
        let err = GitHubError::Api {
            method: Method::Patch,
            endpoint: "repos/o/r".into(),
            status: 403,
            message: "Resource not accessible by integration".into(),
            body: String::new(),
        };
        assert!(err.is_permission_denied());
        assert!(!err.is_not_found());
    }
}
