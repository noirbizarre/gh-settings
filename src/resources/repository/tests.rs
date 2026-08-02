//! Repository resource tests.

use super::*;
use crate::config::Settings;
use pretty_assertions::assert_eq;
use serde_json::json;

fn desired(source: &str) -> RepositorySettings {
    serde_norway::from_str(source).unwrap()
}

fn current(value: Value) -> Current {
    serde_json::from_value(value).unwrap()
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
