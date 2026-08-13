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

// --- ruleset apply path -----------------------------------------------------
//
// Rulesets are the most intricate resource and the only one with a history of
// bugs found against the real API rather than against a fixture: the permanent
// diff, and the `422` for a partially-declared rule. Until these tests existed
// the write path — and in particular the server id threaded through the change
// payload — was verified by one person, once, by hand.

/// The list endpoint, which omits `rules` and `bypass_actors`.
const RULESET_SUMMARIES: &str = r#"[
    {"id": 42, "name": "main", "target": "branch", "enforcement": "active"}
]"#;

/// The detail endpoint, which is the only one that returns a whole ruleset.
///
/// Carries the server-only fields deliberately, so `from_state` is exercised on
/// a payload shaped like GitHub's rather than a tidied one.
const RULESET_DETAIL: &str = r#"{
    "id": 42,
    "node_id": "RRS_abc",
    "name": "main",
    "target": "branch",
    "enforcement": "active",
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-02T00:00:00Z",
    "source": "o/r",
    "source_type": "Repository",
    "current_user_can_bypass": "always",
    "_links": {"self": {"href": "https://api.github.com/repos/o/r/rulesets/42"}},
    "bypass_actors": [],
    "rules": [{"type": "non_fast_forward"}]
}"#;

/// Register the two reads `current()` performs for one existing ruleset.
fn with_existing_ruleset(sandbox: Sandbox) -> Sandbox {
    sandbox
        .get("repos/o/r/rulesets", RULESET_SUMMARIES)
        .get("repos/o/r/rulesets/42", RULESET_DETAIL)
}

#[test]
fn sync_creates_a_ruleset_with_its_rules() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nrulesets:\n  - name: main\n    enforcement: active\n    rules:\n      - type: non_fast_forward\n",
        )
        .get("repos/o/r/rulesets", "[]")
        .respond("POST", "repos/o/r/rulesets", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");

    let request = writes[0];
    assert!(
        request.starts_with("POST repos/o/r/rulesets "),
        "creation must POST to the collection: {request}"
    );

    // A ruleset whose rules were dropped on the way out would still create
    // successfully and still report success, so the body is the assertion.
    let body: serde_json::Value = body_of(request);
    assert_eq!(body["name"], "main");
    assert_eq!(body["target"], "branch");
    assert_eq!(body["enforcement"], "active");
    assert_eq!(body["rules"][0]["type"], "non_fast_forward");
}

#[test]
fn sync_updates_a_ruleset_through_the_server_id_not_its_name() {
    // The identity in the configuration is the name, but the API is addressed by
    // id. That translation happens in `diff`, which puts the id from `current`
    // into the change payload, and nothing else checks it.
    let runner = with_existing_ruleset(Sandbox::new().config(
        "version: 1\nrulesets:\n  - name: main\n    enforcement: disabled\n    rules:\n      - type: non_fast_forward\n",
    ))
    .accept("PUT", "repos/o/r/rulesets/42")
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(
        writes[0].starts_with("PUT repos/o/r/rulesets/42 "),
        "update must address the id: {}",
        writes[0]
    );

    let body: serde_json::Value = body_of(writes[0]);
    assert_eq!(body["enforcement"], "disabled");
    // GitHub rejects a partial rule list, so an update sends the whole ruleset
    // rather than the changed field.
    assert_eq!(body["rules"][0]["type"], "non_fast_forward");
}

#[test]
fn sync_deletes_an_unmanaged_ruleset_when_pruning() {
    let runner = with_existing_ruleset(
        Sandbox::new().config("version: 1\nrulesets:\n  prune: true\n  items: []\n"),
    )
    .respond("DELETE", "repos/o/r/rulesets/42", Fixture::no_content())
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    assert_eq!(
        output.writes(),
        vec!["DELETE repos/o/r/rulesets/42 "],
        "{:?}",
        output.writes()
    );
}

#[test]
fn sync_leaves_an_unmanaged_ruleset_alone_without_prune() {
    // Pruning is opt-in. A configuration that declares no rulesets at all still
    // manages the section, and must not delete what it did not declare.
    let runner = with_existing_ruleset(Sandbox::new().config("version: 1\nrulesets: []\n")).build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);
    assert!(
        output.writes().is_empty(),
        "deleted a ruleset without --prune: {:?}",
        output.writes()
    );
}

#[test]
fn an_unknown_rule_type_reaches_the_api_untouched() {
    // The untyped passthrough is a promise (ADR-backed): a rule type this build
    // predates must round-trip rather than be silently dropped on the next sync.
    // Tested at the `diff` layer already; this is the half that would actually
    // lose data, since it is the request body that reaches GitHub.
    let runner = Sandbox::new()
        .config(
            "version: 1\nrulesets:\n  - name: main\n    rules:\n      - type: some_future_rule\n        parameters:\n          shiny: true\n",
        )
        .get("repos/o/r/rulesets", "[]")
        .respond("POST", "repos/o/r/rulesets", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let body: serde_json::Value = body_of(output.writes()[0]);
    assert_eq!(body["rules"][0]["type"], "some_future_rule");
    assert_eq!(body["rules"][0]["parameters"]["shiny"], true);
}

#[test]
fn a_ruleset_body_never_carries_the_fields_only_the_server_may_set() {
    // `id`, `created_at` and friends come back from the detail endpoint. Sending
    // them back is how a resource acquires a permanent diff, or a 422.
    let runner = with_existing_ruleset(Sandbox::new().config(
        "version: 1\nrulesets:\n  - name: main\n    enforcement: disabled\n    rules:\n      - type: non_fast_forward\n",
    ))
    .accept("PUT", "repos/o/r/rulesets/42")
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let body: serde_json::Value = body_of(output.writes()[0]);
    for field in [
        "id",
        "node_id",
        "created_at",
        "updated_at",
        "_links",
        "current_user_can_bypass",
        "source",
        "source_type",
    ] {
        assert!(
            body.get(field).is_none(),
            "server-only field `{field}` was sent back: {body}"
        );
    }
}

/// The JSON body of a logged request, which is everything after
/// `METHOD endpoint `.
#[track_caller]
fn body_of(request: &str) -> serde_json::Value {
    let body = request
        .splitn(3, ' ')
        .nth(2)
        .unwrap_or_else(|| panic!("request has no body: {request}"));
    serde_json::from_str(body).unwrap_or_else(|error| panic!("body is not JSON ({error}): {body}"))
}

// --- rendering --------------------------------------------------------------
//
// `plan --verbose` is the field-level before/after view, and the stated reason
// there is no separate `diff` command — which makes it a product surface with
// no snapshot until now.

#[test]
fn plan_renders_pending_changes() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn plan_renders_field_level_changes_when_verbose() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: ff0000\n    description: Broken\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--verbose"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn plan_renders_an_up_to_date_repository() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n    description: Something isn't working\n",
        )
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn sync_renders_what_it_applied() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", LABELS)
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn continue_on_error_attempts_every_change() {
    // The mirror of `sync_stops_at_the_first_failure_by_default`. The flag has
    // been implemented and unit-tested since it landed, but never exercised
    // through the binary — so nothing proved the CLI actually threaded it into
    // `ApplyOptions`, which is the only part a user can observe.
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

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--continue-on-error"]);

    // Still a failure: carrying on is not the same as succeeding.
    output.expect_status(1);
    assert_eq!(
        output.writes().len(),
        2,
        "both changes should have been attempted: {:?}",
        output.writes()
    );
    assert!(
        output.stdout.contains("0 skipped"),
        "carrying on means nothing is skipped: {}",
        output.stdout
    );
}

#[test]
fn continue_on_error_applies_what_it_can() {
    // The reason to want the flag: one broken change must not cost you the
    // others. Deletions address a label by name, which is what makes a
    // per-change failure expressible here — the two creations share an endpoint
    // and so cannot be told apart by the stub.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  prune: true\n  items:\n    - name: feature\n      color: a2eeef\n")
        .get(
            "repos/o/r/labels",
            r#"[{"name": "aaa", "color": "cccccc"}, {"name": "zzz", "color": "cccccc"}]"#,
        )
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .respond(
            "DELETE",
            "repos/o/r/labels/aaa",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .respond("DELETE", "repos/o/r/labels/zzz", Fixture::no_content())
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--continue-on-error"]);

    // One change failed, so the run failed — but the other two still happened.
    output.expect_status(1);
    assert_eq!(
        output.writes().len(),
        3,
        "every change should have been attempted: {:?}",
        output.writes()
    );
    assert!(
        output.stdout.contains("2 applied"),
        "the changes that could succeed should have: {}",
        output.stdout
    );
}

#[test]
fn continue_on_error_reports_every_failure_in_json() {
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

    let output = runner.run(&[
        "sync",
        "-R",
        "o/r",
        "--yes",
        "--continue-on-error",
        "--format",
        "json",
    ]);
    output.expect_status(1);

    let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(value["success"], false);
    let failures = value["failures"].as_array().expect("failures");
    assert_eq!(
        failures.len(),
        2,
        "a machine consumer must see both failures, not just the first: {}",
        output.stdout
    );
}

#[test]
fn a_permission_failure_names_the_permission_that_was_missing() {
    // The point of ADR-015's single declaration: the requirement is already in
    // memory when the 403 arrives, so telling the user to go and look it up
    // was withholding something we were holding.
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
        output.stderr.contains("Administration: write"),
        "the explanation must name the permission: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("repo"),
        "and the classic scope: {}",
        output.stderr
    );

    assert_cli_snapshot!(output.stderr);
}

#[test]
fn a_permission_failure_on_labels_does_not_mention_administration() {
    // The explanation is per-resource. Labels need `Issues: write`, and telling
    // someone to grant `Administration: write` for a failed label write would
    // send them to change a setting that has nothing to do with it.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .respond(
            "POST",
            "repos/o/r/labels",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);

    assert!(output.stderr.contains("Issues: write"), "{}", output.stderr);
    assert!(
        !output.stderr.contains("Administration"),
        "labels do not need Administration: {}",
        output.stderr
    );
}

#[test]
fn a_permission_failure_inside_actions_says_the_token_cannot_be_granted_it() {
    // Inside Actions the note is the answer rather than a distraction: no
    // `permissions:` block can grant `Administration: write`, so a user who
    // keeps adding permissions will never get there.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .respond(
            "PATCH",
            "repos/o/r",
            Fixture::error(403, "Resource not accessible by integration"),
        )
        .build();

    let output = runner.run_with_env(
        &["sync", "-R", "o/r", "--yes"],
        &[("GITHUB_ACTIONS", "true")],
    );
    output.expect_status(1);

    assert!(
        output.stderr.contains("cannot be granted to GITHUB_TOKEN"),
        "{}",
        output.stderr
    );
}

// --- pre-flight -------------------------------------------------------------

#[test]
fn sync_refuses_to_start_when_the_token_certainly_cannot_write() {
    // A classic token advertises its scopes, so "this will fail" is a fact
    // rather than a guess. Discovering it after the first failed write is
    // strictly worse, and with --continue-on-error, worse several times over.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .token("ghp_x")
        .scopes("gist")
        .respond(
            "GET",
            "",
            Fixture::ok("{}").header("x-oauth-scopes", "gist"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);

    assert!(
        output.writes().is_empty(),
        "the whole point is that nothing is attempted: {:?}",
        output.writes()
    );
    assert!(
        output.stderr.contains("Refusing to start"),
        "{}",
        output.stderr
    );
    assert_cli_snapshot!(output.stderr);
}

#[test]
fn the_preflight_lets_an_unintrospectable_token_proceed() {
    // The constraint that matters. This token reports no scopes and the admin
    // probe cannot settle it either, so we do not know — and not knowing must
    // never block a write. The user gets GitHub's own answer instead of ours.
    //
    // If this test ever starts failing because the pre-flight became more
    // confident, the pre-flight is wrong, not the test: there is no flag to
    // overrule a refusal.
    let no_permissions = {
        let mut value: serde_json::Value =
            serde_json::from_str(&default_repository()).expect("valid default");
        // `permissions` absent means the probe cannot tell, which is the state
        // this test is about.
        value.as_object_mut().unwrap().remove("permissions");
        value.to_string()
    };

    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&no_permissions)
        .token("github_pat_x")
        .accept("PATCH", "repos/o/r")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);

    assert!(
        !output.stderr.contains("Refusing to start"),
        "an unknown verdict must not block: {}",
        output.stderr
    );
    assert!(
        output
            .writes()
            .iter()
            .any(|write| write.starts_with("PATCH")),
        "the write should have been attempted: {:?}",
        output.writes()
    );
}

#[test]
fn the_preflight_only_judges_the_resources_that_have_changes() {
    // Labels need `Issues: write`, which the Actions token holds. A plan that
    // touches only labels must not be refused because some *other* resource
    // would have been impossible.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: feature\n    color: a2eeef\n")
        .get("repos/o/r/labels", "[]")
        .token("ghs_actionstoken")
        .scopes("issues")
        .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
        .build();

    let output = runner.run_with_env(
        &["sync", "-R", "o/r", "--yes"],
        &[("GITHUB_ACTIONS", "true")],
    );
    output.expect_status(0);
    assert!(
        !output.stderr.contains("Refusing to start"),
        "{}",
        output.stderr
    );
}

#[test]
fn the_preflight_refuses_the_actions_token_for_repository_settings() {
    // The documented headline case, now caught before the write instead of
    // after it.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .token("ghs_actionstoken")
        .scopes("issues")
        .build();

    let output = runner.run_with_env(
        &["sync", "-R", "o/r", "--yes"],
        &[("GITHUB_ACTIONS", "true")],
    );
    output.expect_status(1);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
    assert!(
        output.stderr.contains("cannot be granted to GITHUB_TOKEN"),
        "{}",
        output.stderr
    );
}

#[test]
fn dry_run_is_never_blocked_by_the_preflight() {
    // `--dry-run` writes nothing, so there is nothing to refuse. Blocking it
    // would remove the one way to inspect a plan with a read-only token.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .token("ghp_x")
        .scopes("gist")
        .respond(
            "GET",
            "",
            Fixture::ok("{}").header("x-oauth-scopes", "gist"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--dry-run"]);
    output.expect_status(0);
    assert!(
        !output.stderr.contains("Refusing to start"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_preflight_refusal_is_machine_readable() {
    // A refusal is still an answer, and `--format json` promises stdout is
    // parseable. Printing prose to stderr and nothing to stdout would break a
    // consumer in exactly the situation it most needs to understand.
    let runner = Sandbox::new()
        .config("version: 1\nrepository:\n  description: hello\n")
        .repository(&default_repository())
        .token("ghp_x")
        .scopes("gist")
        .respond(
            "GET",
            "",
            Fixture::ok("{}").header("x-oauth-scopes", "gist"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--format", "json"]);
    output.expect_status(1);
    assert!(output.writes().is_empty(), "{:?}", output.writes());

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout was not JSON: {error}\n{}", output.stdout));

    assert_eq!(value["success"], false);
    let failures = value["failures"].as_array().expect("failures");
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["resource"], "repository");
    assert!(
        failures[0]["error"]
            .as_str()
            .unwrap()
            .contains("missing the `repo` scope")
    );
    assert!(
        failures[0]["status"].is_null(),
        "no request was made, so there is no HTTP status to report: {}",
        failures[0]
    );
}

#[test]
fn plan_rejects_an_invalid_configuration_in_the_requested_format() {
    // `plan` used to print a human diagnostic to stderr and nothing at all to
    // stdout, so a pipeline parsing stdout saw an empty document and could not
    // tell a broken configuration from a broken pipe.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: nothex\n")
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--format", "json"]);
    output.expect_status(1);

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout was not JSON: {error}\n{}", output.stdout));

    assert_eq!(value["valid"], false);
    assert_eq!(
        value["findings"][0]["code"],
        "gh_settings::labels::invalid_color"
    );
}

#[test]
fn sync_rejects_an_invalid_configuration_in_the_requested_format() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: nothex\n")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--format", "json"]);
    output.expect_status(1);

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout was not JSON: {error}\n{}", output.stdout));

    assert_eq!(value["valid"], false);
    assert!(
        output.writes().is_empty(),
        "wrote despite an invalid configuration: {:?}",
        output.writes()
    );
}

// ---------------------------------------------------------------------------
// Environments and variables
// ---------------------------------------------------------------------------

/// The environment list endpoint, with the page size the resource asks for.
const ENVIRONMENTS: &str = "repos/o/r/environments?per_page=100";

#[test]
fn environments_are_created_before_their_variables() {
    // A variable cannot be written into an environment that does not exist, so
    // the ordering declared through `depends_on` has to survive all the way to
    // the request log. This is the property ADR-011 exists for.
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        )
        .accept("PUT", "repos/o/r/environments/staging")
        .respond(
            "POST",
            "repos/o/r/environments/staging/variables",
            Fixture::created("{}"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(
        writes[0].starts_with("PUT repos/o/r/environments/staging"),
        "{writes:?}"
    );
    assert!(
        writes[1].starts_with("POST repos/o/r/environments/staging/variables"),
        "{writes:?}"
    );
}

#[test]
fn a_variable_in_an_environment_that_does_not_exist_yet_is_planned_as_a_creation() {
    // Planning happens entirely before applying, so this read hits an
    // environment that is not there yet. A 404 means "no variables", not
    // "something went wrong".
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        )
        .respond(
            "GET",
            "repos/o/r/environments/staging/variables?per_page=100",
            Fixture::error(404, "{\"message\": \"Not Found\"}"),
        )
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert!(
        output.stdout.contains("create staging variable URL"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_forbidden_variable_read_is_not_mistaken_for_an_empty_one() {
    // "You may not look" must never be silently read as "there is nothing
    // there", or a plan would propose creating variables that already exist.
    let runner = Sandbox::new()
        .config("version: 1\nvariables:\n  - name: REGION\n    value: eu\n")
        .respond(
            "GET",
            "repos/o/r/actions/variables?per_page=100",
            Fixture::error(403, "{\"message\": \"Resource not accessible\"}"),
        )
        .build();

    runner.run(&["plan", "-R", "o/r"]).expect_status(1);
}

#[test]
fn plan_does_not_read_environments_or_variables_when_they_are_unmanaged() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    for endpoint in ["/environments", "/variables"] {
        assert!(
            !output.requests.iter().any(|r| r.contains(endpoint)),
            "read {endpoint} despite it being unmanaged: {:?}",
            output.requests
        );
    }
}

#[test]
fn a_repository_variable_is_updated_in_place() {
    let runner = Sandbox::new()
        .config("version: 1\nvariables:\n  - name: REGION\n    value: us\n")
        .get(
            "repos/o/r/actions/variables?per_page=100",
            r#"{"total_count": 1, "variables": [{"name": "REGION", "value": "eu"}]}"#,
        )
        .accept("PATCH", "repos/o/r/actions/variables/REGION")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("\"value\":\"us\""), "{}", writes[0]);
}

#[test]
fn a_branch_pattern_change_deletes_before_it_creates() {
    // GitHub answers a duplicate pattern name with a 422 rather than merging,
    // so the order within a single change matters.
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  - name: staging\n    deployment_branch_policy:\n      branches: [main]\n",
        )
        .get(
            ENVIRONMENTS,
            r#"{"total_count": 1, "environments": [{"name": "staging", "protection_rules": [{"type": "branch_policy"}], "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}]}"#,
        )
        .get(
            "repos/o/r/environments/staging/deployment-branch-policies?per_page=100",
            r#"{"total_count": 1, "branch_policies": [{"id": 9, "name": "stale", "type": "branch"}]}"#,
        )
        .accept("PUT", "repos/o/r/environments/staging")
        .accept(
            "DELETE",
            "repos/o/r/environments/staging/deployment-branch-policies/9",
        )
        .respond(
            "POST",
            "repos/o/r/environments/staging/deployment-branch-policies",
            Fixture::created("{}"),
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 3, "{writes:?}");
    assert!(writes[0].starts_with("PUT repos/o/r/environments/staging "));
    assert!(
        writes[1].starts_with("DELETE repos/o/r/environments/staging/deployment-branch-policies/9")
    );
    assert!(
        writes[2].starts_with("POST repos/o/r/environments/staging/deployment-branch-policies")
    );
    assert!(writes[2].contains("\"name\":\"main\""), "{}", writes[2]);
}

#[test]
fn pruning_an_environment_is_reported_as_destructive() {
    let runner = Sandbox::new()
        .config("version: 1\nenvironments:\n  prune: true\n  items:\n    - name: staging\n")
        .get(
            ENVIRONMENTS,
            r#"{"total_count": 2, "environments": [{"name": "staging", "protection_rules": []}, {"name": "legacy", "protection_rules": []}]}"#,
        )
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert!(
        output.stdout.contains(
            "delete environment legacy (also deletes its variables, secrets and deployment history)"
        ),
        "{}",
        output.stdout
    );
}

#[test]
fn sync_leaves_an_unmanaged_environment_alone() {
    let runner = Sandbox::new()
        .config("version: 1\nenvironments:\n  - name: staging\n")
        .get(
            ENVIRONMENTS,
            r#"{"total_count": 2, "environments": [{"name": "staging", "protection_rules": []}, {"name": "legacy", "protection_rules": []}]}"#,
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn a_reviewer_slug_is_resolved_before_anything_is_written() {
    let runner = Sandbox::new()
        .config("version: 1\nenvironments:\n  - name: staging\n    reviewers:\n      - team: eng\n")
        .get("orgs/o/teams/eng", r#"{"id": 7}"#)
        .accept("PUT", "repos/o/r/environments/staging")
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(
        writes[0].contains(r#""reviewers":[{"id":7,"type":"Team"}]"#),
        "{}",
        writes[0]
    );
}

#[test]
fn a_misspelled_reviewer_fails_the_plan_rather_than_a_half_finished_apply() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  - name: staging\n    reviewers:\n      - team: nosuchteam\n",
        )
        .respond(
            "GET",
            "orgs/o/teams/nosuchteam",
            Fixture::error(404, "{\"message\": \"Not Found\"}"),
        )
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(1);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
    assert!(output.stderr.contains("nosuchteam"), "{}", output.stderr);
}

#[test]
fn syncing_environments_is_idempotent() {
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  - name: staging\n    wait_timer: 30\n    reviewers: []\n    deployment_branch_policy: null\n",
        )
        .get(
            ENVIRONMENTS,
            r#"{"total_count": 1, "environments": [{"name": "staging", "protection_rules": [{"type": "wait_timer", "wait_timer": 30}], "deployment_branch_policy": null}]}"#,
        )
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn plan_renders_environments_and_variables() {
    // Verbose, destructive, and spanning both scopes: the shape most likely to
    // read badly if a summary or a field diff is wrong.
    let runner = Sandbox::new()
        .config(
            "version: 1\nenvironments:\n  prune: true\n  items:\n    - name: production\n      wait_timer: 15\n      reviewers:\n        - team: engineering\n      deployment_branch_policy:\n        branches: [main]\n      variables:\n        - name: DEPLOY_URL\n          value: https://example.com\nvariables:\n  - name: DEFAULT_REGION\n    value: eu-west-1\n",
        )
        .get("orgs/o/teams/engineering", r#"{"id": 7}"#)
        .get(
            ENVIRONMENTS,
            r#"{"total_count": 1, "environments": [{"name": "legacy", "protection_rules": []}]}"#,
        )
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "-v"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

const PAGES_SITE: &str = r#"{
    "build_type": "legacy",
    "source": {"branch": "gh-pages", "path": "/"},
    "cname": "docs.example.com",
    "https_enforced": true,
    "public": true
}"#;

// Note the `public` above: the API reports it, but neither `POST` nor `PUT`
// accepts it, so it must never appear in a request body.

#[test]
fn plan_does_not_read_pages_when_unmanaged() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    assert!(
        !output.requests.iter().any(|r| r.ends_with("/pages")),
        "read pages despite them being unmanaged: {:?}",
        output.requests
    );
}

#[test]
fn sync_enables_pages_with_a_post_then_a_put() {
    // `POST /pages` accepts only the build type and source, so anything else has
    // to follow in a `PUT` against the site it has just created.
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  build_type: legacy\n  source:\n    branch: gh-pages\n  cname: docs.example.com\n")
        .respond("GET", "repos/o/r/pages", Fixture::error(404, "Not Found"))
        .respond("POST", "repos/o/r/pages", Fixture::created("{}"))
        .respond("PUT", "repos/o/r/pages", Fixture::no_content())
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(writes[0].starts_with("POST repos/o/r/pages"), "{writes:?}");
    assert!(writes[0].contains(r#""branch":"gh-pages""#), "{writes:?}");
    // The domain cannot ride along with the creation.
    assert!(!writes[0].contains("cname"), "{writes:?}");
    assert!(writes[1].starts_with("PUT repos/o/r/pages"), "{writes:?}");
    assert!(
        writes[1].contains(r#""cname":"docs.example.com""#),
        "{writes:?}"
    );
}

#[test]
fn sync_leaves_an_unmanaged_pages_field_alone() {
    // The property the whole design rests on: a file setting only
    // `https_enforced` must not reset the custom domain or the build type.
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  https_enforced: false\n")
        .get("repos/o/r/pages", PAGES_SITE)
        .respond("PUT", "repos/o/r/pages", Fixture::no_content())
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(
        writes[0].contains(r#""https_enforced":false"#),
        "{writes:?}"
    );
    assert!(!writes[0].contains("cname"), "{writes:?}");
    assert!(!writes[0].contains("build_type"), "{writes:?}");
}

#[test]
fn sync_never_disables_pages_even_with_prune() {
    // There is no way to declare "off", so `--prune` has nothing to act on. A
    // published site must not come down because of a missing key.
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  build_type: legacy\n  source:\n    branch: gh-pages\n")
        .get("repos/o/r/pages", PAGES_SITE)
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes", "--prune"]);
    output.expect_status(0);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn a_pages_domain_differing_only_in_case_is_not_a_change() {
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  build_type: legacy\n  cname: Docs.Example.COM\n")
        .get("repos/o/r/pages", PAGES_SITE)
        .build();

    runner.run(&["plan", "-R", "o/r"]).expect_status(0);
}

#[test]
fn the_pages_plan_reads_the_way_it_should() {
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  build_type: legacy\n  source:\n    branch: gh-pages\n    path: /docs\n  cname: new.example.com\n")
        .get("repos/o/r/pages", PAGES_SITE)
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--verbose"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn enabling_pages_reads_the_way_it_should() {
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  build_type: workflow\n  https_enforced: true\n")
        .respond("GET", "repos/o/r/pages", Fixture::error(404, "Not Found"))
        .build();

    let output = runner.run(&["plan", "-R", "o/r", "--verbose"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn the_public_flag_is_never_sent_even_though_github_reports_it() {
    // `GET /pages` returns `public`, but it is not a body parameter of either
    // endpoint. It is rejected by the schema, so it cannot reach a request.
    let runner = Sandbox::new()
        .config("version: 1\npages:\n  public: false\n")
        .get("repos/o/r/pages", PAGES_SITE)
        .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
    assert!(
        output.stderr.contains("public"),
        "the rejection must name the field\n{}",
        output.stderr
    );
}

/// A repository with Actions on and everything at GitHub's defaults.
fn actions_defaults(sandbox: Sandbox) -> Sandbox {
    sandbox
        .actions(
            "",
            r#"{"enabled": true, "allowed_actions": "all", "sha_pinning_required": false}"#,
        )
        .actions(
            "workflow",
            r#"{"default_workflow_permissions": "read", "can_approve_pull_request_reviews": false}"#,
        )
        .actions(
            "artifact-and-log-retention",
            r#"{"days": 90, "maximum_allowed_days": 400}"#,
        )
        .actions(
            "fork-pr-contributor-approval",
            r#"{"approval_policy": "first_time_contributors"}"#,
        )
        // Verbatim from a real public repository. None of the three answers
        // `404`, which is why they are worth writing out: reading them as
        // errors made every plan against a public repository fail outright.
        .respond(
            "GET",
            "repos/o/r/actions/permissions/access",
            Fixture::error(
                422,
                "Access policy only applies to internal and private repositories.",
            ),
        )
        .respond(
            "GET",
            "repos/o/r/actions/permissions/fork-pr-workflows-private-repos",
            Fixture::error(
                422,
                "Fork PR workflow settings is not allowed for public repositories.",
            ),
        )
        .respond(
            "GET",
            "repos/o/r/actions/permissions/selected-actions",
            Fixture::error(
                409,
                "All actions and workflows are allowed on this repository",
            ),
        )
}

#[test]
fn an_absent_actions_section_costs_no_requests() {
    // Absent means unmanaged, and unmanaged should not even be read.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .get("repos/o/r/labels", LABELS)
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    assert!(
        !output
            .requests
            .iter()
            .any(|r| r.contains("actions/permissions")),
        "read Actions settings despite them being unmanaged: {:?}",
        output.requests
    );
}

#[test]
fn a_matching_actions_section_plans_nothing() {
    let runner = actions_defaults(Sandbox::new().config(
        "version: 1\nactions:\n  enabled: true\n  allowed_actions: all\n  \
         default_workflow_permissions: read\n  artifact_and_log_retention_days: 90\n",
    ))
    .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn each_actions_endpoint_gets_its_own_write() {
    // GitHub rejects a body that mixes fields from two of these endpoints, so
    // the split is a correctness requirement rather than tidiness.
    let runner = actions_defaults(Sandbox::new().config(
        "version: 1\nactions:\n  allowed_actions: local_only\n  \
         default_workflow_permissions: write\n  artifact_and_log_retention_days: 30\n  \
         fork_pr_contributor_approval: all_external_contributors\n",
    ))
    .accept("PUT", "repos/o/r/actions/permissions")
    .accept("PUT", "repos/o/r/actions/permissions/workflow")
    .accept(
        "PUT",
        "repos/o/r/actions/permissions/artifact-and-log-retention",
    )
    .accept(
        "PUT",
        "repos/o/r/actions/permissions/fork-pr-contributor-approval",
    )
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 4, "{writes:?}");
    assert!(
        writes[0].starts_with("PUT repos/o/r/actions/permissions ")
            && writes[0].contains("\"allowed_actions\":\"local_only\"")
            // The API refuses this body without `enabled`; it comes from the
            // current state, so syncing a policy does not turn Actions off.
            && writes[0].contains("\"enabled\":true"),
        "{}",
        writes[0]
    );
    assert!(
        writes[1].contains("/workflow") && writes[1].contains("\"write\""),
        "{}",
        writes[1]
    );
    assert!(
        writes[2].contains("/artifact-and-log-retention") && writes[2].contains("\"days\":30"),
        "{}",
        writes[2]
    );
    assert!(
        writes[3].contains("/fork-pr-contributor-approval"),
        "{}",
        writes[3]
    );
}

#[test]
fn an_actions_setting_github_does_not_expose_is_reported_not_swallowed() {
    // `/access` is a private-repository endpoint. On a public one the read
    // 404s, the change is still planned, and the write fails loudly — silence
    // would claim a convergence that never happened.
    let runner = actions_defaults(
        Sandbox::new().config("version: 1\nactions:\n  access_level: organization\n"),
    )
    .respond(
        "PUT",
        "repos/o/r/actions/permissions/access",
        Fixture::error(404, "Not Found"),
    )
    .build();

    runner.run(&["plan", "-R", "o/r"]).expect_status(2);

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("404") || output.stdout.contains("404"),
        "the failure must be visible\nstdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn the_actions_plan_reads_the_way_it_should() {
    let runner = actions_defaults(Sandbox::new().config(
        "version: 1\nactions:\n  allowed_actions: selected\n  selected_actions:\n    \
         patterns_allowed:\n      - docker/*\n  artifact_and_log_retention_days: 30\n",
    ))
    .build();

    let output = runner.run(&["plan", "-R", "o/r", "--verbose"]);
    output.expect_status(2);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn actions_endpoints_that_do_not_apply_are_not_read_as_failures() {
    // The statuses below are what github.com actually answers on a public
    // repository — 409 and 422, never 404. Absorbing only 404 made every plan
    // against a public repository fail, so this pins all three.
    let runner = actions_defaults(
        Sandbox::new().config("version: 1\nactions:\n  artifact_and_log_retention_days: 90\n"),
    )
    .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(
        output.stdout.contains("up to date"),
        "a settings read that does not apply was treated as a failure\n{}\n{}",
        output.stdout,
        output.stderr
    );
}
