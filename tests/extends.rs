//! End-to-end tests for `extends:`.
//!
//! A configuration may inherit from another repository. The two properties that
//! matter most here are that nothing is fetched unless the file asks for it, and
//! that a finding about the inherited document renders against *that* document's
//! text rather than the local one's.

mod common;

use common::{Fixture, Sandbox, default_repository};

/// The endpoint a base reference resolves to.
const BASE_ENDPOINT: &str = "repos/acme/.github/contents/.github/settings.yml?ref=v1";

fn with_base(sandbox: Sandbox, base: &str) -> Sandbox {
    sandbox.respond("GET", BASE_ENDPOINT, Fixture::ok(base))
}

#[test]
fn a_configuration_that_does_not_inherit_contacts_nobody() {
    // `validate` is documented as working in a pull request with no
    // credentials. Inheritance makes that conditional, so the unconditional part
    // is worth enforcing rather than describing.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(0);
    assert!(
        output.requests.is_empty(),
        "nothing should have been fetched: {:?}",
        output.requests
    );
}

#[test]
fn an_inherited_label_is_planned_as_if_it_were_local() {
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "labels:\n  - name: bug\n    color: d73a4a\n",
    )
    .get("repos/o/r/labels", "[]")
    .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert!(
        output.stdout.contains("create label bug"),
        "{}",
        output.stdout
    );
}

#[test]
fn the_base_is_read_at_the_ref_that_was_pinned() {
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "version: 1\n",
    )
    .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(0);
    assert!(
        output
            .requests
            .iter()
            .any(|request| request.contains(BASE_ENDPOINT)),
        "{:?}",
        output.requests
    );
}

#[test]
fn a_local_declaration_overrides_the_inherited_one() {
    let runner = with_base(
        Sandbox::new().config(
            "version: 1\nextends: acme/.github@v1\nlabels:\n  - name: bug\n    color: ff0000\n",
        ),
        "labels:\n  - name: bug\n    color: d73a4a\n",
    )
    .get("repos/o/r/labels", "[]")
    .respond("POST", "repos/o/r/labels", Fixture::created("{}"))
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert_eq!(writes.len(), 1, "one label, not two: {writes:?}");
    assert!(
        writes[0].contains("ff0000"),
        "the local colour wins: {writes:?}"
    );
}

#[test]
fn pruning_declared_by_the_base_does_not_delete_anything_locally() {
    // The property that keeps a shared file safe: editing it must not start
    // deleting across every repository that extends it.
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "labels:\n  prune: true\n  items:\n    - name: bug\n      color: d73a4a\n",
    )
    .get(
        "repos/o/r/labels",
        r#"[{"name": "bug", "color": "d73a4a"}, {"name": "local-only", "color": "cccccc"}]"#,
    )
    .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(
        !output.stdout.contains("local-only"),
        "the base's prune must not reach this repository: {}",
        output.stdout
    );
}

#[test]
fn a_finding_about_the_base_is_rendered_against_the_base() {
    // The reason the whole provenance mechanism exists: the offending text lives
    // in a different file from the one being configured.
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "labels:\n  - name: bug\n    color: NOTHEXAT\n",
    )
    .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(1);

    assert!(
        output.stderr.contains("acme/.github@v1"),
        "the inherited document must be named: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("NOTHEXAT"),
        "and its own text quoted: {}",
        output.stderr
    );
}

#[test]
fn a_base_that_itself_inherits_is_rejected() {
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "extends: other/base@v1\n",
    )
    .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(1);
    assert!(
        output
            .stderr
            .contains("nested inheritance is not supported"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("inherited from acme/.github@v1"),
        "the complaint is about the base, so it must render against the base: {}",
        output.stderr
    );
}

#[test]
fn an_unpinned_reference_is_rejected_without_fetching() {
    let runner = Sandbox::new()
        .config("version: 1\nextends: acme/.github\n")
        .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(1);
    assert!(output.stderr.contains("no ref"), "{}", output.stderr);
    assert!(
        output.requests.is_empty(),
        "a reference that cannot be parsed should not be fetched: {:?}",
        output.requests
    );
}

#[test]
fn an_unreadable_base_names_the_base_and_the_permission() {
    let runner = Sandbox::new()
        .config("version: 1\nextends: acme/.github@v1\n")
        .respond("GET", BASE_ENDPOINT, Fixture::error(404, "Not Found"))
        .build();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("acme/.github@v1"),
        "{}",
        output.stderr
    );
}

#[test]
fn inherited_repository_settings_merge_field_by_field() {
    let runner = with_base(
        Sandbox::new()
            .config("version: 1\nextends: acme/.github@v1\nrepository:\n  description: local\n"),
        "repository:\n  description: inherited\n  homepage: https://example.com\n",
    )
    .repository(&default_repository())
    .accept("PATCH", "repos/o/r")
    .build();

    let output = runner.run(&["sync", "-R", "o/r", "--yes"]);
    output.expect_status(0);

    let writes = output.writes();
    assert!(writes[0].contains("local"), "{writes:?}");
    assert!(
        writes[0].contains("https://example.com"),
        "a field the local file never mentioned stays inherited: {writes:?}"
    );
}

#[test]
fn a_saved_plan_reports_a_moved_base_as_a_moved_base() {
    // Not as drift. The repository being configured has not changed at all, and
    // a shared base is usually owned by someone else — sending people to look at
    // their own repository would waste the one thing the message is for.
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "labels:\n  - name: bug\n    color: d73a4a\n",
    )
    .get("repos/o/r/labels", "[]")
    .build();

    let plan_path = runner.path().join("plan.json");
    let plan_arg = plan_path.display().to_string();

    runner
        .run(&["plan", "-R", "o/r", "--out", &plan_arg])
        .expect_status(2);

    // The base is served again with different content, as a moving ref would.
    let moved = Sandbox::new()
        .config("version: 1\nextends: acme/.github@v1\n")
        .respond(
            "GET",
            BASE_ENDPOINT,
            Fixture::ok("labels:\n  - name: chore\n    color: cccccc\n")
                .header("etag", "\"moved\""),
        )
        .get("repos/o/r/labels", "[]")
        .build();

    std::fs::copy(&plan_path, moved.path().join("plan.json")).expect("copy the saved plan");
    let output = moved.run(&[
        "sync",
        "-R",
        "o/r",
        "--yes",
        "--plan",
        &moved.path().join("plan.json").display().to_string(),
    ]);

    output.expect_status(1);
    assert!(
        output.stderr.contains("acme/.github@v1"),
        "the message must name the base that moved: {}",
        output.stderr
    );
    assert!(
        output.writes().is_empty(),
        "nothing should have been applied: {:?}",
        output.writes()
    );
}

#[test]
fn the_plan_says_where_inherited_changes_came_from() {
    // Half a plan can originate in a file the reader does not own. The JSON
    // artifact has always recorded the base; the human rendering did not, which
    // left the reader nothing to attribute a surprising change to.
    let runner = with_base(
        Sandbox::new().config("version: 1\nextends: acme/.github@v1\n"),
        "labels:\n  - name: bug\n    color: d73a4a\n",
    )
    .get("repos/o/r/labels", "[]")
    .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert!(
        output.stdout.contains("Inherits from acme/.github@v1"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_plan_that_inherits_nothing_says_nothing_about_inheritance() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .get("repos/o/r/labels", "[]")
        .build();

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(2);
    assert!(
        !output.stdout.contains("Inherits from"),
        "{}",
        output.stdout
    );
}
