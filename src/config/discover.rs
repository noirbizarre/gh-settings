//! Locating the configuration file and the target repository.
//!
//! Both follow the GitHub CLI's conventions so the extension feels native: the
//! repository is inferred from the git remote unless `-R` says otherwise, and the
//! configuration lives at `.github/settings.yml` unless `--config` says otherwise.

use std::path::{Path, PathBuf};

use crate::github::{GitHubError, Target};

use super::ConfigError;

/// Where a configuration file came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// An explicit `--config` path.
    Explicit(PathBuf),
    /// The `GH_SETTINGS_CONFIG` environment variable.
    Environment(PathBuf),
    /// The conventional location, discovered by searching upwards.
    Discovered(PathBuf),
}

impl ConfigSource {
    /// The path itself.
    pub fn path(&self) -> &Path {
        match self {
            Self::Explicit(path) | Self::Environment(path) | Self::Discovered(path) => path,
        }
    }
}

/// Filenames we accept, in order of preference.
///
/// `.yml` first because that is what `safe-settings` uses and therefore what
/// most existing repositories have.
pub const CANDIDATES: &[&str] = &[".github/settings.yml", ".github/settings.yaml"];

/// Locate the configuration file.
///
/// Searches upwards from `start` so the command works from anywhere inside a
/// working tree, exactly as git and `gh` do.
pub fn discover(start: &Path, explicit: Option<&Path>) -> Result<ConfigSource, ConfigError> {
    if let Some(path) = explicit {
        return if path.is_file() {
            Ok(ConfigSource::Explicit(path.to_path_buf()))
        } else {
            Err(ConfigError::Unreadable {
                path: path.display().to_string(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
            })
        };
    }

    if let Ok(value) = std::env::var("GH_SETTINGS_CONFIG") {
        let path = PathBuf::from(value);
        return if path.is_file() {
            Ok(ConfigSource::Environment(path))
        } else {
            Err(ConfigError::Unreadable {
                path: path.display().to_string(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
            })
        };
    }

    for directory in start.ancestors() {
        for candidate in CANDIDATES {
            let path = directory.join(candidate);
            if path.is_file() {
                return Ok(ConfigSource::Discovered(path));
            }
        }
        // Stop at the repository root: walking past it would pick up an unrelated
        // parent project's configuration, which is worse than finding nothing.
        if directory.join(".git").exists() {
            break;
        }
    }

    Err(ConfigError::NotFound)
}

/// The default path to write when creating a configuration file.
pub fn default_path(root: &Path) -> PathBuf {
    root.join(CANDIDATES[0])
}

/// Infer the target repository from a git remote.
///
/// Mirrors `gh`: prefer `upstream` when it exists, since a fork's `origin` points
/// at the user's copy rather than the project being configured.
pub fn infer_target(remotes: &[(String, String)]) -> Result<Target, GitHubError> {
    for name in ["upstream", "origin"] {
        if let Some((_, url)) = remotes.iter().find(|(remote, _)| remote == name)
            && let Ok(target) = url.parse::<Target>()
        {
            return Ok(target);
        }
    }

    // Fall back to any remote that parses, so unconventional setups still work.
    remotes
        .iter()
        .find_map(|(_, url)| url.parse::<Target>().ok())
        .ok_or(GitHubError::NoTarget)
}

/// Read the git remotes of a working tree.
pub async fn git_remotes(directory: &Path) -> Vec<(String, String)> {
    let Ok(output) = tokio::process::Command::new("git")
        .args(["remote", "-v"])
        .current_dir(directory)
        .output()
        .await
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let url = parts.next()?;
            Some((name.to_string(), url.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn remotes(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
            .collect()
    }

    #[test]
    fn prefers_upstream_over_origin() {
        // On a fork, `origin` is the user's copy; `upstream` is the project one
        // actually intends to configure.
        let target = infer_target(&remotes(&[
            ("origin", "git@github.com:me/fork.git"),
            ("upstream", "git@github.com:project/repo.git"),
        ]))
        .unwrap();
        assert_eq!(target, Target::new("project", "repo"));
    }

    #[test]
    fn falls_back_to_origin() {
        let target = infer_target(&remotes(&[("origin", "git@github.com:me/repo.git")])).unwrap();
        assert_eq!(target, Target::new("me", "repo"));
    }

    #[test]
    fn falls_back_to_any_parsable_remote() {
        let target = infer_target(&remotes(&[("fork", "https://github.com/me/repo")])).unwrap();
        assert_eq!(target, Target::new("me", "repo"));
    }

    #[test]
    fn fails_when_no_remote_parses() {
        assert!(infer_target(&remotes(&[("origin", "not-a-url")])).is_err());
        assert!(infer_target(&[]).is_err());
    }

    #[test]
    fn finds_the_conventional_location() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/settings.yml"), "").unwrap();

        let source = discover(dir.path(), None).unwrap();
        assert!(matches!(source, ConfigSource::Discovered(_)));
    }

    #[test]
    fn searches_upwards() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/settings.yml"), "").unwrap();
        let nested = dir.path().join("src/deep/nested");
        std::fs::create_dir_all(&nested).unwrap();

        let source = discover(&nested, None).unwrap();
        assert_eq!(source.path(), dir.path().join(".github/settings.yml"));
    }

    #[test]
    fn accepts_the_yaml_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/settings.yaml"), "").unwrap();
        assert!(discover(dir.path(), None).is_ok());
    }

    #[test]
    fn prefers_yml_over_yaml() {
        // `.yml` is what safe-settings uses, so it is what existing repositories
        // have; picking it keeps migrations predictable.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/settings.yml"), "").unwrap();
        std::fs::write(dir.path().join(".github/settings.yaml"), "").unwrap();
        assert!(
            discover(dir.path(), None)
                .unwrap()
                .path()
                .to_string_lossy()
                .ends_with(".yml")
        );
    }

    #[test]
    fn reports_when_nothing_is_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            discover(dir.path(), None),
            Err(ConfigError::NotFound)
        ));
    }

    #[test]
    fn an_explicit_path_that_does_not_exist_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.yml");
        assert!(matches!(
            discover(dir.path(), Some(&missing)),
            Err(ConfigError::Unreadable { .. })
        ));
    }

    #[test]
    fn an_explicit_path_wins_over_discovery() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/settings.yml"), "").unwrap();
        let explicit = dir.path().join("custom.yml");
        std::fs::write(&explicit, "").unwrap();

        let source = discover(dir.path(), Some(&explicit)).unwrap();
        assert_eq!(source, ConfigSource::Explicit(explicit));
    }
}
