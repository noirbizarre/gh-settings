//! Label resource tests.
//!
//! The diff is a pure function, so almost everything worth testing about this
//! resource can be tested without a runtime, a network or a stub.

use super::*;
use crate::config::{Prunable, SpanIndex};
use crate::resources::FieldDiff;
use pretty_assertions::assert_eq;
use rstest::rstest;

fn desired(labels: Vec<Label>, prune: bool) -> Desired {
    Desired {
        labels: labels.iter().map(Label::normalized).collect(),
        prune,
    }
}

fn current(labels: Vec<Label>) -> Current {
    Current {
        labels: labels
            .into_iter()
            .map(|label| (model::key(&label.name), label.normalized()))
            .collect(),
    }
}

fn plan(desired_labels: Vec<Label>, current_labels: Vec<Label>, prune: bool) -> Vec<Change> {
    let mut changes = Labels.diff(
        &desired(desired_labels, prune),
        &current(current_labels),
        &PruneOpts::default(),
    );
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    changes
}

#[test]
fn creates_missing_labels() {
    let changes = plan(vec![Label::new("bug", "d73a4a")], vec![], false);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "create label bug");
}

#[test]
fn an_identical_label_produces_no_change() {
    let label = Label::new("bug", "d73a4a").with_description("Something isn't working");
    assert!(plan(vec![label.clone()], vec![label], false).is_empty());
}

#[rstest]
#[case("#D73A4A", "d73a4a")]
#[case("D73A4A", "d73a4a")]
#[case("  #d73a4a  ", "d73a4a")]
fn colours_are_normalised_to_github_storage_form(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(model::normalize_color(input), expected);
}

#[test]
fn a_colour_differing_only_in_case_is_not_a_change() {
    // Without normalisation this is the classic permanent diff: the user writes
    // `#D73A4A`, GitHub stores `d73a4a`, and every run reports an update.
    let changes = plan(
        vec![Label::new("bug", "#D73A4A")],
        vec![Label::new("bug", "d73a4a")],
        false,
    );
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn an_omitted_description_is_never_cleared() {
    // Omission means unmanaged. Clearing here would silently destroy data on the
    // first run against an existing repository.
    let changes = plan(
        vec![Label::new("bug", "d73a4a")],
        vec![Label::new("bug", "d73a4a").with_description("existing")],
        false,
    );
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn an_empty_description_normalises_to_absent() {
    let label = Label {
        name: "bug".into(),
        color: "d73a4a".into(),
        description: Some("   ".into()),
        new_name: None,
    };
    assert_eq!(label.normalized().description, None);
}

#[test]
fn updates_a_changed_colour() {
    let changes = plan(
        vec![Label::new("bug", "b60205")],
        vec![Label::new("bug", "d73a4a")],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);
    assert_eq!(
        changes[0].fields,
        vec![FieldDiff::changed("color", "d73a4a", "b60205")]
    );
}

#[test]
fn matching_is_case_insensitive() {
    // GitHub rejects `Bug` and `bug` coexisting, so treating them as distinct
    // would plan a create that is guaranteed to fail with a 422.
    let changes = plan(
        vec![Label::new("Bug", "d73a4a")],
        vec![Label::new("bug", "d73a4a")],
        false,
    );
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn does_not_delete_unmanaged_labels_by_default() {
    let changes = plan(
        vec![Label::new("bug", "d73a4a")],
        vec![Label::new("bug", "d73a4a"), Label::new("legacy", "cccccc")],
        false,
    );
    assert!(changes.is_empty(), "prune is off by default: {changes:#?}");
}

#[test]
fn deletes_unmanaged_labels_when_pruning() {
    let changes = plan(
        vec![Label::new("bug", "d73a4a")],
        vec![Label::new("bug", "d73a4a"), Label::new("legacy", "cccccc")],
        true,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].key, "legacy");
    assert!(changes[0].is_destructive());
}

#[test]
fn the_command_line_can_force_pruning_off() {
    let changes = Labels.diff(
        &desired(vec![Label::new("bug", "d73a4a")], true),
        &current(vec![Label::new("legacy", "cccccc")]),
        &PruneOpts { force: Some(false) },
    );
    assert!(
        changes.iter().all(|change| change.op != Op::Delete),
        "--no-prune must win over the configuration"
    );
}

#[test]
fn renames_instead_of_recreating() {
    // A rename preserves the label's issue assignments; delete-then-create would
    // silently unlabel every issue.
    let changes = plan(
        vec![Label::new("bug", "d73a4a").renamed_to("defect")],
        vec![Label::new("bug", "d73a4a")],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);
    assert_eq!(changes[0].summary, "rename label bug to defect");
    assert!(!changes[0].is_destructive());

    let payload: Payload = changes[0].decode().unwrap();
    assert_eq!(payload.from.as_deref(), Some("bug"));
    assert_eq!(payload.label.name, "defect");
}

#[test]
fn renaming_a_missing_label_creates_it_under_the_new_name() {
    let changes = plan(
        vec![Label::new("bug", "d73a4a").renamed_to("defect")],
        vec![],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].key, "defect");
}

#[test]
fn a_completed_rename_is_idempotent() {
    // Second run: the label already carries the new name, so `bug` no longer
    // exists and `defect` matches. Nothing should happen, and crucially `defect`
    // must not be pruned.
    let changes = plan(
        vec![Label::new("bug", "d73a4a").renamed_to("defect")],
        vec![Label::new("defect", "d73a4a")],
        false,
    );
    assert_eq!(changes.len(), 1, "{changes:#?}");
    assert_eq!(changes[0].op, Op::Create);
}

#[test]
fn update_bodies_only_carry_new_name_on_an_actual_rename() {
    let label = Label::new("defect", "d73a4a");
    let renaming = label.as_update_body("bug");
    assert_eq!(
        renaming.get("new_name").and_then(|v| v.as_str()),
        Some("defect")
    );

    let not_renaming = label.as_update_body("defect");
    assert!(not_renaming.get("new_name").is_none());
}

#[rstest]
#[case("bug", "bug")]
#[case("good first issue", "good%20first%20issue")]
#[case("area/docs", "area%2Fdocs")]
#[case("priority:high", "priority%3Ahigh")]
fn label_names_are_url_encoded(#[case] name: &str, #[case] expected: &str) {
    // Spaces and slashes are extremely common in label names and would otherwise
    // corrupt the endpoint path.
    assert_eq!(urlencode(name), expected);
}

mod api_shape {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn decodes_a_real_api_payload() {
        // The API returns fields that are not configuration. Decoding through
        // the strict configuration type would fail on every real repository —
        // as it did, until this test existed.
        let payload = r#"{
            "id": 208045946,
            "node_id": "MDU6TGFiZWwyMDgwNDU5NDY=",
            "url": "https://api.github.com/repos/o/r/labels/bug",
            "name": "bug",
            "color": "d73a4a",
            "default": true,
            "description": "Something isn't working"
        }"#;

        let state: LabelState = serde_json::from_str(payload).expect("should decode");
        assert_eq!(state.name, "bug");

        let label = state.as_label();
        assert_eq!(label.color, "d73a4a");
        assert_eq!(
            label.description.as_deref(),
            Some("Something isn't working")
        );
    }

    #[test]
    fn a_null_description_becomes_absent() {
        let state: LabelState =
            serde_json::from_str(r#"{"name": "bug", "color": "d73a4a", "description": null}"#)
                .unwrap();
        assert_eq!(state.as_label().description, None);
    }

    #[test]
    fn the_configuration_type_still_rejects_typos() {
        // The strictness that made the two types necessary must not be lost.
        assert!(serde_norway::from_str::<Label>("name: bug\ncolour: d73a4a\n").is_err());
    }
}

mod validation {
    use super::*;
    use pretty_assertions::assert_eq;

    fn findings(labels: Vec<Label>) -> Vec<Finding> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        let normalised: Vec<Label> = labels.iter().map(Label::normalized).collect();
        model::validate(&normalised, &ctx)
    }

    fn codes(labels: Vec<Label>) -> Vec<String> {
        findings(labels).into_iter().map(|f| f.code).collect()
    }

    #[test]
    fn accepts_a_valid_set() {
        assert!(codes(vec![Label::new("bug", "d73a4a")]).is_empty());
    }

    #[test]
    fn rejects_duplicates_case_insensitively() {
        let codes = codes(vec![
            Label::new("bug", "d73a4a"),
            Label::new("BUG", "cccccc"),
        ]);
        assert_eq!(codes, vec!["gh_settings::labels::duplicate"]);
    }

    #[rstest]
    #[case("xyz")]
    #[case("d73a4")]
    #[case("d73a4aa")]
    #[case("zzzzzz")]
    fn rejects_malformed_colours(#[case] color: &str) {
        assert_eq!(
            codes(vec![Label::new("bug", color)]),
            vec!["gh_settings::labels::invalid_color"]
        );
    }

    #[test]
    fn accepts_a_hash_prefixed_colour() {
        assert!(codes(vec![Label::new("bug", "#d73a4a")]).is_empty());
    }

    #[test]
    fn rejects_an_empty_name() {
        assert!(
            codes(vec![Label::new("  ", "d73a4a")])
                .contains(&"gh_settings::labels::empty_name".to_string())
        );
    }

    #[test]
    fn rejects_an_overlong_description() {
        let label = Label::new("bug", "d73a4a").with_description("x".repeat(101));
        assert_eq!(
            codes(vec![label]),
            vec!["gh_settings::labels::description_too_long"]
        );
    }

    #[test]
    fn accepts_a_description_at_the_limit() {
        let label = Label::new("bug", "d73a4a").with_description("x".repeat(100));
        assert!(codes(vec![label]).is_empty());
    }

    #[test]
    fn rejects_a_rename_that_collides_with_another_entry() {
        let codes = codes(vec![
            Label::new("bug", "d73a4a").renamed_to("defect"),
            Label::new("defect", "cccccc"),
        ]);
        assert!(codes.contains(&"gh_settings::labels::rename_collision".to_string()));
    }

    #[test]
    fn warns_about_a_rename_to_the_same_name() {
        let findings = findings(vec![Label::new("bug", "d73a4a").renamed_to("bug")]);
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].is_error(), "should be advisory only");
    }

    #[test]
    fn reports_every_problem_at_once() {
        // Users should not have to fix errors one run at a time.
        let codes = codes(vec![Label::new("bug", "zzz"), Label::new("bug", "cccccc")]);
        assert!(codes.len() >= 2, "{codes:?}");
    }
}

mod section {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_bare_list_does_not_prune() {
        let section: Prunable<Label> =
            serde_norway::from_str("- name: bug\n  color: d73a4a\n").unwrap();
        assert!(!section.prune());
        assert_eq!(section.items().len(), 1);
    }

    #[test]
    fn the_object_form_can_opt_into_pruning() {
        let section: Prunable<Label> =
            serde_norway::from_str("prune: true\nitems:\n  - name: bug\n    color: d73a4a\n")
                .unwrap();
        assert!(section.prune());
        assert_eq!(section.items().len(), 1);
    }

    #[test]
    fn an_empty_list_is_managed_but_empty() {
        // Distinct from an absent section: with prune on, this means "no labels".
        let section: Prunable<Label> = serde_norway::from_str("[]").unwrap();
        assert!(section.items().is_empty());
    }
}
