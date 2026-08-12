//! Environment resource tests.
//!
//! Most of what can go wrong here is normalisation: GitHub reports protection
//! as a heterogeneous rule array and omits every rule that is not set, so a
//! naive comparison reports an update on every single run.

use super::*;
use crate::config::Settings;
use crate::resources::FieldDiff;
use pretty_assertions::assert_eq;
use serde_json::json;

fn settings(yaml: &str) -> Settings {
    serde_norway::from_str(yaml).expect("valid configuration")
}

fn desired(yaml: &str) -> Desired {
    Environments
        .desired(&settings(yaml))
        .expect("the configuration manages environments")
}

/// Build current state from the shape the API actually returns.
fn state(value: serde_json::Value) -> EnvironmentState {
    serde_json::from_value(value).expect("a well-formed environment")
}

fn current(states: Vec<serde_json::Value>) -> Current {
    Current {
        environments: states
            .into_iter()
            .map(|value| {
                let state = state(value);
                (
                    model::key(&state.name),
                    CurrentEnvironment {
                        environment: state.as_environment().normalized(),
                        pattern_ids: HashMap::new(),
                    },
                )
            })
            .collect(),
    }
}

fn plan(yaml: &str, existing: Vec<serde_json::Value>) -> Vec<Change> {
    let mut changes = Environments.diff(&desired(yaml), &current(existing), &PruneOpts::default());
    changes.sort_by(|left, right| left.key.cmp(&right.key));
    changes
}

/// An environment with no protection at all, as GitHub reports one.
fn bare(name: &str) -> serde_json::Value {
    json!({ "name": name, "protection_rules": [], "deployment_branch_policy": null })
}

#[test]
fn an_absent_section_leaves_environments_unmanaged() {
    assert!(Environments.desired(&Settings::default()).is_none());
}

#[test]
fn creates_missing_environments() {
    let changes = plan("environments:\n  - name: staging\n", vec![]);

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "create environment staging");
}

#[test]
fn an_environment_that_already_exists_produces_no_change() {
    assert!(plan("environments:\n  - name: staging\n", vec![bare("staging")]).is_empty());
}

#[test]
fn environment_names_are_matched_case_insensitively() {
    // GitHub answers a create for an existing name with a 422, so a case-only
    // difference has to match rather than be planned as a creation.
    assert!(plan("environments:\n  - name: Staging\n", vec![bare("staging")]).is_empty());
}

#[test]
fn wait_timer_zero_and_an_omitted_wait_timer_are_the_same_state() {
    // GitHub stores no `wait_timer` rule at all for a zero delay, so comparing
    // the raw values would report an update forever.
    assert!(
        plan(
            "environments:\n  - name: staging\n    wait_timer: 0\n",
            vec![bare("staging")]
        )
        .is_empty()
    );
}

#[test]
fn a_wait_timer_is_read_back_out_of_the_protection_rules() {
    assert!(
        plan(
            "environments:\n  - name: staging\n    wait_timer: 30\n",
            vec![json!({
                "name": "staging",
                "protection_rules": [{ "type": "wait_timer", "wait_timer": 30 }],
                "deployment_branch_policy": null,
            })],
        )
        .is_empty()
    );
}

#[test]
fn changing_a_wait_timer_is_an_update() {
    let changes = plan(
        "environments:\n  - name: staging\n    wait_timer: 60\n",
        vec![json!({
            "name": "staging",
            "protection_rules": [{ "type": "wait_timer", "wait_timer": 30 }],
            "deployment_branch_policy": null,
        })],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);
    assert_eq!(
        changes[0].fields,
        vec![FieldDiff::changed("wait_timer", "30", "60")]
    );
}

#[test]
fn an_omitted_wait_timer_is_never_cleared() {
    // An omitted field is unmanaged; resetting it to a default because the file
    // said nothing is the one thing this tool must never do.
    assert!(
        plan(
            "environments:\n  - name: staging\n",
            vec![json!({
                "name": "staging",
                "protection_rules": [{ "type": "wait_timer", "wait_timer": 30 }],
                "deployment_branch_policy": null,
            })],
        )
        .is_empty()
    );
}

#[test]
fn an_empty_reviewer_list_and_no_reviewer_rule_are_the_same_state() {
    // No `required_reviewers` rule means nobody reviews, which is a state
    // rather than an absence.
    assert!(
        plan(
            "environments:\n  - name: staging\n    reviewers: []\n",
            vec![bare("staging")]
        )
        .is_empty()
    );
}

#[test]
fn prevent_self_review_is_ignored_when_there_are_no_reviewers() {
    // The flag lives on the `required_reviewers` rule, so with nobody to review
    // there is nowhere for it to come back from — comparing it would report an
    // update on every run.
    assert!(
        plan(
            "environments:\n  - name: staging\n    reviewers: []\n    prevent_self_review: true\n",
            vec![bare("staging")],
        )
        .is_empty()
    );
}

#[test]
fn reviewers_are_compared_by_identifier_not_by_order() {
    // The API returns reviewers in arbitrary order.
    let mut desired = desired(
        "environments:\n  - name: staging\n    reviewers:\n      - team: eng\n      - user: octocat\n",
    );
    for environment in &mut desired.environments {
        for reviewer in environment.reviewers.iter_mut().flatten() {
            reviewer.id = Some(if reviewer.team.is_some() { 7 } else { 1 });
        }
        *environment = environment.normalized();
    }

    let existing = current(vec![json!({
        "name": "staging",
        "protection_rules": [{
            "type": "required_reviewers",
            "prevent_self_review": false,
            "reviewers": [
                { "type": "Team", "reviewer": { "id": 7, "slug": "eng" } },
                { "type": "User", "reviewer": { "id": 1, "login": "octocat" } },
            ],
        }],
        "deployment_branch_policy": null,
    })]);

    assert!(
        Environments
            .diff(&desired, &existing, &PruneOpts::default())
            .is_empty()
    );
}

#[test]
fn a_reviewer_is_matched_on_its_identifier_rather_than_its_name() {
    // Somebody who changes their login is still the same reviewer; comparing
    // names would report an update every run for them.
    let mut desired =
        desired("environments:\n  - name: staging\n    reviewers:\n      - user: newname\n");
    for environment in &mut desired.environments {
        for reviewer in environment.reviewers.iter_mut().flatten() {
            reviewer.id = Some(1);
        }
    }

    let existing = current(vec![json!({
        "name": "staging",
        "protection_rules": [{
            "type": "required_reviewers",
            "prevent_self_review": false,
            "reviewers": [{ "type": "User", "reviewer": { "id": 1, "login": "oldname" } }],
        }],
        "deployment_branch_policy": null,
    })]);

    assert!(
        Environments
            .diff(&desired, &existing, &PruneOpts::default())
            .is_empty()
    );
}

#[test]
fn protection_rules_are_folded_back_into_the_flat_configuration_shape() {
    let environment = state(json!({
        "name": "production",
        "protection_rules": [
            { "type": "wait_timer", "wait_timer": 15 },
            {
                "type": "required_reviewers",
                "prevent_self_review": true,
                "reviewers": [{ "type": "Team", "reviewer": { "id": 7, "slug": "eng" } }],
            },
            { "type": "branch_policy" },
        ],
        "deployment_branch_policy": { "protected_branches": true, "custom_branch_policies": false },
    }))
    .as_environment();

    assert_eq!(environment.wait_timer, Some(15));
    assert_eq!(environment.prevent_self_review, Some(true));
    assert_eq!(
        environment.reviewers,
        Some(vec![Reviewer {
            user: None,
            team: Some("eng".into()),
            id: Some(7),
        }])
    );
    assert_eq!(
        environment.deployment_branch_policy,
        Some(Some(DeploymentBranchPolicy::Protected(
            model::ProtectedKeyword::Protected
        )))
    );
}

#[test]
fn an_omitted_branch_policy_leaves_it_unmanaged() {
    assert!(
        plan(
            "environments:\n  - name: staging\n",
            vec![json!({
                "name": "staging",
                "protection_rules": [],
                "deployment_branch_policy": {
                    "protected_branches": true, "custom_branch_policies": false,
                },
            })],
        )
        .is_empty()
    );
}

#[test]
fn an_explicit_null_branch_policy_clears_it() {
    // Absent and null are different requests: one says "leave it alone", the
    // other says "any branch may deploy", which is a real setting.
    let changes = plan(
        "environments:\n  - name: staging\n    deployment_branch_policy: null\n",
        vec![json!({
            "name": "staging",
            "protection_rules": [],
            "deployment_branch_policy": {
                "protected_branches": true, "custom_branch_policies": false,
            },
        })],
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].fields,
        vec![FieldDiff::changed(
            "deployment_branch_policy",
            "protected",
            "any branch"
        )]
    );
}

#[test]
fn the_protected_keyword_is_accepted() {
    let changes = plan(
        "environments:\n  - name: staging\n    deployment_branch_policy: protected\n",
        vec![bare("staging")],
    );

    assert_eq!(changes.len(), 1);
    let payload: Payload = changes[0].decode().expect("decodable");
    let body = payload.environment.as_ref().unwrap();
    assert_eq!(
        body.as_body(body)["deployment_branch_policy"],
        json!({ "protected_branches": true, "custom_branch_policies": false })
    );
}

#[test]
fn custom_branch_patterns_are_created_after_the_environment() {
    let changes = plan(
        "environments:\n  - name: staging\n    deployment_branch_policy:\n      branches: [main]\n      tags: [\"v*\"]\n",
        vec![],
    );

    let payload: Payload = changes[0].decode().expect("decodable");
    assert_eq!(
        payload.create_patterns,
        vec![Pattern::branch("main"), Pattern::tag("v*")]
    );
    assert!(payload.delete_pattern_ids.is_empty());
}

#[test]
fn a_removed_pattern_is_deleted_by_its_server_identifier() {
    let mut existing = current(vec![json!({
        "name": "staging",
        "protection_rules": [{ "type": "branch_policy" }],
        "deployment_branch_policy": {
            "protected_branches": false, "custom_branch_policies": true,
        },
    })]);
    let entry = existing.environments.get_mut("staging").unwrap();
    entry.environment.deployment_branch_policy = Some(Some(DeploymentBranchPolicy::Custom {
        branches: vec!["main".into(), "stale".into()],
        tags: Vec::new(),
    }));
    entry.pattern_ids.insert(Pattern::branch("main"), 1);
    entry.pattern_ids.insert(Pattern::branch("stale"), 2);

    let changes = Environments.diff(
        &desired(
            "environments:\n  - name: staging\n    deployment_branch_policy:\n      branches: [main]\n",
        ),
        &existing,
        &PruneOpts::default(),
    );

    let payload: Payload = changes[0].decode().expect("decodable");
    assert_eq!(payload.delete_pattern_ids, vec![2]);
    assert!(payload.create_patterns.is_empty());
}

#[test]
fn switching_away_from_custom_policies_needs_no_pattern_deletions() {
    // The `PUT` discards them, so deleting each one first would be a wasted
    // round trip per pattern.
    let mut existing = current(vec![json!({
        "name": "staging",
        "protection_rules": [{ "type": "branch_policy" }],
        "deployment_branch_policy": {
            "protected_branches": false, "custom_branch_policies": true,
        },
    })]);
    let entry = existing.environments.get_mut("staging").unwrap();
    entry.environment.deployment_branch_policy = Some(Some(DeploymentBranchPolicy::Custom {
        branches: vec!["main".into()],
        tags: Vec::new(),
    }));
    entry.pattern_ids.insert(Pattern::branch("main"), 1);

    let changes = Environments.diff(
        &desired("environments:\n  - name: staging\n    deployment_branch_policy: protected\n"),
        &existing,
        &PruneOpts::default(),
    );

    let payload: Payload = changes[0].decode().expect("decodable");
    assert!(payload.delete_pattern_ids.is_empty());
    assert!(payload.create_patterns.is_empty());
}

#[test]
fn identical_custom_patterns_produce_no_change() {
    let mut existing = current(vec![json!({
        "name": "staging",
        "protection_rules": [{ "type": "branch_policy" }],
        "deployment_branch_policy": {
            "protected_branches": false, "custom_branch_policies": true,
        },
    })]);
    existing
        .environments
        .get_mut("staging")
        .unwrap()
        .environment
        .deployment_branch_policy = Some(Some(DeploymentBranchPolicy::Custom {
        branches: vec!["main".into()],
        tags: vec!["v*".into()],
    }));

    assert!(
        Environments
            .diff(
                &desired(
                    "environments:\n  - name: staging\n    deployment_branch_policy:\n      branches: [main]\n      tags: [\"v*\"]\n",
                ),
                &existing,
                &PruneOpts::default(),
            )
            .is_empty()
    );
}

#[test]
fn nothing_is_deleted_by_default() {
    assert!(
        plan(
            "environments:\n  - name: staging\n",
            vec![bare("staging"), bare("production")]
        )
        .is_empty()
    );
}

#[test]
fn deleting_an_environment_says_what_else_it_destroys() {
    // Deleting an environment takes its variables, its secrets and its
    // deployment history with it, and the plan is the last chance to say so.
    let changes = Environments.diff(
        &desired("environments:\n  prune: true\n  items:\n    - name: staging\n"),
        &current(vec![bare("staging"), bare("production")]),
        &PruneOpts::default(),
    );

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert!(changes[0].is_destructive());
    assert_eq!(
        changes[0].summary,
        "delete environment production \
         (also deletes its variables, secrets and deployment history)"
    );
}

#[test]
fn variables_are_not_diffed_here() {
    // They belong to the `variables` resource; comparing them in two places
    // would produce the same change twice.
    assert!(
        plan(
            "environments:\n  - name: staging\n    variables:\n      - name: URL\n        value: x\n",
            vec![bare("staging")],
        )
        .is_empty()
    );
}

#[test]
fn the_body_restates_every_effective_value() {
    // The endpoint replaces the environment wholesale, so a field the file does
    // not manage has to be filled from what exists or the write clears it.
    let existing = state(json!({
        "name": "staging",
        "protection_rules": [{ "type": "wait_timer", "wait_timer": 30 }],
        "deployment_branch_policy": null,
    }))
    .as_environment()
    .normalized();

    let mut environment = Environment::new("staging");
    environment.deployment_branch_policy = Some(None);

    let body = environment.as_body(&existing);
    assert_eq!(body["wait_timer"], json!(30));
    assert_eq!(body["reviewers"], json!([]));
    assert_eq!(body["prevent_self_review"], json!(false));
    assert_eq!(body["deployment_branch_policy"], json!(null));
}

#[test]
fn a_reviewer_body_carries_the_resolved_identifier() {
    let mut environment = Environment::new("staging");
    environment.reviewers = Some(vec![Reviewer {
        user: None,
        team: Some("eng".into()),
        id: Some(7),
    }]);
    environment.prevent_self_review = Some(true);

    let body = environment.as_body(&Environment::new("staging"));
    assert_eq!(body["reviewers"], json!([{ "type": "Team", "id": 7 }]));
    assert_eq!(body["prevent_self_review"], json!(true));
}

#[test]
fn a_missing_type_on_a_branch_policy_pattern_reads_as_a_branch() {
    let pattern: model::PatternState =
        serde_json::from_value(json!({ "id": 1, "name": "main" })).expect("decodable");
    assert_eq!(pattern.as_pattern(), Pattern::branch("main"));
}
