//! The `gh api` transport.
//!
//! Rather than reimplementing authentication, pagination, retries and GitHub
//! Enterprise base URLs, we shell out to the GitHub CLI (ADR-003). Since
//! `gh-settings` *is* a `gh` extension, `gh` is guaranteed to be present.
//!
//! A welcome side effect is testability: putting a stub `gh` on `PATH` gives full
//! coverage of the write paths with no network and no HTTP mocking. See
//! `tests/common/mod.rs`.

use std::ffi::OsString;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::github::client::{GitHubClient, Request, Response};
use crate::github::{GitHubError, Result};

/// Runs API calls through the `gh` executable.
#[derive(Debug, Clone)]
pub struct GhCliTransport {
    program: OsString,
    hostname: Option<String>,
    /// When true, no write request is ever executed. Defence in depth so that a
    /// bug in the engine cannot turn `plan` into `sync`.
    read_only: bool,
}

impl Default for GhCliTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl GhCliTransport {
    /// A transport invoking `gh` from `PATH`.
    pub fn new() -> Self {
        Self {
            program: OsString::from("gh"),
            hostname: None,
            read_only: false,
        }
    }

    /// Use a specific executable instead of `gh` from `PATH`.
    pub fn with_program(mut self, program: impl Into<OsString>) -> Self {
        self.program = program.into();
        self
    }

    /// Target a GitHub Enterprise Server host.
    pub fn with_hostname(mut self, hostname: Option<String>) -> Self {
        self.hostname = hostname;
        self
    }

    /// Refuse to perform any mutating request.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Translate a [`Request`] into the `gh` argument vector.
    ///
    /// Kept separate from execution so it can be asserted on directly in unit
    /// tests without spawning anything.
    fn args(&self, request: &Request) -> Vec<String> {
        let mut args = vec![
            "api".to_string(),
            "--method".to_string(),
            request.method.as_str().to_string(),
        ];

        if let Some(hostname) = &self.hostname {
            args.push("--hostname".into());
            args.push(hostname.clone());
        }

        // `--include` gives us the response headers, which `doctor` needs to read
        // `X-OAuth-Scopes`. It is incompatible with `--paginate`, so paginated
        // requests forgo headers; no caller needs both.
        if request.paginate {
            // `gh api --paginate` merges top-level JSON arrays across pages into
            // a single array, which is exactly what list endpoints need. `--slurp`
            // would instead nest one array per page, so it is deliberately not
            // used here.
            args.push("--paginate".into());
        } else {
            args.push("--include".into());
        }

        for (name, value) in &request.headers {
            args.push("--header".into());
            args.push(format!("{name}: {value}"));
        }

        if request.body.is_some() {
            // Read the body from stdin as raw JSON: `-f`/`-F` would coerce types
            // and cannot express nested objects such as ruleset rules.
            args.push("--input".into());
            args.push("-".into());
        }

        args.push(request.endpoint.clone());
        args
    }
}

#[async_trait]
impl GitHubClient for GhCliTransport {
    async fn request(&self, request: Request) -> Result<Response> {
        if self.read_only && request.method.is_write() {
            panic!(
                "attempted a {} request to {} while in read-only mode; this is a bug",
                request.method, request.endpoint
            );
        }

        let args = self.args(&request);
        let body = request
            .body
            .as_ref()
            .map(|body| serde_json::to_vec(body).unwrap_or_default());

        tracing::debug!(target: "gh_settings::github", args = ?args, "gh api");

        let mut command = Command::new(&self.program);
        command
            .args(&args)
            .stdin(if body.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                GitHubError::GhNotFound
            } else {
                GitHubError::Spawn {
                    args: args.join(" "),
                    source,
                }
            }
        })?;

        if let Some(body) = body {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take().expect("stdin was piped");
            // A broken pipe here means `gh` exited early; the real diagnosis is in
            // its stderr, so swallow the write error and let the exit code talk.
            let _ = stdin.write_all(&body).await;
            let _ = stdin.shutdown().await;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|source| GitHubError::Spawn {
                args: args.join(" "),
                source,
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        let (mut status, headers, payload) = if request.paginate {
            // `--paginate` is incompatible with `--include`, so there is no
            // status line to read. Assume success and recover the real status
            // from the error payload below if the process failed.
            (200, Vec::new(), stdout.as_str())
        } else {
            parse_included_response(&stdout)
        };

        if !output.status.success() {
            // Without `--include` there is no status line, so a failed
            // paginated request would otherwise look like a CLI failure and be
            // reported as "check that you are authenticated" — which is
            // actively misleading when the truth is a 403 or a 404. Every
            // collection resource uses `--paginate`, so this covers the most
            // common real failure there is: the wrong token.
            if request.paginate && status < 400 {
                status = parse_error_status(payload, &stderr).unwrap_or(status);
            }

            // `gh api` exits 1 on any HTTP error, so a parsed status tells us
            // whether this was an API rejection or a CLI failure (bad flags, no
            // auth, network down).
            if status >= 400 {
                let body_value: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
                let message = body_value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| stderr.trim())
                    .to_string();
                return Err(GitHubError::Api {
                    method: request.method,
                    endpoint: request.endpoint,
                    status,
                    message,
                    body: payload.to_string(),
                });
            }
            return Err(GitHubError::Cli {
                status: output.status.code().unwrap_or(-1),
                stderr: if stderr.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                },
            });
        }

        // A `204 No Content` (routine for DELETE) has an empty body.
        let body = if payload.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(payload).map_err(|source| GitHubError::Decode {
                endpoint: request.endpoint.clone(),
                source,
            })?
        };

        Ok(Response {
            status,
            body,
            headers,
        })
    }
}

/// Recover an HTTP status from a failed request that carried no status line.
///
/// `gh` reports the failure in two places, either of which may be absent:
/// the JSON error body it prints to stdout carries a `status` field, and its
/// stderr line ends in `(HTTP 403)`.
fn parse_error_status(body: &str, stderr: &str) -> Option<u16> {
    if let Ok(value) = serde_json::from_str::<Value>(body.trim())
        && let Some(status) = value.get("status")
    {
        // GitHub renders it as a string; be liberal about which.
        let parsed = status
            .as_str()
            .and_then(|status| status.parse::<u16>().ok())
            .or_else(|| {
                status
                    .as_u64()
                    .and_then(|status| u16::try_from(status).ok())
            });
        if let Some(status) = parsed {
            return Some(status);
        }
    }

    // `gh: Upgrade to GitHub Pro ... (HTTP 403)`
    let marker = stderr.rfind("(HTTP ")?;
    let rest = &stderr[marker + "(HTTP ".len()..];
    let end = rest.find(')')?;
    rest[..end].trim().parse().ok()
}

/// Split a `gh api --include` response into status, headers and body.
///
/// The output is a raw HTTP response head followed by a blank line and the body.
/// Redirects and `100 Continue` mean several heads can be stacked, so we keep the
/// last one.
fn parse_included_response(output: &str) -> (u16, Vec<(String, String)>, &str) {
    let mut rest = output;
    let mut status = 0u16;
    let mut headers = Vec::new();

    loop {
        if !rest.starts_with("HTTP/") {
            break;
        }
        // Header block ends at the first blank line, in either line ending.
        let Some((head, tail)) = split_head(rest) else {
            break;
        };

        let mut lines = head.lines();
        let Some(status_line) = lines.next() else {
            break;
        };
        status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);

        headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_string()))
            .collect();

        rest = tail;
    }

    (status, headers, rest)
}

/// Split at the first blank line, returning the head and the remainder.
fn split_head(input: &str) -> Option<(&str, &str)> {
    if let Some(index) = input.find("\r\n\r\n") {
        return Some((&input[..index], &input[index + 4..]));
    }
    input
        .find("\n\n")
        .map(|index| (&input[..index], &input[index + 2..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Method;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn builds_a_read_argv() {
        let transport = GhCliTransport::new();
        assert_eq!(
            transport.args(&Request::get("repos/o/r")),
            vec!["api", "--method", "GET", "--include", "repos/o/r"]
        );
    }

    #[test]
    fn paginated_reads_drop_headers_and_do_not_slurp() {
        let transport = GhCliTransport::new();
        let args = transport.args(&Request::list("repos/o/r/labels"));
        assert!(args.contains(&"--paginate".to_string()));
        // `--include` and `--paginate` are mutually exclusive in `gh`.
        assert!(!args.contains(&"--include".to_string()));
        // `--slurp` would nest one array per page instead of merging them.
        assert!(!args.contains(&"--slurp".to_string()));
    }

    #[test]
    fn bodies_are_piped_as_raw_json() {
        let transport = GhCliTransport::new();
        let args = transport.args(&Request::post("repos/o/r/labels", json!({"name": "bug"})));
        let position = args.iter().position(|a| a == "--input").expect("--input");
        assert_eq!(args[position + 1], "-");
    }

    #[test]
    fn honours_the_enterprise_hostname() {
        let transport = GhCliTransport::new().with_hostname(Some("github.acme.com".into()));
        let args = transport.args(&Request::get("repos/o/r"));
        assert!(
            args.windows(2)
                .any(|w| w == ["--hostname", "github.acme.com"])
        );
    }

    #[test]
    fn parses_status_headers_and_body() {
        let raw = "HTTP/2.0 200 OK\r\nX-OAuth-Scopes: repo, read:org\r\nContent-Type: application/json\r\n\r\n{\"name\":\"bug\"}";
        let (status, headers, body) = parse_included_response(raw);
        assert_eq!(status, 200);
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "x-oauth-scopes")
                .map(|(_, v)| v.as_str()),
            Some("repo, read:org")
        );
        assert_eq!(body, "{\"name\":\"bug\"}");
    }

    #[test]
    fn keeps_the_last_head_when_redirected() {
        let raw = "HTTP/1.1 301 Moved\r\nLocation: /elsewhere\r\n\r\nHTTP/1.1 200 OK\r\nX-Trace: 2\r\n\r\n{}";
        let (status, headers, body) = parse_included_response(raw);
        assert_eq!(status, 200);
        assert_eq!(headers, vec![("x-trace".to_string(), "2".to_string())]);
        assert_eq!(body, "{}");
    }

    #[test]
    fn recovers_the_status_from_a_failed_paginated_request() {
        // A paginated request has no status line, so without this every HTTP
        // error on a collection endpoint reads as an authentication problem.
        let body = r#"{"message":"Upgrade to GitHub Pro or make this repository public to enable this feature.","status":"403"}"#;
        assert_eq!(parse_error_status(body, ""), Some(403));
    }

    #[test]
    fn recovers_the_status_from_stderr_when_the_body_has_none() {
        let stderr = "gh: Not Found (HTTP 404)";
        assert_eq!(parse_error_status("", stderr), Some(404));
    }

    #[test]
    fn accepts_a_numeric_status_field() {
        assert_eq!(parse_error_status(r#"{"status":422}"#, ""), Some(422));
    }

    #[test]
    fn reports_no_status_when_neither_source_has_one() {
        // A genuine CLI failure — bad flags, no network — must stay a CLI
        // error rather than being dressed up as an API rejection.
        assert_eq!(parse_error_status("", "gh: command not found"), None);
        assert_eq!(parse_error_status("not json", ""), None);
    }

    #[test]
    fn tolerates_an_empty_body() {
        let raw = "HTTP/2.0 204 No Content\r\n\r\n";
        let (status, _, body) = parse_included_response(raw);
        assert_eq!(status, 204);
        assert!(body.is_empty());
    }

    #[test]
    #[should_panic(expected = "read-only mode")]
    fn read_only_transport_refuses_writes() {
        let transport = GhCliTransport::new().read_only(true);
        let request = Request::new(Method::Delete, "repos/o/r/labels/bug");
        // The guard fires before any process is spawned.
        futures_lite_block_on(transport.request(request));
    }

    /// Minimal blocking executor so the guard test needs no runtime dependency.
    fn futures_lite_block_on(future: impl Future<Output = Result<Response>>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let _ = runtime.block_on(future);
    }
}
