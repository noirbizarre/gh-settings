//! Variable resource tests.
//!
//! The diff is a pure function, so everything about scoping, keying and
//! pruning can be tested without a runtime, a network or a stub.

use super::*;
use crate::config::{Prunable, Settings};
use pretty_assertions::assert_eq;

fn settings(yaml: &str) -> Settings {
    serde_norway::from_str(yaml).expect("valid configuration")
}

fn current(entries: Vec<(Scope, &str, &str)>) -> Current {
    Current {
        variables: entries
            .into_iter()
            .map(|(scope, name, value)| {
                let variable = Variable::new(name, value).normalized();
                ((scope, variable.name.clone()), variable)
            })
            .collect(),
    }
}

fn plan(yaml: &str, existing: Vec<(Scope, &str, &str)>) -> Vec<Change> {
    let settings = settings(yaml);
    let desired = Variables
        .desired(&settings)
        .expect("the configuration manages variables");
    let mut changes = Variables.diff(&desired, &current(existing), &PruneOpts::default());
    changes.sort_by(|left, right| left.key.cmp(&right.key));
    changes
}

fn environment(name: &str) -> Scope {
    Scope::Environment(name.to_string())
}

#[test]
fn an_absent_configuration_leaves_variables_unmanaged() {
    assert!(Variables.desired(&Settings::default()).is_none());
}

#[test]
fn environment_variables_alone_still_manage_the_resource() {
    // An unmanaged resource is skipped entirely, so reading only the top-level
    // section here would mean these variables are silently never written.
    let desired = Variables
        .desired(&settings(
            "environments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        ))
        .expect("environment variables manage the resource");

    assert!(desired.managed.contains(&environment("staging")));
    assert!(!desired.managed.contains(&Scope::Repository));
}

#[test]
fn an_environment_without_a_variables_key_is_left_alone() {
    assert!(
        Variables
            .desired(&settings("environments:\n  - name: staging\n"))
            .is_none()
    );
}

#[test]
fn repository_and_environment_variables_share_one_diff() {
    let changes = plan(
        "variables:\n  - name: REGION\n    value: eu\nenvironments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        vec![],
    );

    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|change| change.op == Op::Create));
}

#[test]
fn the_change_key_names_its_scope() {
    // Part of the plan artifact, so this string is public interface.
    let changes = plan(
        "variables:\n  - name: REGION\n    value: eu\nenvironments:\n  - name: staging\n    variables:\n      - name: REGION\n        value: us\n",
        vec![],
    );

    let keys: Vec<&str> = changes.iter().map(|change| change.key.as_str()).collect();
    assert_eq!(keys, vec!["env/staging:REGION", "repo:REGION"]);
}

#[test]
fn variable_names_are_compared_case_insensitively() {
    // GitHub rejects `region` when `REGION` exists, so a case-only difference
    // must not be planned as a creation that then fails with a 409.
    let changes = plan(
        "variables:\n  - name: region\n    value: eu\n",
        vec![(Scope::Repository, "REGION", "eu")],
    );
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn variable_values_are_compared_exactly() {
    // Whitespace is meaningful inside a value: a workflow sees it verbatim.
    let changes = plan(
        "variables:\n  - name: REGION\n    value: \"eu \"\n",
        vec![(Scope::Repository, "REGION", "eu")],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);
    assert_eq!(changes[0].summary, "update repository variable REGION");
}

#[test]
fn an_environment_that_does_not_exist_yet_yields_only_creations() {
    // `plan` runs before anything is applied, so the environment the file
    // declares is simply not there to read from.
    let changes = plan(
        "environments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        vec![],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "create staging variable URL");
}

#[test]
fn nothing_is_pruned_by_default() {
    let changes = plan(
        "variables:\n  - name: REGION\n    value: eu\n",
        vec![
            (Scope::Repository, "REGION", "eu"),
            (Scope::Repository, "STRAY", "x"),
        ],
    );
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn pruning_removes_undeclared_repository_variables() {
    let changes = plan(
        "variables:\n  prune: true\n  items:\n    - name: REGION\n      value: eu\n",
        vec![
            (Scope::Repository, "REGION", "eu"),
            (Scope::Repository, "STRAY", "x"),
        ],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].key, "repo:STRAY");
}

#[test]
fn pruning_never_touches_a_scope_the_configuration_does_not_manage() {
    // `variables: {prune: true}` asks to tidy the repository's own variables,
    // not every environment's — an environment the file says nothing about is
    // not something the user has asked to have cleaned out.
    let changes = plan(
        "variables:\n  prune: true\n  items: []\n",
        vec![(environment("production"), "SECRETISH", "x")],
    );
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn pruning_an_environments_variables_follows_the_environments_section() {
    // A variable cannot outlive the environment holding it, so one flag
    // governing both is the only reading that stays coherent.
    let changes = plan(
        "environments:\n  prune: true\n  items:\n    - name: staging\n      variables: []\n",
        vec![(environment("staging"), "STRAY", "x")],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].key, "env/staging:STRAY");
    assert_eq!(changes[0].summary, "delete staging variable STRAY");
}

#[test]
fn an_empty_variables_list_manages_the_scope_without_declaring_anything() {
    let desired = Variables
        .desired(&settings(
            "environments:\n  - name: staging\n    variables: []\n",
        ))
        .expect("declared, therefore managed");

    assert!(desired.managed.contains(&environment("staging")));
    assert!(desired.variables.is_empty());
}

#[test]
fn the_command_line_can_force_pruning_off() {
    let settings =
        settings("variables:\n  prune: true\n  items:\n    - name: REGION\n      value: eu\n");
    let desired = Variables.desired(&settings).expect("managed");
    let changes = Variables.diff(
        &desired,
        &current(vec![
            (Scope::Repository, "REGION", "eu"),
            (Scope::Repository, "STRAY", "x"),
        ]),
        &PruneOpts { force: Some(false) },
    );

    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn payloads_round_trip() {
    let changes = plan(
        "environments:\n  - name: staging\n    variables:\n      - name: URL\n        value: https://staging\n",
        vec![],
    );

    let payload: Payload = changes[0].decode().expect("decodable");
    assert_eq!(payload.scope, environment("staging"));
    assert_eq!(payload.variable, Variable::new("URL", "https://staging"));
}

#[test]
fn each_scope_addresses_its_own_endpoint() {
    let target = crate::github::Target::new("o", "r");

    assert_eq!(
        Scope::Repository.endpoint(&target),
        "repos/o/r/actions/variables"
    );
    assert_eq!(
        environment("staging").endpoint(&target),
        "repos/o/r/environments/staging/variables"
    );
}

#[test]
fn an_environment_name_is_encoded_into_one_path_segment() {
    let target = crate::github::Target::new("o", "r");
    assert_eq!(
        environment("qa/eu west").endpoint(&target),
        "repos/o/r/environments/qa%2Feu%20west/variables"
    );
}

#[test]
fn the_prunable_object_form_is_accepted() {
    let desired = Variables
        .desired(&settings(
            "variables:\n  prune: true\n  items:\n    - name: REGION\n      value: eu\n",
        ))
        .expect("managed");
    assert_eq!(desired.prune.get(&Scope::Repository), Some(&true));
    assert_eq!(
        Prunable::from(vec![Variable::new("A", "b")]).items().len(),
        1
    );
}
