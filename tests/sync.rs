//! End-to-end tests for `plan` and `sync`.
//!
//! These drive the real binary through the stub `gh`, so they cover argument
//! parsing, configuration loading, the diff, the apply path and the rendering all
//! at once. Assertions target the recorded request log wherever possible: that a
//! plan *reads well* matters less than that `sync` issues exactly the right calls.

mod common;

use common::{Fixture, Sandbox, default_repository};

const LABELS: &str = r#"[
    {"name": "bug", "color": "d73a4a", "description": "Something isn't working"},
    {"name": "legacy", "color": "cccccc"}
]"#;

#[test]
fn plan_reports_no_changes_when_everything_matches() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n    description: Something isn't working\n",
        )
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn plan_exits_with_a_distinct_code_when_changes_are_pending() {
    // CI needs to tell "drift detected" apart from "the run failed".
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    runner.run(&["plan", "-R", "o/r"]).expect_status(2);
}

#[test]
fn plan_never_issues_a_write() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    assert!(
        output.writes().is_empty(),
        "plan performed writes: {:?}",
        output.writes()
    );
}

#[test]
fn plan_does_not_read_resources_the_configuration_does_not_manage() {
    // Absent means unmanaged, and unmanaged should not even cost a request.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    assert!(
        !output.requests.iter().any(|r| r.contains("/rulesets")),
        "read rulesets despite them being unmanaged: {:?}",
        output.requests
    );
}

#[test]
fn sync_creates_a_missing_label() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", LABELS)
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].starts_with("POST repos/o/r/labels"));
    assert!(writes[0].contains("\"name\":\"feature\""), "{}", writes[0]);
}

#[test]
fn sync_does_not_delete_unmanaged_labels_by_default() {
    // The single most important safety property: adopting the tool on an
    // existing repository must not destroy anything.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n    description: Something isn't working\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);
    assert!(
        output.writes().is_empty(),
        "unmanaged label was touched: {:?}",
        output.writes()
    );
}

#[test]
fn sync_deletes_unmanaged_labels_when_pruning_is_requested() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n    description: Something isn't working\n")
        .get("repos/o/r/labels", LABELS)
        .respond("DELETE", "repos/o/r/labels/legacy", Fixture::no_content())
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--prune"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].starts_with("DELETE repos/o/r/labels/legacy"));
}

#[test]
fn no_prune_overrides_the_configuration() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nlabels:\n  prune: true\n  items:\n    - name: bug\n      color: d73a4a\n      description: Something isn't working\n",
        )
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--no-prune"]);
    output.expect_status(0);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn sync_is_idempotent() {
    // Running twice must produce no changes the second time. This is the core
    // promise of the tool, and the reason normalisation exists.
    let config = "version: 1\nrepository:\n  description: hello\ntopics:\n  - rust\n";

    let runner = Sandbox::new()
        .config(config)
        .repository(&default_repository())
        .get("repos/o/r/topics", r#"{"names": ["rust"]}"#)
        .accept("PATCH", "repos/o/r")
        .build();

    let first = runner.run(&["plan", "-R", "o/r"]);
    first.expect_status(2);

    // Second pass with the description now applied.
    let runner = Sandbox::new()
        .config(config)
        .repository(&common::repository_with(&[("description", "hello")]))
        .get("repos/o/r/topics", r#"{"names": ["rust"]}"#)
        .build();

    let second = runner.run(&["plan", "-R", "o/r"]);
    second.expect_status(0);
    assert!(second.stdout.contains("up to date"), "{}", second.stdout);
}

#[test]
fn a_case_only_colour_difference_is_not_a_change() {
    // GitHub lowercases colours; without normalisation this diffs forever.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: \"#D73A4A\"\n    description: Something isn't working\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    runner.run(&["plan", "-R", "o/r"]).expect_status(0);
}

#[test]
fn autolinks_are_recreated_because_there_is_no_update_endpoint() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nautolinks:\n  - key_prefix: OPS-\n    url_template: https://new.example.com/<num>\n",
        )
        .get(
            "repos/o/r/autolinks",
            r#"[{"id": 7, "key_prefix": "OPS-", "url_template": "https://old.example.com/<num>", "is_alphanumeric": true}]"#,
        )
        .respond("DELETE", "repos/o/r/autolinks/7", Fixture::no_content())
        .respond("POST", "repos/o/r/autolinks", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    // Delete must come first: GitHub rejects a duplicate prefix.
    assert!(
        writes[0].starts_with("DELETE repos/o/r/autolinks/7"),
        "{writes:?}"
    );
    assert!(
        writes[1].starts_with("POST repos/o/r/autolinks"),
        "{writes:?}"
    );
}

#[test]
fn topics_are_written_as_the_complete_list() {
    // The endpoint replaces everything, so a non-pruning run must resend the
    // topics it does not manage or it would delete them by omission.
    let runner = Sandbox::new()
        .config("version: 1\ntopics:\n  - rust\n")
        .get("repos/o/r/topics", r#"{"names": ["legacy"]}"#)
        .accept("PUT", "repos/o/r/topics")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(
        writes[0].contains("legacy"),
        "unmanaged topic dropped: {writes:?}"
    );
    assert!(writes[0].contains("rust"), "{writes:?}");
}

#[test]
fn sync_stops_at_the_first_failure_by_default() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nlabels:\n  - name: aaa\n    color: a2eeef\n  - name: zzz\n    color: a2eeef\n",
        )
        .get("repos/o/r/labels", "[]")
        .respond(
            "POST",
            "repos/o/r/labels",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);
    assert_eq!(output.writes().len(), 1, "should not have retried");
    assert!(output.stdout.contains("skipped"), "{}", output.stdout);
}

#[test]
fn a_permission_failure_points_at_doctor() {
    // A 403 is nearly always the wrong token, not a bad configuration.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .respond(
            "PATCH",
            "repos/o/r",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("gh settings doctor"),
        "stderr:\n{}",
        output.stderr
    );
}

#[test]
fn sync_refuses_to_run_unattended_without_yes() {
    // Tests run without a terminal, which is exactly the CI situation we want to
    // refuse rather than silently assume consent for.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .build();

    let output = runner.run(&["sync", "-R", "o/r"]);
    output.expect_status(1);
    assert!(output.writes().is_empty());
    assert!(
        output.stderr.contains("--yes"),
        "stderr:\n{}",
        output.stderr
    );
}

#[test]
fn dry_run_performs_no_writes() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--dry-run"]);
    output.expect_status(0);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn only_restricts_the_run_to_the_named_resources() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\ntopics:\n  - rust\n")
        .get("repos/o/r/labels", "[]")
        .get("repos/o/r/topics", r#"{"names": []}"#)
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--only", "labels"]);
    assert!(
        !output.requests.iter().any(|r| r.contains("/topics")),
        "{:?}",
        output.requests
    );
}

#[test]
fn a_saved_plan_can_be_applied() {
    let config = "version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n";
    let runner = Sandbox::new()
        .config(config)
        .get("repos/o/r/labels", "[]")
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .build();

    runner
        .run(&["plan", "-R", "o/r", "--out", "plan.json"])
        .expect_status(2);

    let saved = common::read(runner.path(), "plan.json");
    assert!(saved.contains("\"version\": 1"), "{saved}");
    assert!(saved.contains("\"resource\": \"labels\""), "{saved}");

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--plan", "plan.json"]);
    output.expect_status(0);
    assert_eq!(output.writes().len(), 1);
}

#[test]
fn applying_a_stale_plan_is_refused() {
    // A reviewed plan that silently applies something else would defeat the
    // point of having a plan artifact.
    let config = "version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n";

    let runner = Sandbox::new()
        .config(config)
        .get("repos/o/r/labels", "[]")
        .build();
    runner
        .run(&["plan", "-R", "o/r", "--out", "plan.json"])
        .expect_status(2);
    let saved = common::read(runner.path(), "plan.json");

    // The label now exists, so the saved plan no longer describes reality.
    let changed = Sandbox::new()
        .config(config)
        .get(
            "repos/o/r/labels",
            r#"[{"name": "feature", "color": "a2eeef"}]"#,
        )
        .build();
    std::fs::write(changed.path().join("plan.json"), saved).unwrap();

    let output = changed.run(&["sync", "-R", "o/r", "--yes", "--plan", "plan.json"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("changed since"),
        "stderr:\n{}",
        output.stderr
    );
    assert!(output.writes().is_empty());
}

#[test]
fn a_plan_for_another_repository_is_refused() {
    let config = "version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n";
    let runner = Sandbox::new()
        .config(config)
        .get("repos/o/r/labels", "[]")
        .build();
    runner
        .run(&["plan", "-R", "o/r", "--out", "plan.json"])
        .expect_status(2);

    let output = runner.run(&["sync", "-R", "other/repo", "--yes", "--plan", "plan.json"]);
    output.expect_status(1);
    assert!(output.writes().is_empty());
}

#[test]
fn json_output_is_machine_readable() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--format", "json"]);
    output.expect_status(2);

    let value: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(value["repository"], "o/r");
    assert_eq!(value["counts"]["create"], 1);
    assert_eq!(value["changes"][0]["resource"], "labels");
}

#[test]
fn sync_emits_json_even_when_there_is_nothing_to_do() {
    // Nothing to do is the *common* case for anything automated. This path used
    // to print the human "up to date" line and return before reaching the JSON
    // branch, so a consumer parsing stdout broke precisely when everything was
    // fine — and nothing caught it, because the only JSON test covered `plan`.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n    description: Something isn't working\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--format", "json"]);
    output.expect_status(0);

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout was not JSON: {error}\n{}", output.stdout));

    assert_eq!(value["success"], true);
    assert_eq!(value["applied"]["create"], 0);
    assert_eq!(value["skipped"], 0);
    assert_eq!(value["failures"].as_array().unwrap().len(), 0);
}

#[test]
fn sync_emits_json_after_applying_changes() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--format", "json"]);
    output.expect_status(0);

    let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(value["success"], true);
    assert_eq!(value["applied"]["create"], 1);
}

#[test]
fn sync_reports_failures_in_json() {
    // An action needs the status code to distinguish "wrong token" from
    // "wrong configuration".
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .respond(
            "POST",
            "repos/o/r/labels",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--format", "json"]);
    output.expect_status(1);

    let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(value["success"], false);
    assert_eq!(value["failures"][0]["resource"], "labels");
    assert_eq!(value["failures"][0]["status"], 403);
}

#[test]
fn an_unresolvable_bypass_team_fails_the_plan_and_writes_nothing() {
    // The whole reason resolution happens during `prepare` rather than lazily
    // during apply: a misspelled team is caught while planning, so nothing has
    // been written yet. Resolving mid-apply would abort with some rulesets
    // already created and some not.
    let runner = Sandbox::new()
        .config(
            "version: 1\nrulesets:\n  - name: main\n    bypass_actors:\n      - team: nonexistent\n    rules:\n      - type: non_fast_forward\n",
        )
        .respond(
            "GET",
            "orgs/o/teams/nonexistent",
            Fixture::error(404, "Not Found"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);

    assert!(
        output.stderr.contains("nonexistent"),
        "the error must name the slug: {}",
        output.stderr
    );
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn a_bypass_team_is_looked_up_once_however_often_it_is_named() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nrulesets:\n  - name: one\n    bypass_actors:\n      - team: eng\n    rules:\n      - type: non_fast_forward\n  - name: two\n    bypass_actors:\n      - team: eng\n    rules:\n      - type: non_fast_forward\n",
        )
        .get("orgs/o/teams/eng", r#"{"id": 42}"#)
        .get("repos/o/r/rulesets", "[]")
        .respond("POST", "repos/o/r/rulesets", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let lookups = output
        .requests
        .iter()
        .filter(|request| request.contains("orgs/o/teams/eng"))
        .count();
    assert_eq!(lookups, 1, "{:?}", output.requests);
}
