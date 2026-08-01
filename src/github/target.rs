//! Target repository resolution.
//!
//! A [`Target`] is the `owner/repo` pair every operation acts upon. It is either
//! given explicitly with `-R owner/repo` or inferred from the current git
//! repository, mirroring how the GitHub CLI itself behaves.

use std::fmt;
use std::str::FromStr;

use crate::github::GitHubError;

/// The repository an operation acts upon.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// Repository owner (user or organization) login.
    pub owner: String,
    /// Repository name, without the owner prefix.
    pub repo: String,
}

impl Target {
    /// Build a target from its two components.
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    /// The `owner/repo` path fragment used to build REST endpoints.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// Build a repository-scoped REST endpoint.
    ///
    /// ```
    /// # use gh_settings::github::Target;
    /// let target = Target::new("noirbizarre", "gh-settings");
    /// assert_eq!(target.endpoint("labels"), "repos/noirbizarre/gh-settings/labels");
    /// assert_eq!(target.endpoint(""), "repos/noirbizarre/gh-settings");
    /// ```
    pub fn endpoint(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            format!("repos/{}/{}", self.owner, self.repo)
        } else {
            format!("repos/{}/{}/{}", self.owner, self.repo, path)
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

impl FromStr for Target {
    type Err = GitHubError;

    /// Parse an `owner/repo` string.
    ///
    /// Full URLs are accepted as a convenience because users copy them out of the
    /// browser, but anything else is rejected rather than guessed at.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cleaned = s
            .trim()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .trim_end_matches('/');

        // Accept `https://github.com/owner/repo` and `git@github.com:owner/repo`.
        let cleaned = match cleaned.rsplit_once("://") {
            Some((_, rest)) => rest.split_once('/').map_or(rest, |(_, p)| p),
            None => match cleaned.split_once('@') {
                Some((_, rest)) if rest.contains(':') => {
                    rest.split_once(':').map_or(rest, |(_, p)| p)
                }
                _ => cleaned,
            },
        };

        let mut parts = cleaned.split('/').filter(|p| !p.is_empty());
        let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(GitHubError::InvalidTarget(s.to_string()));
        };

        if owner.is_empty() || repo.is_empty() {
            return Err(GitHubError::InvalidTarget(s.to_string()));
        }

        Ok(Self::new(owner, repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("noirbizarre/gh-settings")]
    #[case("  noirbizarre/gh-settings  ")]
    #[case("noirbizarre/gh-settings/")]
    #[case("https://github.com/noirbizarre/gh-settings")]
    #[case("https://github.com/noirbizarre/gh-settings.git")]
    #[case("git@github.com:noirbizarre/gh-settings.git")]
    fn parses_every_shape_users_paste(#[case] input: &str) {
        let target: Target = input.parse().expect("should parse");
        assert_eq!(target, Target::new("noirbizarre", "gh-settings"));
    }

    #[rstest]
    #[case("")]
    #[case("gh-settings")]
    #[case("a/b/c")]
    #[case("/")]
    #[case("owner/")]
    fn rejects_rather_than_guesses(#[case] input: &str) {
        assert!(
            input.parse::<Target>().is_err(),
            "{input:?} should be rejected"
        );
    }

    #[test]
    fn builds_endpoints() {
        let target = Target::new("o", "r");
        assert_eq!(target.endpoint("/topics"), "repos/o/r/topics");
        assert_eq!(target.slug(), "o/r");
    }
}
