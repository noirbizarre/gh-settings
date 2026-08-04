//! End-to-end tests for `validate`.
//!
//! Diagnostics are a product surface, so most of these are snapshots: they lock
//! in the exact rendering, including where the underline points.

mod common;

use common::Sandbox;

/// Run `validate` against a configuration and return the diagnostic output.
fn validate(config: &str) -> common::Output {
    Sandbox::new()
        .config(config)
        .build()
        .run(&["validate", "-R", "o/r"])
}

macro_rules! assert_snapshot {
    ($output:expr) => {
        assert_cli_snapshot!($output)
    };
}

#[test]
fn a_valid_configuration_passes() {
    let output = validate(
        "version: 1\nrepository:\n  description: A repository\ntopics:\n  - rust\nlabels:\n  - name: bug\n    color: d73a4a\n",
    );
    output.expect_status(0);
    assert_snapshot!(output.stdout);
}

#[test]
fn an_empty_configuration_is_valid() {
    validate("{}\n").expect_status(0);
}

#[test]
fn validate_contacts_nobody() {
    // It has to be usable as a pre-commit hook and in a pull request CI job with
    // no repository credentials at all.
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n")
        .build();
    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.requests.is_empty(), "{:?}", output.requests);
}

#[test]
fn an_unknown_section_suggests_a_correction() {
    let output = validate("version: 1\nrepositry:\n  description: x\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_unknown_field_suggests_a_correction() {
    let output = validate("version: 1\nrepository:\n  descriptoin: x\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn duplicate_labels_are_reported_with_both_locations() {
    let output = validate(
        "version: 1\nlabels:\n  - name: bug\n    color: d73a4a\n  - name: BUG\n    color: cccccc\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_invalid_colour_points_at_the_colour() {
    let output = validate("version: 1\nlabels:\n  - name: bug\n    color: nothex\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_autolink_without_a_placeholder_is_rejected() {
    let output = validate(
        "version: 1\nautolinks:\n  - key_prefix: OPS-\n    url_template: https://example.com/browse\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn disabling_every_merge_strategy_is_rejected() {
    let output = validate(
        "version: 1\nrepository:\n  allow_merge_commit: false\n  allow_squash_merge: false\n  allow_rebase_merge: false\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn topics_declared_twice_are_rejected() {
    let output = validate("version: 1\ntopics:\n  - a\nrepository:\n  topics:\n    - b\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn every_problem_is_reported_at_once() {
    // Fixing configuration one error per run is a miserable experience.
    let output = validate(
        "version: 1\nlabels:\n  - name: bug\n    color: zzz\n  - name: BUG\n    color: qqq\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn warnings_alone_do_not_fail() {
    // A scheme-less homepage is worth mentioning but must not block a sync.
    let output = validate("version: 1\nrepository:\n  homepage: example.com\n");
    output.expect_status(0);
    assert_snapshot!(output.stderr);
}

#[test]
fn strict_turns_warnings_into_failures() {
    validate("version: 1\nrepository:\n  homepage: example.com\n").expect_status(0);

    Sandbox::new()
        .config("version: 1\nrepository:\n  homepage: example.com\n")
        .build()
        .run(&["validate", "-R", "o/r", "--strict"])
        .expect_status(1);
}

#[test]
fn a_missing_version_is_only_a_warning() {
    let output = validate("labels:\n  - name: bug\n    color: d73a4a\n");
    output.expect_status(0);
    assert!(output.stderr.contains("version"), "{}", output.stderr);
}

#[test]
fn an_unsupported_version_is_an_error() {
    validate("version: 99\nlabels: []\n").expect_status(1);
}

#[test]
fn malformed_yaml_is_reported_clearly() {
    let output = validate("labels:\n  - name: bug\n   color: broken indent\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn a_missing_configuration_file_explains_what_to_do() {
    let runner = Sandbox::new().build();
    std::fs::remove_file(runner.path().join(".github/settings.yml")).ok();

    let output = runner.run(&["validate", "-R", "o/r"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("gh settings export"),
        "{}",
        output.stderr
    );
}

#[test]
fn json_output_is_machine_readable() {
    let runner = Sandbox::new()
        .config("version: 1\nlabels:\n  - name: bug\n    color: zzz\n")
        .build();
    let output = runner.run(&["validate", "-R", "o/r", "--format", "json"]);
    output.expect_status(1);

    let value: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(value["valid"], false);
    assert_eq!(
        value["findings"][0]["code"],
        "gh_settings::labels::invalid_color"
    );
}

#[test]
fn an_unknown_rule_type_is_only_a_warning() {
    // GitHub ships new rule types continuously; refusing them would make this
    // tool a blocker rather than a help.
    let output = validate(
        "version: 1\nrulesets:\n  - name: main\n    rules:\n      - type: some_future_rule\n",
    );
    output.expect_status(0);
    assert!(
        output.stderr.contains("some_future_rule"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_misspelled_rule_type_suggests_a_correction() {
    let output =
        validate("version: 1\nrulesets:\n  - name: main\n    rules:\n      - type: pull_requst\n");
    output.expect_status(0);
    assert!(output.stderr.contains("pull_request"), "{}", output.stderr);
}

#[test]
fn a_branch_only_rule_on_a_tag_ruleset_is_rejected() {
    let output = validate(
        "version: 1\nrulesets:\n  - name: tags\n    target: tag\n    rules:\n      - type: pull_request\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}
