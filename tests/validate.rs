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

#[test]
fn a_rule_finding_underlines_the_rule_the_user_wrote() {
    // Rules used to be sorted during normalisation while the finding's path was
    // still a position in the authored document, so the underline landed on
    // whichever rule happened to occupy that index afterwards. The node existed,
    // so the debug assertion passed and the diagnostic was confidently wrong.
    //
    // `pull_request` sorts after `creation`, so the two orders disagree — which
    // is exactly what the single-rule test above cannot catch.
    let output = validate(
        "version: 1\nrulesets:\n  - name: tags\n    target: tag\n    rules:\n      - type: pull_request\n      - type: creation\n",
    );
    output.expect_status(1);

    assert!(
        output.stderr.contains("- type: pull_request\n"),
        "the offending rule must be quoted: {}",
        output.stderr
    );
    assert_snapshot!(output.stderr);
}

// --- the two `Prunable` forms -----------------------------------------------
//
// Every collection section accepts both `labels: [...]` and
// `labels: { prune: true, items: [...] }`. The object form nests items one level
// deeper, and validation used to look under the bare section either way. The
// lookup falls back to the nearest ancestor rather than failing, so the miss
// turned into an underline covering the whole section — confidently wrong, which
// is worse than no underline at all.

#[test]
fn a_bad_label_is_underlined_in_the_object_form() {
    let output = validate(
        "version: 1\nlabels:\n  prune: true\n  items:\n    - name: bug\n      color: NOTHEX\n",
    );
    output.expect_status(1);
    assert!(
        output.stderr.contains("NOTHEX"),
        "the underline should reach the colour: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains(",->"),
        "a span covering several lines means it swallowed the section: {}",
        output.stderr
    );
    assert_snapshot!(output.stderr);
}

#[test]
fn a_bad_label_is_underlined_in_the_list_form() {
    // The form that always worked, kept beside its counterpart so a regression
    // in either is visible next to the other.
    let output = validate("version: 1\nlabels:\n  - name: bug\n    color: NOTHEX\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn a_bad_autolink_is_underlined_in_the_object_form() {
    let output = validate(
        "version: 1\nautolinks:\n  prune: true\n  items:\n    - key_prefix: \"\"\n      url_template: https://example.com/<num>\n",
    );
    output.expect_status(1);
    assert!(
        !output.stderr.contains(",->"),
        "a span covering several lines means it swallowed the section: {}",
        output.stderr
    );
    assert_snapshot!(output.stderr);
}

#[test]
fn a_bad_ruleset_is_underlined_in_the_object_form() {
    let output = validate(
        "version: 1\nrulesets:\n  prune: true\n  items:\n    - name: \"\"\n      rules:\n        - type: non_fast_forward\n",
    );
    output.expect_status(1);
    assert!(
        !output.stderr.contains(",->"),
        "a span covering several lines means it swallowed the section: {}",
        output.stderr
    );
    assert_snapshot!(output.stderr);
}

#[test]
fn a_bad_topic_in_the_safe_settings_spelling_is_underlined_where_it_was_written() {
    // `repository.topics` is folded into `topics` before validation, so the
    // finding names a path that exists in no document. That used to panic a
    // debug build outright and silently lose the underline in a release one —
    // on a configuration that is perfectly legal.
    let output = validate("version: 1\nrepository:\n  topics:\n    - Not A Valid Topic!\n");
    output.expect_status(1);

    assert!(
        output.stderr.contains("Not A Valid Topic!"),
        "the underline must reach the topic the user wrote: {}",
        output.stderr
    );
    assert_snapshot!(output.stderr);
}

#[test]
fn validate_needs_no_repository() {
    // The docs recommend `validate` as a pre-commit hook and as a fork pull
    // request check. Both run somewhere that has no GitHub remote to infer a
    // repository from, and `extends` names its base absolutely, so there is
    // nothing for a repository to contribute to the answer.
    let runner = Sandbox::new()
        .config("version: 1\ntopics:\n  - rust\n")
        .build();

    let output = runner.run(&["validate"]);
    output.expect_status(0);
    assert!(output.stdout.contains("is valid"), "{}", output.stdout);
}

#[test]
fn validate_needs_no_credentials() {
    let runner = Sandbox::new()
        .config("version: 1\ntopics:\n  - rust\n")
        .build();

    let output = runner.run(&["validate"]);
    output.expect_status(0);
    assert!(
        output.requests.is_empty(),
        "validate contacted GitHub: {:?}",
        output.requests
    );
}

#[test]
fn a_reserved_variable_name_is_rejected() {
    // GitHub answers a 409 without explaining, so the check has to happen here.
    let output = validate("version: 1\nvariables:\n  - name: GITHUB_TOKEN\n    value: x\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_invalid_variable_name_points_at_the_name() {
    let output = validate("version: 1\nvariables:\n  - name: 9lives\n    value: x\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn duplicate_variables_are_reported_with_both_locations() {
    let output = validate(
        "version: 1\nvariables:\n  - name: region\n    value: eu\n  - name: REGION\n    value: us\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn a_variable_inside_an_environment_is_underlined_where_it_was_written() {
    let output = validate(
        "version: 1\nenvironments:\n  - name: staging\n    variables:\n      - name: GITHUB_X\n        value: x\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn duplicate_environments_are_reported_with_both_locations() {
    let output = validate("version: 1\nenvironments:\n  - name: staging\n  - name: Staging\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_excessive_wait_timer_is_rejected() {
    let output = validate("version: 1\nenvironments:\n  - name: staging\n    wait_timer: 50000\n");
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn a_reviewer_that_is_both_a_user_and_a_team_is_rejected() {
    let output = validate(
        "version: 1\nenvironments:\n  - name: staging\n    reviewers:\n      - user: octocat\n        team: eng\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn an_empty_custom_branch_policy_is_rejected() {
    let output = validate(
        "version: 1\nenvironments:\n  - name: staging\n    deployment_branch_policy:\n      branches: []\n",
    );
    output.expect_status(1);
    assert_snapshot!(output.stderr);
}

#[test]
fn prevent_self_review_without_reviewers_is_only_a_warning() {
    // It does nothing, silently, which is worth saying — but it breaks nothing.
    let output = validate(
        "version: 1\nenvironments:\n  - name: staging\n    reviewers: []\n    prevent_self_review: true\n",
    );
    output.expect_status(0);
    assert_snapshot!(output.stderr);
}
