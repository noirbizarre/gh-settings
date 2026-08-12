//! Merge tests.
//!
//! Every case here is a way the merge could quietly lose or invent
//! configuration, which is the failure mode that matters: a wrong merge does not
//! error, it just manages something the user did not ask for.

use super::*;
use crate::config::source::Sources;
use pretty_assertions::assert_eq;

/// Parse two documents and merge them, base first.
fn merged(base_source: &str, child_source: &str) -> (Settings, Provenance, Sources) {
    let (mut sources, child_id) = Sources::root("local.yml", child_source);
    let base_id = sources.push("acme/.github@v1", base_source);

    let mut base_settings: Settings = serde_norway::from_str(base_source).expect("valid base");
    let mut child_settings: Settings = serde_norway::from_str(child_source).expect("valid child");
    base_settings.canonicalize();
    child_settings.canonicalize();

    let base_spans = SpanIndex::build(base_id, base_source);
    let child_spans = SpanIndex::build(child_id, child_source);

    let base = Layer {
        id: base_id,
        settings: &base_settings,
        spans: &base_spans,
    };
    let child = Layer {
        id: child_id,
        settings: &child_settings,
        spans: &child_spans,
    };

    let (settings, provenance) = merge(&base, &child);
    (settings, provenance, sources)
}

fn label_names(settings: &Settings) -> Vec<String> {
    settings
        .labels
        .as_ref()
        .expect("labels")
        .items()
        .iter()
        .map(|label| label.name.clone())
        .collect()
}

// --- collections ------------------------------------------------------------

#[test]
fn an_inherited_collection_is_available_to_a_child_that_declares_none() {
    let (settings, _, _) = merged(
        "labels:\n  - name: bug\n    color: d73a4a\n",
        "version: 1\n",
    );
    assert_eq!(label_names(&settings), ["bug"]);
}

#[test]
fn a_child_item_replaces_the_inherited_one_of_the_same_identity() {
    let (settings, _, _) = merged(
        "labels:\n  - name: bug\n    color: d73a4a\n    description: from the base\n",
        "labels:\n  - name: bug\n    color: ff0000\n",
    );

    let labels = settings.labels.as_ref().unwrap().items();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].color, "ff0000");
    assert_eq!(
        labels[0].description, None,
        "the whole item is replaced, so the base's description does not survive"
    );
}

#[test]
fn item_identity_is_the_same_key_the_diff_uses() {
    // Labels are matched case-insensitively on GitHub, so `BUG` and `bug` are
    // one label. A merge that disagreed with the diff would produce a plan that
    // tries to create a label that already exists.
    let (settings, _, _) = merged(
        "labels:\n  - name: bug\n    color: d73a4a\n",
        "labels:\n  - name: BUG\n    color: ff0000\n",
    );
    assert_eq!(label_names(&settings), ["BUG"]);
}

#[test]
fn a_child_only_item_is_appended_after_the_inherited_ones() {
    let (settings, _, _) = merged(
        "labels:\n  - name: bug\n    color: d73a4a\n  - name: chore\n    color: cccccc\n",
        "labels:\n  - name: feature\n    color: a2eeef\n",
    );
    assert_eq!(label_names(&settings), ["bug", "chore", "feature"]);
}

#[test]
fn inherited_order_is_preserved_when_a_child_overrides_in_the_middle() {
    // Base order, so adding to the child renumbers only the tail and the
    // provenance of inherited items does not churn.
    let (settings, _, _) = merged(
        "labels:\n  - name: a\n    color: aaaaaa\n  - name: b\n    color: bbbbbb\n  - name: c\n    color: cccccc\n",
        "labels:\n  - name: b\n    color: ffffff\n  - name: d\n    color: dddddd\n",
    );
    assert_eq!(label_names(&settings), ["a", "b", "c", "d"]);
    assert_eq!(settings.labels.as_ref().unwrap().items()[1].color, "ffffff");
}

#[test]
fn duplicates_within_one_document_survive_the_merge() {
    // The merge must not deduplicate: `gh_settings::labels::duplicate` is a real
    // finding, and silently collapsing the two entries would delete the user's
    // mistake instead of reporting it.
    let (settings, _, _) = merged(
        "version: 1\n",
        "labels:\n  - name: bug\n    color: d73a4a\n  - name: bug\n    color: ff0000\n",
    );
    assert_eq!(label_names(&settings), ["bug", "bug"]);
}

#[test]
fn every_collection_merges_by_its_own_identity() {
    let (settings, _, _) = merged(
        "topics:\n  - rust\nautolinks:\n  - key_prefix: 'OPS-'\n    url_template: https://base/<num>\nrulesets:\n  - name: main\n    rules:\n      - type: creation\n",
        "topics:\n  - cli\nautolinks:\n  - key_prefix: 'OPS-'\n    url_template: https://child/<num>\nrulesets:\n  - name: main\n    rules:\n      - type: non_fast_forward\n",
    );

    assert_eq!(settings.topics.as_ref().unwrap().items(), ["rust", "cli"]);

    let autolinks = settings.autolinks.as_ref().unwrap().items();
    assert_eq!(autolinks.len(), 1, "same key_prefix, so one autolink");
    assert_eq!(autolinks[0].url_template, "https://child/<num>");

    let rulesets = settings.rulesets.as_ref().unwrap().items();
    assert_eq!(rulesets.len(), 1, "same name, so one ruleset");
    assert_eq!(rulesets[0].rules[0].rule_type, "non_fast_forward");
}

#[test]
fn environments_and_variables_merge_by_the_key_their_diff_uses() {
    // GitHub matches environment names case-insensitively and variable names
    // case-insensitively too, so a merge that disagreed with the diff would
    // produce a plan that creates something which already exists.
    let (settings, _, _) = merged(
        "environments:\n  - name: staging\n    wait_timer: 5\nvariables:\n  - name: region\n    value: eu\n",
        "environments:\n  - name: Staging\n    wait_timer: 10\nvariables:\n  - name: REGION\n    value: us\n",
    );

    let environments = settings.environments.as_ref().unwrap().items();
    assert_eq!(environments.len(), 1, "same name, so one environment");
    assert_eq!(environments[0].name, "Staging");
    assert_eq!(environments[0].wait_timer, Some(10));

    let variables = settings.variables.as_ref().unwrap().items();
    assert_eq!(variables.len(), 1, "same name, so one variable");
    assert_eq!(variables[0].value, "us");
}

#[test]
fn a_redeclared_environment_replaces_its_nested_variables_whole() {
    // Replace-by-identity, applied to a large item. Documented in ADR-017
    // rather than special-cased: merging field by field would reintroduce the
    // omitted-versus-defaulted ambiguity the whole design avoids.
    let (settings, _, _) = merged(
        "environments:\n  - name: staging\n    variables:\n      - name: A\n        value: inherited\n",
        "environments:\n  - name: staging\n    wait_timer: 5\n",
    );

    let environments = settings.environments.as_ref().unwrap().items();
    assert_eq!(environments[0].wait_timer, Some(5));
    assert_eq!(
        environments[0].variables, None,
        "the child's item wins whole"
    );
}

// --- pruning ----------------------------------------------------------------

#[test]
fn pruning_never_inherits() {
    // The reason: otherwise editing one shared file starts deleting across every
    // repository that extends it, decided by someone who does not own them.
    let (settings, _, _) = merged(
        "labels:\n  prune: true\n  items:\n    - name: bug\n      color: d73a4a\n",
        "version: 1\n",
    );
    assert!(!settings.labels.as_ref().unwrap().prune());
}

#[test]
fn pruning_declared_by_the_child_is_kept() {
    let (settings, _, _) = merged(
        "labels:\n  - name: bug\n    color: d73a4a\n",
        "labels:\n  prune: true\n  items:\n    - name: feature\n      color: a2eeef\n",
    );
    let labels = settings.labels.as_ref().unwrap();
    assert!(labels.prune());
    assert_eq!(labels.items().len(), 2, "pruning does not discard the base");
}

// --- repository -------------------------------------------------------------

#[test]
fn repository_fields_merge_individually() {
    let (settings, _, _) = merged(
        "repository:\n  description: from the base\n  homepage: https://base\n",
        "repository:\n  description: from the child\n",
    );
    let repository = settings.repository.as_ref().unwrap();
    assert_eq!(
        repository.description,
        Some(Some("from the child".to_string()))
    );
    assert_eq!(
        repository.homepage,
        Some(Some("https://base".to_string())),
        "a field the child never mentioned stays inherited"
    );
}

#[test]
fn a_child_clearing_a_field_beats_the_base_setting_it() {
    // `description: null` is "manage this, and make it empty" — not "say
    // nothing". Collapsing the two layers of `Option` would turn an explicit
    // clear back into an inherited value.
    let (settings, _, _) = merged(
        "repository:\n  description: from the base\n",
        "repository:\n  description: null\n",
    );
    assert_eq!(
        settings.repository.as_ref().unwrap().description,
        Some(None)
    );
}

#[test]
fn security_settings_merge_field_by_field() {
    // Taking the child's block whole would silently unmanage the base's
    // `advanced_security` the moment a child mentioned anything else.
    let (settings, _, _) = merged(
        "repository:\n  security:\n    advanced_security: true\n",
        "repository:\n  security:\n    secret_scanning: true\n",
    );
    let security = settings.repository.as_ref().unwrap().security.unwrap();
    assert_eq!(security.advanced_security, Some(true));
    assert_eq!(security.secret_scanning, Some(true));
}

// --- provenance -------------------------------------------------------------

/// The text a merged path underlines, read from the document it names.
fn underlined(
    provenance: &Provenance,
    sources: &Sources,
    indices: &[(SourceId, SpanIndex)],
    path: &str,
) -> String {
    let (source, physical) = provenance.resolve(path).expect("a recorded path");
    let index = indices
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, index)| index)
        .expect("an index for that document");
    let span = index.exact(&physical).expect("a node at the physical path");
    sources.get(source).text[span.offset()..span.offset() + span.len()].to_string()
}

#[test]
fn a_merged_item_points_at_the_document_that_declared_it() {
    // The whole reason provenance exists. Merged position 1 is the child's item
    // 0, and position 2 is the base's item 1 — neither of which a positional
    // lookup could have found.
    const BASE: &str = "labels:\n  - name: a\n    color: aaaaaa\n  - name: c\n    color: cccccc\n";
    const CHILD: &str = "labels:\n  - name: a\n    color: ffffff\n";

    let (settings, provenance, sources) = merged(BASE, CHILD);
    assert_eq!(label_names(&settings), ["a", "c"]);

    let child_id = SourceId::ROOT;
    let base_id = sources
        .iter()
        .find(|file| file.name == "acme/.github@v1")
        .unwrap()
        .id;
    let indices = vec![
        (child_id, SpanIndex::build(child_id, CHILD)),
        (base_id, SpanIndex::build(base_id, BASE)),
    ];

    assert_eq!(
        underlined(&provenance, &sources, &indices, "labels.0.color"),
        "ffffff",
        "position 0 was overridden by the child"
    );
    assert_eq!(
        underlined(&provenance, &sources, &indices, "labels.1.color"),
        "cccccc",
        "position 1 is still the base's second label"
    );
}

#[test]
fn provenance_survives_the_two_prunable_forms_disagreeing() {
    // The case `items_path` could never handle: the base uses the object form
    // and the child the bare list, so one physical shape cannot describe both.
    const BASE: &str = "labels:\n  prune: true\n  items:\n    - name: a\n      color: aaaaaa\n";
    const CHILD: &str = "labels:\n  - name: b\n    color: bbbbbb\n";

    let (_, provenance, sources) = merged(BASE, CHILD);

    let child_id = SourceId::ROOT;
    let base_id = sources
        .iter()
        .find(|file| file.name == "acme/.github@v1")
        .unwrap()
        .id;
    let indices = vec![
        (child_id, SpanIndex::build(child_id, CHILD)),
        (base_id, SpanIndex::build(base_id, BASE)),
    ];

    assert_eq!(
        underlined(&provenance, &sources, &indices, "labels.0.color"),
        "aaaaaa"
    );
    assert_eq!(
        underlined(&provenance, &sources, &indices, "labels.1.color"),
        "bbbbbb"
    );
}

#[test]
fn a_repository_field_points_at_whichever_document_set_it() {
    const BASE: &str = "repository:\n  homepage: https://base\n";
    const CHILD: &str = "repository:\n  description: from the child\n";

    let (_, provenance, _) = merged(BASE, CHILD);

    let (homepage_source, _) = provenance.resolve("repository.homepage").unwrap();
    let (description_source, _) = provenance.resolve("repository.description").unwrap();
    assert_ne!(homepage_source, SourceId::ROOT);
    assert_eq!(description_source, SourceId::ROOT);
}

#[test]
fn a_path_no_document_declares_is_not_recorded() {
    let (_, provenance, _) = merged("version: 1\n", "version: 1\n");
    assert_eq!(provenance.resolve("repository.private"), None);
}
