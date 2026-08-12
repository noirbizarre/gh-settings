# 12. Testing through a `gh` process stub and request-log assertions

## Status

Accepted, extended by [019](019-live-test-sandboxes.md).

## Context

The apply path is where mistakes are expensive: a wrong endpoint or a malformed
body can destroy configuration. Testing it needs the requests to be observable.

Because the transport spawns `gh` (ADR-003), the seam is a process boundary
rather than an HTTP one.

## Decision

Three layers:

1. **Unit tests** on `normalize` and `diff`, which are pure. This is the bulk of
   the suite and needs no runtime.
2. **A stub `gh`** placed first on `PATH` in a temporary directory. It replays
   canned responses and appends every invocation to a log. Tests assert on that
   log — *which* requests were made, in *which* order, with *which* bodies — not
   merely on the output.
3. **Snapshot tests** (`insta`) over rendered diagnostics and plans, with colour
   and terminal detection disabled for determinism.

The stub answers unregistered *reads* with an empty result, so a test declares
only the fixtures it cares about. It fails unregistered *writes*, so an
unexpected mutation can never pass silently.

## Consequences

* No HTTP mocking, no network, no test-only code paths in the binary.
* Assertions can express intent directly: "delete must be issued before create",
  "plan must never write", "an unmanaged resource must not even be read".
* The stub is a shell script, so it only runs on Unix. Windows CI runs the unit
  and snapshot layers.
