//! Test harness.
//!
//! Because the transport shells out to `gh` (ADR-003), the entire write path can
//! be exercised by putting a stub `gh` on `PATH`. No HTTP mocking, no network,
//! and — most usefully — the stub records every invocation, so tests can assert
//! on *which requests were made, in which order*, not merely on the final output.
//!
//! Each test gets a fresh temporary directory containing:
//!
//! ```text
//! bin/gh              the stub, first on PATH
//! fixtures/           canned responses, one file per request
//! requests.log        one line per invocation
//! .github/settings.yml
//! ```

#![allow(dead_code)]

pub mod live;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// A prepared sandbox.
pub struct Sandbox {
    dir: TempDir,
    fixtures: BTreeMap<String, Fixture>,
}

/// A canned response.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// HTTP status to report.
    pub status: u16,
    /// Response body.
    pub body: String,
    /// Extra headers, e.g. `x-oauth-scopes`.
    pub headers: Vec<(String, String)>,
}

impl Fixture {
    /// A `200` with a JSON body.
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// A `201`, as returned by creations.
    pub fn created(body: impl Into<String>) -> Self {
        Self {
            status: 201,
            ..Self::ok(body)
        }
    }

    /// A `204`, as returned by deletions.
    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: String::new(),
            headers: Vec::new(),
        }
    }

    /// An error response with GitHub's usual body shape.
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            body: format!(r#"{{"message":{message:?}}}"#),
            headers: Vec::new(),
        }
    }

    /// Attach a response header.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// The key a request is looked up under: `METHOD endpoint`.
pub fn key(method: &str, endpoint: &str) -> String {
    format!("{method} {}", endpoint.trim_start_matches('/'))
}

impl Sandbox {
    /// Start building a sandbox.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temporary directory");
        std::fs::create_dir_all(dir.path().join("bin")).expect("bin");
        std::fs::create_dir_all(dir.path().join("fixtures")).expect("fixtures");
        std::fs::create_dir_all(dir.path().join(".github")).expect(".github");

        Self {
            dir,
            fixtures: BTreeMap::new(),
        }
    }

    /// Write the configuration file.
    pub fn config(self, contents: &str) -> Self {
        std::fs::write(self.dir.path().join(".github/settings.yml"), contents)
            .expect("write config");
        self
    }

    /// Register a canned response.
    pub fn respond(mut self, method: &str, endpoint: &str, fixture: Fixture) -> Self {
        self.fixtures.insert(key(method, endpoint), fixture);
        self
    }

    /// Register a `200` JSON response.
    pub fn get(self, endpoint: &str, body: &str) -> Self {
        self.respond("GET", endpoint, Fixture::ok(body))
    }

    /// Register the responses a repository read needs.
    pub fn repository(self, body: &str) -> Self {
        self.get("repos/o/r", body)
    }

    /// Set the scopes the stub's `gh auth status` reports.
    ///
    /// Mirrors real `gh`, which reports scopes inline for classic tokens and
    /// omits the field entirely for fine-grained and App tokens.
    pub fn scopes(self, scopes: &str) -> Self {
        std::fs::write(self.dir.path().join("scopes"), scopes).expect("write scopes");
        self
    }

    /// Set the token the stub's `gh auth token` reports.
    ///
    /// `doctor` classifies credentials by prefix, so this is how the tests cover
    /// classic, fine-grained and Actions tokens.
    pub fn token(self, token: &str) -> Self {
        std::fs::write(self.dir.path().join("token"), format!("{token}\n")).expect("write token");
        self
    }

    /// Accept any write to an endpoint, returning `200`.
    pub fn accept(self, method: &str, endpoint: &str) -> Self {
        self.respond(method, endpoint, Fixture::ok("{}"))
    }

    /// The sandbox root.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Materialise the stub and return a runner.
    pub fn build(self) -> Runner {
        write_stub(self.dir.path(), &self.fixtures);
        Runner { dir: self.dir }
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the binary against a prepared sandbox.
pub struct Runner {
    dir: TempDir,
}

impl Runner {
    /// The sandbox root.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Run `gh-settings` with the given arguments.
    pub fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }

    /// Run `gh-settings` with extra environment variables.
    pub fn run_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_gh-settings"));

        let path = format!(
            "{}:{}",
            self.dir.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );

        command
            .args(args)
            .current_dir(self.dir.path())
            .env("PATH", path)
            .env("GH_STUB_DIR", self.dir.path())
            // Determinism: no colour, no hyperlinks, no terminal detection.
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env_remove("CLICOLOR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("GH_SETTINGS_CONFIG")
            .env_remove("RUST_LOG")
            // `doctor` classifies tokens differently inside Actions, and the
            // suite must not behave differently depending on where it runs.
            .env_remove("GITHUB_ACTIONS");

        for (name, value) in env {
            command.env(name, value);
        }

        let output = command.output().expect("run gh-settings");

        Output {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            requests: self.requests(),
        }
    }

    /// Every request the stub received, in order.
    pub fn requests(&self) -> Vec<String> {
        let log = self.dir.path().join("requests.log");
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

/// The result of a run.
#[derive(Debug)]
pub struct Output {
    /// Process exit code.
    pub status: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Requests the stub received, in order.
    pub requests: Vec<String>,
}

impl Output {
    /// Assert the exit code.
    #[track_caller]
    pub fn expect_status(&self, expected: i32) -> &Self {
        assert_eq!(
            self.status, expected,
            "unexpected exit code\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }

    /// The write requests only, which is what most apply-path assertions care
    /// about.
    pub fn writes(&self) -> Vec<&str> {
        self.requests
            .iter()
            .map(String::as_str)
            .filter(|request| !request.starts_with("GET "))
            .collect()
    }
}

/// Write the stub `gh` and its fixture files.
fn write_stub(root: &Path, fixtures: &BTreeMap<String, Fixture>) {
    let fixtures_dir = root.join("fixtures");

    for (key, fixture) in fixtures {
        let filename = key.replace(['/', ' '], "_");

        // Three files rather than one, so the stub never has to parse an HTTP
        // response. It used to strip the header block with
        // `sed '1,/^\r\?$/d'`, which is GNU syntax: BSD sed on macOS matched
        // nothing and silently deleted the whole body, so every paginated read
        // decoded as null. Splitting the files removes the parsing entirely,
        // and with it the class of bug.
        let mut head = format!("HTTP/2.0 {} X\r\n", fixture.status);
        for (name, value) in &fixture.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");

        std::fs::write(
            fixtures_dir.join(format!("{filename}.http")),
            format!("{head}{}", fixture.body),
        )
        .expect("write fixture");
        std::fs::write(fixtures_dir.join(format!("{filename}.body")), &fixture.body)
            .expect("write fixture body");
        std::fs::write(
            fixtures_dir.join(format!("{filename}.status")),
            fixture.status.to_string(),
        )
        .expect("write fixture status");
    }

    let stub = r#"#!/usr/bin/env bash
# Stub `gh`, used by the integration suite. Records every invocation and replays
# canned responses. Deliberately dumb: any logic here is logic the real tests
# are not exercising.
set -uo pipefail

root="${GH_STUB_DIR:?GH_STUB_DIR must be set}"
log="$root/requests.log"

case "${1:-}" in
  --version)
    echo "gh version 2.62.0 (2024-01-01)"
    echo "https://github.com/cli/cli/releases/tag/v2.62.0"
    exit 0
    ;;
  auth)
    case "${2:-}" in
      status)
        if [[ -f "$root/scopes" ]]; then
          printf '{"hosts":{"github.com":[{"host":"github.com","login":"tester","active":true,"scopes":"%s"}]}}\n' "$(cat "$root/scopes")"
        else
          echo '{"hosts":{"github.com":[{"host":"github.com","login":"tester","active":true}]}}'
        fi
        exit 0
        ;;
      token)
        if [[ -f "$root/token" ]]; then
          cat "$root/token"
        else
          echo "ghp_stubtoken"
        fi
        exit 0
        ;;
    esac
    exit 1
    ;;
  api) ;;
  *)
    echo "stub gh: unsupported command: $*" >&2
    exit 1
    ;;
esac

shift
method="GET"
endpoint=""
paginate=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --method|-X) method="$2"; shift 2 ;;
    --hostname|--header|-H) shift 2 ;;
    --input) shift 2 ;;
    --paginate) paginate=1; shift ;;
    --include|--slurp|-i) shift ;;
    -*) shift ;;
    *) endpoint="$1"; shift ;;
  esac
done

body=""
if [[ ! -t 0 ]]; then
  body="$(cat)"
fi

printf '%s %s %s\n' "$method" "$endpoint" "$body" >> "$log"

name="$(printf '%s_%s' "$method" "$endpoint" | tr '/ ' '__')"
file="$root/fixtures/$name.http"

# Real `gh` emits response headers only with --include, never with --paginate.
# The two forms are pre-rendered as separate files, so this stub stays free of
# text processing and therefore of GNU/BSD tool differences.
if [[ -f "$file" ]]; then
  if [[ "$paginate" == "1" ]]; then
    cat "$root/fixtures/$name.body"
  else
    cat "$file"
  fi

  status="$(cat "$root/fixtures/$name.status")"
  if [[ "$status" -ge 400 ]]; then
    exit 1
  fi
  exit 0
fi

# Unregistered reads answer empty rather than failing, so a test only has to
# declare the fixtures it actually cares about. Unregistered writes are an
# error, so an unexpected mutation cannot pass silently.
if [[ "$method" == "GET" ]]; then
  if [[ "$paginate" == "1" ]]; then
    printf '[]'
  else
    printf 'HTTP/2.0 200 OK\r\n\r\n[]'
  fi
  exit 0
fi

printf 'HTTP/2.0 500 Internal Server Error\r\n\r\n{"message":"stub gh: no fixture for %s %s"}' "$method" "$endpoint"
exit 1
"#;

    let stub_path = root.join("bin/gh");
    std::fs::write(&stub_path, stub).expect("write stub");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&stub_path)
            .expect("stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub_path, permissions).expect("chmod stub");
    }
}

/// A repository payload with sensible defaults, for tests that do not care.
pub fn default_repository() -> String {
    serde_json::json!({
        "description": "",
        "homepage": "",
        "private": false,
        "has_issues": true,
        "has_wiki": true,
        "has_projects": true,
        "has_discussions": false,
        "is_template": false,
        "allow_merge_commit": true,
        "allow_squash_merge": true,
        "allow_rebase_merge": true,
        "allow_auto_merge": false,
        "allow_update_branch": false,
        "delete_branch_on_merge": false,
        "default_branch": "main",
        "archived": false,
        "permissions": {"admin": true}
    })
    .to_string()
}

/// A repository payload with specific string fields overridden.
pub fn repository_with(overrides: &[(&str, &str)]) -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(&default_repository()).expect("valid default");
    for (key, replacement) in overrides {
        value[*key] = serde_json::Value::String((*replacement).to_string());
    }
    value.to_string()
}

/// Filters that keep snapshots stable across machines and runs.
///
/// The configuration path must be matched without assuming where the operating
/// system puts temporary directories. Anchoring on `/tmp` worked on Linux,
/// passed on Windows through the separate drive-letter pattern, and silently
/// failed on macOS, where `TMPDIR` lives under `/var/folders/...` — so every
/// snapshot embedded a machine-specific path. Matching on the trailing
/// `.github/settings.yml` instead is true on all three.
pub fn filters() -> Vec<(&'static str, &'static str)> {
    vec![
        // Anchored on a path root, and excluding `]`, so the match cannot run
        // backwards into miette's `,-[` frame or forwards past the filename.
        (
            r"(?:[A-Za-z]:)?[/\\][^\s\]]*\.github[/\\]settings\.ya?ml",
            "[CONFIG]",
        ),
        (r"\d+(\.\d+)?(ms|s)\b", "[DURATION]"),
        // Tracing writes an RFC 3339 timestamp to stderr, so any snapshot of
        // stderr is otherwise different on every run.
        (r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z", "[TIMESTAMP]"),
    ]
}

/// Path helper for tests that need to inspect written files.
pub fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(PathBuf::from(root).join(relative)).unwrap_or_default()
}

/// Snapshot command output with the stabilising filters already applied.
///
/// Shared rather than redefined per suite because forgetting [`filters`] does
/// not fail loudly: the snapshot simply captures a machine-specific temporary
/// path, and only breaks on someone else's machine.
#[macro_export]
macro_rules! assert_cli_snapshot {
    ($output:expr) => {
        insta::with_settings!({ filters => $crate::common::filters() }, {
            insta::assert_snapshot!($output);
        });
    };
    ($name:expr, $output:expr) => {
        insta::with_settings!({ filters => $crate::common::filters() }, {
            insta::assert_snapshot!($name, $output);
        });
    };
}

#[cfg(test)]
mod harness_tests {
    use super::*;

    /// The stub must not depend on GNU-flavoured tools.
    ///
    /// It once stripped HTTP headers with `sed '1,/^\r\?$/d'`. That is GNU
    /// syntax: BSD sed on macOS matched nothing, silently deleted the entire
    /// body, and every paginated read decoded as `null` — so all 35 sandbox
    /// tests failed on macOS while passing on Linux and Windows.
    ///
    /// The response is now pre-rendered into separate files, so the stub does
    /// no text processing at all. This test keeps it that way.
    #[test]
    fn the_stub_does_no_text_processing() {
        let stub = include_str!("mod.rs");
        let start = stub.find("let stub = r#\"").expect("stub source");
        let body = &stub[start..];
        let end = body.find("\"#;").expect("stub end");
        let script = &body[..end];

        // Tokenise rather than substring-match: "used" contains "sed".
        let tokens: Vec<&str> = script
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            .collect();

        for tool in ["sed", "awk", "head", "tail", "cut", "expr"] {
            assert!(
                !tokens.contains(&tool),
                "the stub invokes `{tool}`; text processing differs between GNU and BSD, \
                 and the difference only shows up on macOS CI"
            );
        }
    }

    /// The configuration path must be redacted wherever the OS puts temp files.
    ///
    /// The filter was originally anchored on `/tmp`. That held on Linux, and
    /// Windows was covered by a separate drive-letter pattern, but macOS puts
    /// `TMPDIR` under `/var/folders/...` — so twelve snapshots embedded a
    /// machine-specific path and only ever failed on macOS CI.
    #[test]
    fn the_config_path_is_redacted_on_every_platform() {
        let filter = &filters()[0];
        let regex = regex_lite_matches(filter.0);

        for path in [
            // Linux
            "/tmp/.tmpAbC123/.github/settings.yml",
            // macOS: TMPDIR lives under /var/folders
            "/var/folders/qx/8k2p1r9d5zq0000gn/T/.tmpAbC123/.github/settings.yml",
            // Windows
            r"C:\Users\runner\AppData\Local\Temp\.tmpAbC\.github\settings.yml",
            // The .yaml spelling is accepted too
            "/tmp/.tmpAbC123/.github/settings.yaml",
        ] {
            assert!(
                regex.is_match(path),
                "the filter would leave `{path}` in a snapshot"
            );
        }

        // It must not run backwards into miette's `,-[` frame.
        let framed = "   ,-[/tmp/.tmpAbC/.github/settings.yml:3:3]";
        let redacted = regex.replace_all(framed, "[CONFIG]");
        assert_eq!(redacted, "   ,-[[CONFIG]:3:3]");
    }

    fn regex_lite_matches(pattern: &str) -> regex_lite::Regex {
        regex_lite::Regex::new(pattern).expect("the snapshot filter must be a valid regex")
    }

    #[test]
    fn every_fixture_is_written_in_all_three_forms() {
        // The stub picks between them by flag rather than by parsing, so a
        // missing form is a silent empty response rather than an error.
        let runner = Sandbox::new().get("repos/o/r/labels", "[]").build();
        let fixtures = runner.path().join("fixtures");

        for suffix in ["http", "body", "status"] {
            let path = fixtures.join(format!("GET_repos_o_r_labels.{suffix}"));
            assert!(path.is_file(), "missing {}", path.display());
        }
    }
}
