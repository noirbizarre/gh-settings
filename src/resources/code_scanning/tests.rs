//! Code scanning resource tests.

use super::*;
use crate::config::SpanIndex;
use rstest::rstest;

fn settings(yaml: &str) -> CodeScanningSettings {
    serde_norway::from_str(yaml).expect("valid code_scanning section")
}

fn state(json: Value) -> DefaultSetupState {
    // Through `normalized`, as the real `current()` does.
    serde_json::from_value::<DefaultSetupState>(json)
        .expect("valid default setup state")
        .normalized()
}

/// A repository that never enabled default setup, as GitHub reports it: the
/// languages it detected, and the settings it would use.
fn never_configured() -> DefaultSetupState {
    state(json!({
        "state": "not-configured",
        "languages": ["actions", "javascript", "javascript-typescript", "python", "rust", "typescript"],
        "query_suite": "default",
        "threat_model": "remote",
        "runner_type": "standard",
        "runner_label": null,
    }))
}

/// A configured setup, read back after its setup run finished.
fn configured() -> DefaultSetupState {
    state(json!({
        "state": "configured",
        "languages": ["actions", "javascript", "javascript-typescript", "python", "typescript"],
        "query_suite": "default",
        "threat_model": "remote",
        "runner_type": "standard",
        "runner_label": "",
    }))
}

fn plan(desired: &str, current: Current) -> Vec<Change> {
    CodeScanning.diff(&settings(desired), &current, &PruneOpts::default())
}

fn body(change: &Change) -> Value {
    change
        .decode::<Payload>()
        .expect("a decodable payload")
        .body
}

mod normalisation {
    use super::*;
    use pretty_assertions::assert_eq;

    #[rstest]
    #[case("javascript", "javascript-typescript")]
    #[case("typescript", "javascript-typescript")]
    #[case("javascript-typescript", "javascript-typescript")]
    #[case("c", "c-cpp")]
    #[case("cpp", "c-cpp")]
    #[case("java", "java-kotlin")]
    #[case("kotlin", "java-kotlin")]
    #[case(" Python ", "python")]
    #[case("actions", "actions")]
    fn every_alias_of_a_language_folds_into_the_name_the_api_accepts(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(model::normalize_language(input), expected);
    }

    #[test]
    fn the_aliases_github_reports_collapse_into_one_language() {
        assert_eq!(
            configured().languages,
            vec!["actions", "javascript-typescript", "python"]
        );
    }

    #[test]
    fn an_empty_runner_label_reads_as_unset() {
        assert_eq!(configured().runner_label, None);
        assert_eq!(never_configured().runner_label, None);
    }
}

mod diff {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_empty_section_produces_no_change() {
        assert!(plan("{}", Some(configured())).is_empty());
    }

    #[test]
    fn enabling_sends_the_configured_state() {
        let changes = plan(
            "default_setup:\n  enabled: true\n",
            Some(never_configured()),
        );
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].op, Op::Update);
        assert_eq!(changes[0].summary, "enable CodeQL default setup");
        assert_eq!(body(&changes[0]), json!({"state": "configured"}));
    }

    #[test]
    fn enabling_sends_every_declared_setting_even_where_github_already_agrees() {
        // A disabled setup's settings describe nothing that runs, so matching
        // them proves nothing about what the enabled setup will use.
        let changes = plan(
            "default_setup:\n  enabled: true\n  query_suite: default\n  languages: [python]\n",
            Some(never_configured()),
        );
        assert_eq!(
            body(&changes[0]),
            json!({"state": "configured", "query_suite": "default", "languages": ["python"]})
        );
    }

    #[test]
    fn a_repository_without_the_endpoint_is_treated_as_disabled() {
        let changes = plan("default_setup:\n  enabled: true\n", None);
        assert_eq!(changes[0].summary, "enable CodeQL default setup");
    }

    #[test]
    fn an_enabled_setup_that_matches_produces_no_change() {
        assert!(plan("default_setup:\n  enabled: true\n", Some(configured())).is_empty());
    }

    #[test]
    fn languages_are_compared_through_their_aliases() {
        // GitHub reads `javascript-typescript` back as three names; a plan that
        // compared them literally would never come out empty.
        let yaml = "default_setup:\n  enabled: true\n  languages: [typescript, python, actions]\n";
        assert!(plan(yaml, Some(configured())).is_empty());
    }

    #[test]
    fn a_changed_language_list_is_sent_in_full() {
        let changes = plan(
            "default_setup:\n  enabled: true\n  languages: [actions]\n",
            Some(configured()),
        );
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].summary, "update CodeQL default setup languages");
        assert_eq!(
            body(&changes[0]),
            json!({"state": "configured", "languages": ["actions"]})
        );
    }

    #[test]
    fn only_the_settings_that_differ_are_sent_to_an_enabled_setup() {
        let changes = plan(
            "default_setup:\n  enabled: true\n  query_suite: extended\n  threat_model: remote\n",
            Some(configured()),
        );
        assert_eq!(
            body(&changes[0]),
            json!({"state": "configured", "query_suite": "extended"})
        );
        assert_eq!(changes[0].fields.len(), 1);
        assert_eq!(changes[0].fields[0].before.as_deref(), Some("default"));
    }

    #[test]
    fn disabling_sends_only_the_state() {
        let changes = plan("default_setup:\n  enabled: false\n", Some(configured()));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].summary, "disable CodeQL default setup");
        assert_eq!(body(&changes[0]), json!({"state": "not-configured"}));
    }

    #[test]
    fn a_disabled_setup_that_should_stay_disabled_produces_no_change() {
        assert!(
            plan(
                "default_setup:\n  enabled: false\n",
                Some(never_configured())
            )
            .is_empty()
        );
    }

    #[test]
    fn disabling_is_not_destructive() {
        // Turning a setting off is an update the file asked for, not a prune.
        let changes = plan("default_setup:\n  enabled: false\n", Some(configured()));
        assert!(!changes[0].op.is_destructive());
    }

    #[test]
    fn applying_then_replanning_yields_nothing() {
        // What GitHub reports once the setup run has finished, fed back in.
        let yaml = "default_setup:\n  enabled: true\n  query_suite: extended\n  threat_model: remote_and_local\n  languages: [actions, javascript]\n";
        let after = state(json!({
            "state": "configured",
            "languages": ["actions", "javascript", "javascript-typescript", "typescript"],
            "query_suite": "extended",
            "threat_model": "remote_and_local",
            "runner_type": "standard",
            "runner_label": "",
        }));
        assert!(plan(yaml, Some(after)).is_empty());
    }
}

mod validation {
    use super::*;
    use pretty_assertions::assert_eq;

    fn codes(yaml: &str) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        CodeScanning
            .validate(&settings(yaml), &ctx)
            .into_iter()
            .map(|finding| finding.code)
            .collect()
    }

    #[test]
    fn settings_alongside_a_disabled_setup_are_flagged() {
        assert_eq!(
            codes("default_setup:\n  enabled: false\n  query_suite: extended\n"),
            vec!["gh_settings::code_scanning::ignored_when_disabled"]
        );
    }

    #[test]
    fn an_empty_language_list_is_rejected() {
        assert_eq!(
            codes("default_setup:\n  enabled: true\n  languages: []\n"),
            vec!["gh_settings::code_scanning::no_languages"]
        );
    }

    #[test]
    fn labeled_runners_need_a_label() {
        assert_eq!(
            codes("default_setup:\n  enabled: true\n  runner_type: labeled\n"),
            vec!["gh_settings::code_scanning::missing_runner_label"]
        );
    }

    #[test]
    fn a_label_needs_labeled_runners() {
        assert_eq!(
            codes("default_setup:\n  enabled: true\n  runner_label: code-scanning\n"),
            vec!["gh_settings::code_scanning::unused_runner_label"]
        );
    }

    #[test]
    fn a_complete_section_is_silent() {
        let yaml = "default_setup:\n  enabled: true\n  query_suite: extended\n  threat_model: remote\n  languages: [python]\n  runner_type: labeled\n  runner_label: code-scanning\n";
        assert!(codes(yaml).is_empty());
    }

    #[test]
    fn enabled_is_required() {
        assert!(serde_norway::from_str::<CodeScanningSettings>("default_setup: {}\n").is_err());
    }
}
