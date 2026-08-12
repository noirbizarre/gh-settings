//! Live test harness: the real `gh`, against a real repository.
//!
//! The stub in [`super`] asserts the *shape* of the requests we send. It cannot
//! assert that GitHub accepts them, and it cannot show what GitHub sends back.
//! Both bugs fixed in 0.1.0 lived in exactly that gap:
//!
//! * ruleset rule parameters were defaulted by the server, so every plan
//!   reported a change for ever — a permanent diff the stub could never see,
//!   because the stub replayed our own fixtures back at us;
//! * every HTTP error on a paginated endpoint was reported as an
//!   authentication failure, because a paginated response carries no status
//!   line and we assumed success.
//!
//! Both were found by running against the API by hand. This exists so nobody
//! has to remember to.
//!
//! # Safety
//!
//! These tests **mutate a real repository**, so:
//!
//! * they are `#[ignore]`d and only run when `GH_SETTINGS_TEST_REPO` is set;
//! * the repository must be free of managed configuration before they start.
//!   A suite that can eat someone's real settings is not worth having, so this
//!   is checked, not assumed;
//! * each test cleans up after itself.
//!
//! The sandbox is yours, not CI's: a repository shared by two actors does not
//! interleave, it makes the second pre-flight refuse (ADR-019). Build one, and
//! dig it out again after a crashed run, with:
//!
//! ```sh
//! mise run test:live:setup you/sandbox
//! GH_SETTINGS_TEST_REPO=you/sandbox mise run test:live
//! ```
//!
//! The repository must be **public** if rulesets are to be covered: on the free
//! plan a private repository answers `403 Upgrade to GitHub Pro` for the
//! rulesets endpoints. The pre-flight says so rather than letting the failure
//! arrive obscurely.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// The environment variable naming the repository under test.
pub const REPO_VAR: &str = "GH_SETTINGS_TEST_REPO";

/// A live run against a real repository.
pub struct Live {
    repo: String,
    dir: TempDir,
}

/// Why a live run was skipped or refused.
#[derive(Debug)]
pub enum Unavailable {
    /// The variable is unset: not an error, just not requested.
    NotRequested,
    /// The repository cannot be used, with the reason.
    Refused(String),
}

impl Live {
    /// Prepare a live run, or explain why not.
    ///
    /// Returns `Err(NotRequested)` when the variable is unset so an ordinary
    /// `cargo test` is unaffected.
    pub fn new() -> Result<Self, Unavailable> {
        let repo = std::env::var(REPO_VAR).map_err(|_| Unavailable::NotRequested)?;
        let repo = repo.trim().to_string();

        if repo.is_empty() {
            return Err(Unavailable::NotRequested);
        }

        if !repo.contains('/') || repo.matches('/').count() != 1 {
            return Err(Unavailable::Refused(format!(
                "{REPO_VAR}={repo} is not an `owner/repo` pair"
            )));
        }

        let dir = tempfile::tempdir()
            .map_err(|error| Unavailable::Refused(format!("no temporary directory: {error}")))?;
        std::fs::create_dir_all(dir.path().join(".github"))
            .map_err(|error| Unavailable::Refused(error.to_string()))?;

        let live = Self { repo, dir };
        live.preflight()?;
        Ok(live)
    }

    /// Refuse to run against a repository that already has configuration.
    ///
    /// The check is deliberately strict. A false refusal costs someone a
    /// minute; a false acceptance costs them their labels.
    fn preflight(&self) -> Result<(), Unavailable> {
        let refuse = |reason: String| Unavailable::Refused(reason);

        // Rulesets are the reason the repository must be public on a free plan.
        let rulesets = self.api(&["repos", &self.repo, "rulesets"]);
        match rulesets {
            Ok(body) if body.trim() != "[]" && !body.trim().is_empty() => {
                return Err(refuse(format!(
                    "{} already has rulesets; refusing to touch it",
                    self.repo
                )));
            }
            Err(error) if error.contains("Upgrade to GitHub Pro") => {
                return Err(refuse(format!(
                    "{} is private and rulesets need GitHub Pro. Make the sandbox \
                     public, or accept that rulesets go untested.",
                    self.repo
                )));
            }
            Err(error) => return Err(refuse(format!("could not read rulesets: {error}"))),
            Ok(_) => {}
        }

        let autolinks = self
            .api(&["repos", &self.repo, "autolinks"])
            .map_err(|error| refuse(format!("could not read autolinks: {error}")))?;
        if autolinks.trim() != "[]" {
            return Err(refuse(format!(
                "{} already has autolinks; refusing to touch it",
                self.repo
            )));
        }

        // Environments and variables carry deployment history and workflow
        // configuration, so a repository holding either is not a sandbox.
        let environments = self
            .api(&["repos", &self.repo, "environments"])
            .map_err(|error| refuse(format!("could not read environments: {error}")))?;
        let count = serde_json::from_str::<serde_json::Value>(&environments)
            .ok()
            .and_then(|page| page.get("total_count").and_then(serde_json::Value::as_u64))
            .unwrap_or(0);
        if count > 0 {
            return Err(refuse(format!(
                "{} already has environments; refusing to touch it",
                self.repo
            )));
        }

        let variables = self
            .api(&["repos", &self.repo, "actions", "variables"])
            .map_err(|error| refuse(format!("could not read variables: {error}")))?;
        let count = serde_json::from_str::<serde_json::Value>(&variables)
            .ok()
            .and_then(|page| page.get("total_count").and_then(serde_json::Value::as_u64))
            .unwrap_or(0);
        if count > 0 {
            return Err(refuse(format!(
                "{} already has Actions variables; refusing to touch it",
                self.repo
            )));
        }

        // Labels are the one resource with defaults, so a repository is only
        // "clean" if every label it has is one GitHub created.
        let labels = self
            .api(&["repos", &self.repo, "labels"])
            .map_err(|error| refuse(format!("could not read labels: {error}")))?;
        let names: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(&labels)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|label| {
                label
                    .get("name")
                    .and_then(|name| name.as_str())
                    .map(str::to_string)
            })
            .collect();

        const DEFAULTS: &[&str] = &[
            "bug",
            "documentation",
            "duplicate",
            "enhancement",
            "good first issue",
            "help wanted",
            "invalid",
            "question",
            "wontfix",
        ];
        let unexpected: Vec<&String> = names
            .iter()
            .filter(|name| !DEFAULTS.contains(&name.as_str()))
            .collect();
        if !unexpected.is_empty() {
            return Err(refuse(format!(
                "{} has labels that are not GitHub defaults ({unexpected:?}); \
                 refusing to touch it",
                self.repo
            )));
        }

        Ok(())
    }

    /// The repository under test.
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The working directory holding `.github/settings.yml`.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write the configuration file.
    pub fn config(&self, contents: &str) -> &Self {
        std::fs::write(self.dir.path().join(".github/settings.yml"), contents)
            .expect("write config");
        self
    }

    /// Run the binary against the real repository.
    pub fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gh-settings"));
        command
            .args(args)
            .args(["-R", &self.repo])
            .current_dir(self.dir.path())
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env_remove("GH_SETTINGS_CONFIG")
            .env_remove("RUST_LOG");

        let output = command.output().expect("run gh-settings");
        Output {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Call `gh api` directly, for setup and teardown.
    fn api(&self, path: &[&str]) -> Result<String, String> {
        let output = Command::new("gh")
            .arg("api")
            .arg(path.join("/"))
            .output()
            .map_err(|error| format!("could not run gh: {error}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    /// Remove everything the suite may have created.
    ///
    /// Best effort and idempotent: a failed run must leave the repository
    /// recoverable by simply running again.
    ///
    /// Note this cannot be an empty configuration file. "Absent means
    /// unmanaged" is the tool's central safety rule, so an empty file prunes
    /// *nothing* — it has to name each section explicitly with `prune: true`
    /// and no items.
    ///
    /// Only the prunable collections are here. Pages and the repository fields
    /// have no `prune`, and the pre-flight does not look at them either, so
    /// their residue is cleared by `scripts/live-sandbox.sh` instead.
    pub fn cleanup(&self) {
        let purge = "version: 1\n\
             labels:\n  prune: true\n  items: []\n\
             topics:\n  prune: true\n  items: []\n\
             autolinks:\n  prune: true\n  items: []\n\
             rulesets:\n  prune: true\n  items: []\n\
             variables:\n  prune: true\n  items: []\n\
             environments:\n  prune: true\n  items: []\n";

        self.config(purge);
        let output = self.run(&["sync", "--yes", "--prune"]);

        if output.status != 0 {
            // Not fatal — the next run's pre-flight will refuse rather than
            // silently operating on a dirty repository — but say so, because
            // the reason will not be obvious tomorrow morning.
            eprintln!(
                "cleanup of {} did not fully succeed (exit {}):\n{}\n{}",
                self.repo, output.status, output.stdout, output.stderr
            );
        }
    }
}

/// The result of a live run.
#[derive(Debug)]
pub struct Output {
    /// Process exit code.
    pub status: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

impl Output {
    /// Assert the exit code, showing both streams on failure.
    #[track_caller]
    pub fn expect_status(&self, expected: i32) -> &Self {
        assert_eq!(
            self.status, expected,
            "unexpected exit code\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }

    /// Assert the plan is empty — the idempotency contract.
    #[track_caller]
    pub fn expect_up_to_date(&self) -> &Self {
        self.expect_status(0);
        assert!(
            self.stdout.contains("up to date"),
            "expected no changes, got:\n{}",
            self.stdout
        );
        self
    }
}

/// Set up a live run, or skip the test with an explanation.
///
/// Skipping is printed rather than silent: a suite that quietly does nothing is
/// worse than one that does not exist, because it looks like coverage.
#[macro_export]
macro_rules! live_or_skip {
    () => {
        match $crate::common::live::Live::new() {
            Ok(live) => live,
            Err($crate::common::live::Unavailable::NotRequested) => {
                eprintln!(
                    "skipped: set {} to run the live suite",
                    $crate::common::live::REPO_VAR
                );
                return;
            }
            Err($crate::common::live::Unavailable::Refused(reason)) => {
                panic!("refusing to run the live suite: {reason}");
            }
        }
    };
}

/// Path helper mirroring the stub harness.
pub fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(PathBuf::from(root).join(relative)).unwrap_or_default()
}
