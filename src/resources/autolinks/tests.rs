//! Autolink resource tests.

use super::*;
use crate::config::SpanIndex;
use pretty_assertions::assert_eq;

fn state(id: u64, prefix: &str, template: &str, alphanumeric: bool) -> AutolinkState {
    AutolinkState {
        id,
        key_prefix: prefix.to_string(),
        url_template: template.to_string(),
        is_alphanumeric: alphanumeric,
    }
}

fn plan(
    desired_links: Vec<Autolink>,
    current_links: Vec<AutolinkState>,
    prune: bool,
) -> Vec<Change> {
    let desired = Desired {
        autolinks: desired_links.iter().map(Autolink::normalized).collect(),
        prune,
    };
    let current = Current {
        autolinks: current_links
            .into_iter()
            .map(|state| (state.key_prefix.clone(), state))
            .collect(),
    };
    let mut changes = Autolinks.diff(&desired, &current, &PruneOpts::default());
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    changes
}

#[test]
fn creates_missing_autolinks() {
    let changes = plan(
        vec![Autolink::new(
            "OPS-",
            "https://jira.example.com/browse/<num>",
        )],
        vec![],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "create autolink OPS-");
}

#[test]
fn an_identical_autolink_produces_no_change() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://example.com/<num>")],
        vec![state(1, "OPS-", "https://example.com/<num>", true)],
        false,
    );
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn an_omitted_alphanumeric_flag_matches_the_server_default() {
    // GitHub defaults `is_alphanumeric` to true. Without normalisation, every
    // autolink written without the flag would diff forever.
    let changes = plan(
        vec![Autolink::new("OPS-", "https://example.com/<num>")],
        vec![state(1, "OPS-", "https://example.com/<num>", true)],
        false,
    );
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn an_explicit_false_flag_differs_from_the_default() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://example.com/<num>").alphanumeric(false)],
        vec![state(1, "OPS-", "https://example.com/<num>", true)],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Recreate);
}

#[test]
fn a_changed_template_is_a_recreate_not_an_update() {
    // There is no update endpoint for autolinks; modelling this as an update
    // would produce a plan step that cannot be executed.
    let changes = plan(
        vec![Autolink::new("OPS-", "https://new.example.com/<num>")],
        vec![state(7, "OPS-", "https://old.example.com/<num>", true)],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Recreate);
    assert!(changes[0].summary.contains("no update endpoint"));
}

#[test]
fn a_recreate_is_flagged_destructive() {
    // There is a window in which the autolink does not exist, and the plan output
    // must not hide that.
    let changes = plan(
        vec![Autolink::new("OPS-", "https://new.example.com/<num>")],
        vec![state(7, "OPS-", "https://old.example.com/<num>", true)],
        false,
    );
    assert!(changes[0].is_destructive());
}

#[test]
fn a_recreate_carries_both_the_id_to_delete_and_the_link_to_create() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://new.example.com/<num>")],
        vec![state(7, "OPS-", "https://old.example.com/<num>", true)],
        false,
    );
    let payload: Payload = changes[0].decode().unwrap();
    assert_eq!(payload.delete_id, Some(7));
    assert_eq!(
        payload.autolink.unwrap().url_template,
        "https://new.example.com/<num>"
    );
}

#[test]
fn a_recreate_reports_the_field_that_changed() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://new.example.com/<num>")],
        vec![state(7, "OPS-", "https://old.example.com/<num>", true)],
        false,
    );
    assert_eq!(
        changes[0].fields,
        vec![FieldDiff::changed(
            "url_template",
            "https://old.example.com/<num>",
            "https://new.example.com/<num>"
        )]
    );
}

#[test]
fn does_not_delete_unmanaged_autolinks_by_default() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://example.com/<num>")],
        vec![
            state(1, "OPS-", "https://example.com/<num>", true),
            state(2, "OLD-", "https://legacy.example.com/<num>", true),
        ],
        false,
    );
    assert!(changes.is_empty(), "prune is off by default: {changes:#?}");
}

#[test]
fn deletes_unmanaged_autolinks_when_pruning() {
    let changes = plan(
        vec![Autolink::new("OPS-", "https://example.com/<num>")],
        vec![
            state(1, "OPS-", "https://example.com/<num>", true),
            state(2, "OLD-", "https://legacy.example.com/<num>", true),
        ],
        true,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].key, "OLD-");
    assert_eq!(changes[0].decode::<Payload>().unwrap().delete_id, Some(2));
}

#[test]
fn prefixes_are_matched_case_sensitively() {
    // Unlike labels, GitHub treats autolink prefixes as case sensitive.
    let changes = plan(
        vec![Autolink::new("ops-", "https://example.com/<num>")],
        vec![state(1, "OPS-", "https://example.com/<num>", true)],
        false,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
}

#[test]
fn the_create_body_always_states_the_flag_explicitly() {
    let body = Autolink::new("OPS-", "https://example.com/<num>").as_body();
    assert_eq!(body.get("is_alphanumeric"), Some(&Value::Bool(true)));
}

mod validation {
    use super::*;

    fn codes(autolinks: Vec<Autolink>) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        let normalised: Vec<Autolink> = autolinks.iter().map(Autolink::normalized).collect();
        validate(&normalised, &ctx)
            .into_iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn accepts_a_valid_autolink() {
        assert!(
            codes(vec![Autolink::new(
                "OPS-",
                "https://example.com/browse/<num>"
            )])
            .is_empty()
        );
    }

    #[test]
    fn requires_the_num_placeholder() {
        assert!(
            codes(vec![Autolink::new("OPS-", "https://example.com/browse/")])
                .contains(&"gh_settings::autolinks::missing_placeholder".to_string())
        );
    }

    #[test]
    fn requires_an_absolute_url() {
        assert!(
            codes(vec![Autolink::new("OPS-", "/browse/<num>")])
                .contains(&"gh_settings::autolinks::relative_url".to_string())
        );
    }

    #[test]
    fn rejects_duplicate_prefixes() {
        assert!(
            codes(vec![
                Autolink::new("OPS-", "https://a.example.com/<num>"),
                Autolink::new("OPS-", "https://b.example.com/<num>"),
            ])
            .contains(&"gh_settings::autolinks::duplicate".to_string())
        );
    }

    #[test]
    fn rejects_an_empty_prefix() {
        assert!(
            codes(vec![Autolink::new("", "https://example.com/<num>")])
                .contains(&"gh_settings::autolinks::empty_prefix".to_string())
        );
    }

    #[test]
    fn warns_about_shadowing_prefixes() {
        let codes = codes(vec![
            Autolink::new("OPS-", "https://a.example.com/<num>"),
            Autolink::new("OPS-SUB-", "https://b.example.com/<num>"),
        ]);
        assert!(codes.contains(&"gh_settings::autolinks::ambiguous_prefix".to_string()));
    }

    #[test]
    fn shadowing_is_only_a_warning() {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        let findings = validate(
            &[
                Autolink::new("OPS-", "https://a.example.com/<num>").normalized(),
                Autolink::new("OPS-SUB-", "https://b.example.com/<num>").normalized(),
            ],
            &ctx,
        );
        assert!(findings.iter().all(|finding| !finding.is_error()));
    }
}
