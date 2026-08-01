//! End-to-end tests for `export`.
//!
//! The acceptance criterion for export is the round trip: `export` followed by
//! `plan` must report no changes. Anything less makes migrating an existing
//! repository a manual clean-up job.

mod common;

use common::{Sandbox, default_repository};

fn populated() -> Sandbox {
    Sandbox::new()
        .repository(&default_repository())
        .get("repos/o/r/topics", r#"{"names": ["rust", "github-cli"]}"#)
        .get(
            "repos/o/r/labels",
            r#"[{"name": "bug", "color": "d73a4a", "description": "Something isn't working"}]"#,
        )
        .get(
            "repos/o/r/autolinks",
            r#"[{"id": 1, "key_prefix": "OPS-", "url_template": "https://jira.example.com/browse/<num>", "is_alphanumeric": false}]"#,
        )
}

#[test]
fn writes_the_conventional_location() {
    let runner = populated().build();
    runner.run(&["export", "-R", "o/r"]).expect_status(0);

    let written = common::read(runner.path(), ".github/settings.yml");
    assert!(written.contains("version: 1"), "{written}");
    assert!(written.contains("bug"), "{written}");
    assert!(written.contains("OPS-"), "{written}");
}

#[test]
fn includes_the_schema_annotation() {
    // This one line is what gives users completion and validation in their
    // editor, so it must survive every export.
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);

    assert!(
        output.stdout.contains(
            "# yaml-language-server: $schema=https://gh-settings.dev/schema/v1/settings.json"
        ),
        "{}",
        output.stdout
    );
}

#[test]
fn exported_configuration_round_trips_to_an_empty_plan() {
    // The acceptance criterion for the whole command.
    let runner = populated().build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn exported_configuration_is_valid() {
    let runner = populated().build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);
    runner.run(&["validate", "-R", "o/r"]).expect_status(0);
}

#[test]
fn refuses_to_overwrite_without_force() {
    // Export cannot preserve comments, so silently replacing a hand-written file
    // would destroy work.
    let runner = populated()
        .config("version: 1\n# a treasured comment\n")
        .build();

    let output = runner.run(&["export", "-R", "o/r"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("--force"),
        "stderr:\n{}",
        output.stderr
    );
    assert!(
        common::read(runner.path(), ".github/settings.yml").contains("treasured"),
        "the existing file was modified"
    );
}

#[test]
fn force_overwrites() {
    let runner = populated()
        .config("version: 1\n# a treasured comment\n")
        .build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    assert!(!common::read(runner.path(), ".github/settings.yml").contains("treasured"));
}

#[test]
fn omits_sections_the_repository_has_nothing_for() {
    // An exported file full of `topics: []` is noise, and with pruning enabled it
    // would also be a loaded gun.
    let runner = Sandbox::new()
        .repository(&default_repository())
        .get("repos/o/r/topics", r#"{"names": []}"#)
        .get("repos/o/r/labels", "[]")
        .get("repos/o/r/autolinks", "[]")
        .build();

    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(!output.stdout.contains("topics:"), "{}", output.stdout);
    assert!(!output.stdout.contains("labels:"), "{}", output.stdout);
}

#[test]
fn never_exports_the_archived_flag() {
    // A one-way, destructive flag has no business in a file people copy between
    // repositories.
    let runner = Sandbox::new()
        .repository(
            &common::repository_with(&[("description", "x")])
                .replace(r#""archived":false"#, r#""archived":true"#),
        )
        .build();

    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(!output.stdout.contains("archived"), "{}", output.stdout);
}

#[test]
fn export_performs_no_writes() {
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn only_restricts_what_is_exported() {
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout", "--only", "labels"]);
    output.expect_status(0);

    assert!(output.stdout.contains("labels:"), "{}", output.stdout);
    assert!(!output.stdout.contains("autolinks:"), "{}", output.stdout);
}
