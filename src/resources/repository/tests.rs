//! Repository resource tests.

use super::*;
use crate::config::Settings;
use pretty_assertions::assert_eq;
use serde_json::json;

fn desired(source: &str) -> RepositorySettings {
    serde_norway::from_str(source).unwrap()
}

fn current(value: Value) -> Current {
    // Through `normalized`, as the real `current()` does: a test that compares
    // against an unnormalised value is not testing what the tool runs.
    serde_json::from_value::<Current>(value)
        .unwrap()
        .normalized()
}

fn plan(desired_source: &str, current_value: Value) -> Vec<Change> {
    Repository.diff(
        &desired(desired_source),
        &current(current_value),
        &PruneOpts::default(),
    )
}

fn body(change: &Change) -> Value {
    match change.decode::<Payload>().unwrap() {
        Payload::Settings(body) | Payload::Security(body) => body,
    }
}

#[test]
fn an_empty_section_produces_no_change() {
    assert!(plan("{}", json!({"description": "hello"})).is_empty());
}

#[test]
fn an_identical_description_produces_no_change() {
    assert!(plan("description: hello", json!({"description": "hello"})).is_empty());
}

#[test]
fn updates_a_changed_description() {
    let changes = plan("description: new", json!({"description": "old"}));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);
    assert_eq!(changes[0].summary, "update repository description");
    assert_eq!(body(&changes[0]), json!({"description": "new"}));
}

#[test]
fn treats_an_empty_api_description_as_unset() {
    // GitHub reports "no description" as "" rather than null; without this the
    // plan would forever show a change.
    assert!(plan("description: null", json!({"description": ""})).is_empty());
}

#[test]
fn clears_a_description_on_explicit_null() {
    let changes = plan("description: null", json!({"description": "old"}));
    assert_eq!(body(&changes[0]), json!({"description": ""}));
}

#[test]
fn never_touches_an_omitted_field() {
    // The single most important property of this resource: a file that only sets
    // `homepage` must not wipe the description.
    let changes = plan(
        "homepage: https://example.com",
        json!({"description": "keep me", "homepage": ""}),
    );
    assert_eq!(changes.len(), 1);
    assert!(body(&changes[0]).get("description").is_none());
}

#[test]
fn batches_every_field_into_one_request() {
    let changes = plan(
        "has_issues: true\nhas_wiki: false\ndelete_branch_on_merge: true",
        json!({"has_issues": false, "has_wiki": true, "delete_branch_on_merge": false}),
    );
    assert_eq!(changes.len(), 1, "should be a single PATCH");
    assert_eq!(changes[0].fields.len(), 3);
    assert_eq!(changes[0].summary, "update repository (3 fields)");
}

#[test]
fn a_matching_boolean_produces_no_change() {
    assert!(plan("has_issues: true", json!({"has_issues": true})).is_empty());
}

#[test]
fn archiving_is_applied_but_unarchiving_is_not() {
    // GitHub accepts `archived: true` but cannot unarchive through the API, so
    // planning that change would produce a step that can never succeed.
    let archiving = plan("archived: true", json!({"archived": false}));
    assert_eq!(archiving.len(), 1);
    assert_eq!(body(&archiving[0]), json!({"archived": true}));

    let unarchiving = plan("archived: false", json!({"archived": true}));
    assert!(unarchiving.is_empty(), "{unarchiving:#?}");
}

#[test]
fn enum_fields_use_the_api_spelling() {
    let changes = plan(
        "squash_merge_commit_title: PR_TITLE",
        json!({"squash_merge_commit_title": "COMMIT_OR_PR_TITLE"}),
    );
    assert_eq!(
        body(&changes[0]),
        json!({"squash_merge_commit_title": "PR_TITLE"})
    );
}

#[test]
fn a_matching_enum_produces_no_change() {
    assert!(
        plan(
            "merge_commit_title: PR_TITLE",
            json!({"merge_commit_title": "PR_TITLE"})
        )
        .is_empty()
    );
}

#[test]
fn security_changes_travel_in_their_own_request() {
    // The API rejects `security_and_analysis` sent alongside ordinary fields.
    let changes = plan(
        "description: new\nsecurity:\n  secret_scanning: true",
        json!({
            "description": "old",
            "security_and_analysis": {"secret_scanning": {"status": "disabled"}}
        }),
    );
    assert_eq!(changes.len(), 2);
    assert!(matches!(
        changes[0].decode::<Payload>().unwrap(),
        Payload::Settings(_)
    ));
    assert!(matches!(
        changes[1].decode::<Payload>().unwrap(),
        Payload::Security(_)
    ));
}

#[test]
fn security_uses_the_status_object_shape() {
    let changes = plan(
        "security:\n  secret_scanning: true",
        json!({"security_and_analysis": {"secret_scanning": {"status": "disabled"}}}),
    );
    assert_eq!(
        body(&changes[0]),
        json!({"security_and_analysis": {"secret_scanning": {"status": "enabled"}}})
    );
}

#[test]
fn an_already_enabled_feature_produces_no_change() {
    assert!(
        plan(
            "security:\n  secret_scanning: true",
            json!({"security_and_analysis": {"secret_scanning": {"status": "enabled"}}})
        )
        .is_empty()
    );
}

#[test]
fn an_absent_security_feature_reads_as_disabled() {
    // Features unavailable on the plan are omitted from the payload entirely.
    let changes = plan(
        "security:\n  secret_scanning: true",
        json!({"security_and_analysis": {}}),
    );
    assert_eq!(changes.len(), 1);
}

#[test]
fn desired_is_none_when_the_section_is_absent() {
    let settings = Settings::default();
    assert!(Repository.desired(&settings).is_none());
}

#[test]
fn exports_never_include_archived() {
    // Exported files get copied between repositories; a one-way destructive flag
    // has no business in one.
    let current = current(json!({"description": "hello", "archived": true}));
    let exported = RepositorySettings {
        archived: None,
        ..Default::default()
    };
    assert!(current.archived);
    assert_eq!(exported.archived, None);
}

#[test]
fn every_boolean_setting_is_actually_diffed() {
    // `anonymous_access_enabled` was declared, published in the schema and
    // documented, and then never looked at: writing it planned nothing and
    // applied nothing. The merge layer has a compile-time guard against exactly
    // this; the diff had none, so here is one.
    let desired: RepositorySettings = serde_json::from_value(json!({
        // All `true`, against a repository where every one of them is false, so
        // each field is a change on its own and none can hide behind a default.
        "private": true,
        "has_issues": true,
        "has_wiki": true,
        "has_projects": true,
        "has_discussions": true,
        "is_template": true,
        "web_commit_signoff_required": true,
        "allow_merge_commit": true,
        "allow_squash_merge": true,
        "allow_rebase_merge": true,
        "allow_auto_merge": true,
        "allow_update_branch": true,
        "delete_branch_on_merge": true,
        "anonymous_access_enabled": true,
        "archived": true,
    }))
    .unwrap();

    // Every boolean the user could have written, taken from the type itself so
    // a field added later is caught here rather than shipped inert.
    let declared: Vec<String> = serde_json::to_value(&desired)
        .unwrap()
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, value)| value.is_boolean())
        .map(|(name, _)| name.clone())
        .collect();
    assert!(!declared.is_empty());

    let changes = Repository.diff(&desired, &current(json!({})), &PruneOpts::default());
    let patched = body(&changes[0]);

    for name in declared {
        assert!(
            patched.get(&name).is_some(),
            "`{name}` is accepted by the schema but never reaches the API"
        );
    }
}

#[test]
fn a_default_branch_is_compared_with_both_sides_trimmed() {
    assert!(
        plan(
            "default_branch: ' main '",
            json!({"default_branch": "main"})
        )
        .is_empty()
    );
}

#[test]
fn merge_commit_enums_are_compared_case_insensitively() {
    // GitHub is consistent about SCREAMING_SNAKE_CASE today, but a value that
    // arrives in another case must not become a change that can never be
    // applied away.
    assert!(
        plan(
            "merge_commit_title: PR_TITLE",
            json!({"merge_commit_title": "pr_title"})
        )
        .is_empty()
    );
}

#[test]
fn anonymous_access_is_only_exported_when_github_reports_it() {
    // github.com omits the field; Enterprise Server sends it. Exporting `false`
    // everywhere would invent a setting nobody has.
    assert_eq!(current(json!({})).anonymous_access_enabled, None);
    assert_eq!(
        current(json!({"anonymous_access_enabled": true})).anonymous_access_enabled,
        Some(true)
    );
}

#[test]
fn web_commit_signoff_is_left_alone_when_omitted() {
    assert!(plan("has_issues: true", json!({"has_issues": true})).is_empty());
}

#[test]
fn web_commit_signoff_is_required_when_declared() {
    let changes = plan(
        "web_commit_signoff_required: true",
        json!({"web_commit_signoff_required": false}),
    );
    assert_eq!(
        body(&changes[0])["web_commit_signoff_required"],
        json!(true)
    );
}
