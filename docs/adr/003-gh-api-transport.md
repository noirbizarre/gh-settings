# 3. GitHub access shells out to `gh api` behind a port

## Status

Accepted.

## Context

Two options were considered for talking to GitHub.

**An HTTP client** (`reqwest` plus `gh auth token`) is typed and concurrent, but
requires reimplementing pagination, retries, rate-limit handling and GitHub
Enterprise base URLs — and testing it needs HTTP mocking.

**Shelling out to `gh api`** inherits all of that from the GitHub CLI, which is
guaranteed to be present because this *is* a `gh` extension. It also makes the
whole layer testable by putting a stub `gh` on `PATH`.

The cost is process spawn overhead, roughly 30–60 ms per call.

## Decision

Shell out to `gh api`, behind a `GitHubClient` port.

Resources depend on the port only; they never spawn a process, never see a
header, and never build a URL beyond an endpoint path.

## Consequences

* Authentication, GitHub Enterprise, pagination and retries are free and always
  consistent with what the user's `gh` already does.
* The test suite needs no HTTP mocking, and can assert on the exact requests
  issued rather than on output alone (see ADR-012).
* Reads are sequential, so a large repository costs a few hundred milliseconds.
  Acceptable today; if profiling ever says otherwise, an HTTP transport can be
  added behind the same port without touching a single resource.
* `gh` must be on `PATH`. For a `gh` extension this is not a real constraint,
  and `doctor` reports it clearly when it is not.
