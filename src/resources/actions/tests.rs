//! Actions resource tests.

use super::*;
use crate::config::SpanIndex;
use pretty_assertions::assert_eq;
use serde_json::json;

fn settings(yaml: &str) -> ActionsSettings {
    serde_norway::from_str(yaml).expect("valid actions section")
}

/// Build a `Current` from one JSON object per endpoint, keyed by change key.
///
/// Through `normalized`, as the real `current()` does: a test that compares
/// against an unnormalised value is not testing what the tool runs.
fn current(groups: Value) -> Current {
    let groups = groups.as_object().cloned().unwrap_or_default();
    let group = |key: &str| groups.get(key).cloned();

    fn parse<T: serde::de::DeserializeOwned>(value: Option<Value>) -> Option<T> {
        value.map(|value| serde_json::from_value(value).unwrap())
    }

    Current {
        permissions: parse(group(endpoint::PERMISSIONS)),
        selected_actions: parse(group(endpoint::SELECTED_ACTIONS)),
        workflow: parse(group(endpoint::WORKFLOW)),
        retention: parse(group(endpoint::RETENTION)),
        fork_pr_approval: parse(group(endpoint::FORK_PR_APPROVAL)),
        access: parse(group(endpoint::ACCESS)),
        fork_pr_private: parse(group(endpoint::FORK_PR_PRIVATE)),
    }
    .normalized()
}

/// A repository with Actions on, everything at GitHub's defaults, and the two
/// private-only endpoints answering nothing — which is what a public repository
/// looks like.
fn a_public_repository() -> Value {
    json!({
        "permissions": {"enabled": true, "allowed_actions": "all", "sha_pinning_required": false},
        "workflow": {
            "default_workflow_permissions": "read",
            "can_approve_pull_request_reviews": false,
        },
        "artifact-and-log-retention": {"days": 90, "maximum_allowed_days": 400},
        "fork-pr-contributor-approval": {"approval_policy": "first_time_contributors"},
    })
}

fn plan(yaml: &str, current_groups: Value) -> Vec<Change> {
    Actions.diff(
        &settings(yaml),
        &current(current_groups),
        &PruneOpts::default(),
    )
}

fn body(change: &Change) -> Value {
    change.decode::<Payload>().unwrap().body
}

fn keys(changes: &[Change]) -> Vec<String> {
    changes.iter().map(|change| change.key.clone()).collect()
}

#[test]
fn an_empty_section_produces_no_change() {
    assert!(plan("{}", a_public_repository()).is_empty());
}

#[test]
fn an_omitted_field_is_unmanaged() {
    // Only the retention period is declared, so only the retention endpoint is
    // touched — nothing reaches the permissions endpoint, whose current values
    // differ from nothing in particular.
    let changes = plan("artifact_and_log_retention_days: 30", a_public_repository());
    assert_eq!(keys(&changes), vec![endpoint::RETENTION]);
    assert_eq!(body(&changes[0]), json!({"days": 30}));
}

#[test]
fn a_matching_configuration_produces_no_change() {
    let yaml = "\
enabled: true
allowed_actions: all
sha_pinning_required: false
default_workflow_permissions: read
can_approve_pull_request_reviews: false
artifact_and_log_retention_days: 90
fork_pr_contributor_approval: first_time_contributors
";
    assert!(plan(yaml, a_public_repository()).is_empty());
}

#[test]
fn each_endpoint_gets_its_own_request() {
    // GitHub rejects a body that mixes fields from two of these endpoints, so
    // one change per endpoint is a correctness requirement, not tidiness.
    let yaml = "\
allowed_actions: local_only
default_workflow_permissions: write
artifact_and_log_retention_days: 30
fork_pr_contributor_approval: all_external_contributors
";
    let changes = plan(yaml, a_public_repository());
    assert_eq!(
        keys(&changes),
        vec![
            endpoint::PERMISSIONS,
            endpoint::WORKFLOW,
            endpoint::RETENTION,
            endpoint::FORK_PR_APPROVAL,
        ]
    );
    for change in &changes {
        assert_eq!(
            body(change).as_object().unwrap().len(),
            if change.key == endpoint::PERMISSIONS {
                2 // the policy, plus the `enabled` the API insists on
            } else {
                1
            }
        );
    }
}

#[test]
fn changing_the_policy_carries_the_enabled_flag_the_api_requires() {
    // `PUT /actions/permissions` refuses a body without `enabled`. Taking it
    // from the current state preserves it; defaulting it would turn Actions off.
    let changes = plan("allowed_actions: selected", a_public_repository());
    assert_eq!(
        body(&changes[0]),
        json!({"allowed_actions": "selected", "enabled": true})
    );
}

#[test]
fn a_declared_enabled_flag_is_not_overwritten_by_the_current_one() {
    let changes = plan(
        "enabled: false\nallowed_actions: selected",
        a_public_repository(),
    );
    assert_eq!(
        body(&changes[0]),
        json!({"allowed_actions": "selected", "enabled": false})
    );
}

#[test]
fn an_allow_list_is_sent_to_its_own_endpoint() {
    let yaml = "\
allowed_actions: selected
selected_actions:
  github_owned_allowed: true
  verified_allowed: false
  patterns_allowed:
    - docker/*
";
    let changes = plan(yaml, a_public_repository());
    assert_eq!(
        keys(&changes),
        vec![endpoint::PERMISSIONS, endpoint::SELECTED_ACTIONS]
    );
    assert_eq!(
        body(&changes[1]),
        json!({
            "github_owned_allowed": true,
            "verified_allowed": false,
            "patterns_allowed": ["docker/*"],
        })
    );
}

#[test]
fn an_allow_list_is_a_set_not_a_sequence() {
    // GitHub does not promise an order, so comparing the written one would
    // produce a difference that could never be applied away.
    let yaml = "\
selected_actions:
  patterns_allowed:
    - ' docker/* '
    - actions/checkout@v4
    - docker/*
";
    let mut current = a_public_repository();
    current["selected-actions"] = json!({"patterns_allowed": ["actions/checkout@v4", "docker/*"]});
    assert!(plan(yaml, current).is_empty());
}

#[test]
fn a_group_github_does_not_expose_still_produces_a_change() {
    // `/access` answers 404 on a public repository. Reporting "up to date"
    // would claim a convergence that has not happened; emitting the change lets
    // apply surface GitHub's own error.
    let changes = plan("access_level: organization", a_public_repository());
    assert_eq!(keys(&changes), vec![endpoint::ACCESS]);
    assert_eq!(body(&changes[0]), json!({"access_level": "organization"}));
    assert_eq!(changes[0].fields[0].before, None);
}

#[test]
fn the_private_fork_pr_block_carries_the_field_the_api_requires() {
    let yaml = "\
fork_pr_workflows_private_repos:
  send_secrets_and_variables: false
";
    let mut current = a_public_repository();
    current["fork-pr-workflows-private-repos"] = json!({
        "run_workflows_from_fork_pull_requests": true,
        "send_secrets_and_variables": true,
    });
    let changes = plan(yaml, current);
    assert_eq!(
        body(&changes[0]),
        json!({
            "send_secrets_and_variables": false,
            "run_workflows_from_fork_pull_requests": true,
        })
    );
}

#[test]
fn enum_values_are_compared_case_insensitively() {
    // GitHub is consistent about lowercase today, but a value arriving in
    // another case must not become a change that can never be applied away.
    let mut current = a_public_repository();
    current["workflow"]["default_workflow_permissions"] = json!("READ");
    assert!(plan("default_workflow_permissions: read", current).is_empty());
}

#[test]
fn the_read_only_retention_ceiling_is_never_sent() {
    // `maximum_allowed_days` describes the plan, not the repository, and is not
    // a body parameter of the PUT.
    let changes = plan("artifact_and_log_retention_days: 30", a_public_repository());
    assert!(body(&changes[0]).get("maximum_allowed_days").is_none());
}

#[test]
fn every_declared_setting_is_actually_diffed() {
    // The merge layer has a compile-time guard against a field that is accepted
    // by the schema and then never looked at. The diff has none, so here is one:
    // every leaf the user could write, taken from the type itself, must reach
    // some request body.
    let desired: ActionsSettings = serde_json::from_value(json!({
        "enabled": false,
        "allowed_actions": "local_only",
        "sha_pinning_required": true,
        "selected_actions": {
            "github_owned_allowed": false,
            "verified_allowed": true,
            "patterns_allowed": ["docker/*"],
        },
        "default_workflow_permissions": "write",
        "can_approve_pull_request_reviews": true,
        "artifact_and_log_retention_days": 7,
        "fork_pr_contributor_approval": "all_external_contributors",
        "access_level": "organization",
        "fork_pr_workflows_private_repos": {
            "run_workflows_from_fork_pull_requests": false,
            "send_write_tokens_to_workflows": true,
            "send_secrets_and_variables": true,
            "require_approval_for_fork_pr_workflows": false,
        },
    }))
    .unwrap();

    /// Every leaf key of the serialised configuration, flattened.
    fn leaves(value: &Value, into: &mut Vec<String>) {
        for (name, value) in value.as_object().unwrap() {
            match value {
                Value::Object(_) => leaves(value, into),
                _ => into.push(name.clone()),
            }
        }
    }

    let mut declared = Vec::new();
    leaves(&serde_json::to_value(&desired).unwrap(), &mut declared);
    assert!(!declared.is_empty());

    let changes = Actions.diff(&desired, &current(json!({})), &PruneOpts::default());
    let sent: Vec<String> = changes
        .iter()
        .flat_map(|change| {
            body(change)
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();

    for name in declared {
        // Two endpoints name their single field after the endpoint rather than
        // after the setting, so the configuration cannot use the API's spelling
        // without saying `days:` and `approval_policy:` with no context.
        let expected = match name.as_str() {
            "artifact_and_log_retention_days" => "days".to_string(),
            "fork_pr_contributor_approval" => "approval_policy".to_string(),
            _ => name.clone(),
        };
        assert!(
            sent.contains(&expected),
            "`{name}` is accepted by the schema but never reaches the API"
        );
    }
}

mod validation {
    use super::*;
    use pretty_assertions::assert_eq;

    fn codes(yaml: &str) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        Actions
            .validate(&settings(yaml), &ctx)
            .into_iter()
            .map(|finding| finding.code)
            .collect()
    }

    #[test]
    fn an_allow_list_under_the_wrong_policy_is_rejected() {
        let yaml = "allowed_actions: all\nselected_actions:\n  verified_allowed: true\n";
        assert_eq!(
            codes(yaml),
            vec!["gh_settings::actions::allow_list_without_selected"]
        );
    }

    #[test]
    fn an_allow_list_without_a_policy_is_a_warning() {
        let yaml = "selected_actions:\n  verified_allowed: true\n";
        assert_eq!(
            codes(yaml),
            vec!["gh_settings::actions::allow_list_without_policy"]
        );
    }

    #[test]
    fn an_allow_list_with_the_right_policy_is_accepted() {
        let yaml = "allowed_actions: selected\nselected_actions:\n  verified_allowed: true\n";
        assert!(codes(yaml).is_empty());
    }

    #[test]
    fn a_retention_period_outside_the_accepted_range_is_rejected() {
        assert_eq!(
            codes("artifact_and_log_retention_days: 0"),
            vec!["gh_settings::actions::retention_out_of_range"]
        );
        assert_eq!(
            codes("artifact_and_log_retention_days: 401"),
            vec!["gh_settings::actions::retention_out_of_range"]
        );
        assert!(codes("artifact_and_log_retention_days: 90").is_empty());
    }
}

mod export {
    use super::*;
    use crate::github::{GitHubError, Method, Response, Result as GitHubResult};
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;

    /// Answers each endpoint from a canned map, and `404`s for anything absent.
    struct Stub(Value);

    #[async_trait]
    impl GitHubClient for Stub {
        async fn request(&self, request: Request) -> GitHubResult<Response> {
            assert_eq!(request.method, Method::Get, "export must not write");
            let key = request
                .endpoint
                .rsplit_once("actions/permissions")
                .map(|(_, rest)| rest.trim_start_matches('/'))
                .unwrap_or_default();
            let key = if key.is_empty() { "permissions" } else { key };
            match self.0.get(key) {
                Some(body) => Ok(Response::json(200, body.clone(), Vec::new())),
                None => Err(GitHubError::Api {
                    method: Method::Get,
                    endpoint: request.endpoint,
                    status: 404,
                    message: "Not Found".into(),
                    body: String::new(),
                }),
            }
        }
    }

    async fn exported(groups: Value) -> Option<Value> {
        Actions
            .export(&Stub(groups), &Target::new("o", "r"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_repository_that_answers_nothing_gets_no_section() {
        assert_eq!(exported(json!({})).await, None);
    }

    #[tokio::test]
    async fn endpoints_that_do_not_apply_are_left_out() {
        let value = exported(a_public_repository()).await.unwrap();
        assert!(value.get("access_level").is_none());
        assert!(value.get("fork_pr_workflows_private_repos").is_none());
        assert_eq!(value["enabled"], json!(true));
        assert_eq!(value["artifact_and_log_retention_days"], json!(90));
    }

    #[tokio::test]
    async fn the_retention_ceiling_is_not_exported() {
        // It describes the plan, not this repository, and is not a body
        // parameter — exporting it would put an unwritable key in the file.
        let value = exported(a_public_repository()).await.unwrap();
        assert!(value.get("maximum_allowed_days").is_none());
    }

    #[tokio::test]
    async fn an_allow_list_is_only_exported_under_a_selected_policy() {
        let mut groups = a_public_repository();
        groups["selected-actions"] = json!({"verified_allowed": true});

        // Policy is `all`: the list is a leftover GitHub ignores.
        assert!(
            exported(groups.clone())
                .await
                .unwrap()
                .get("selected_actions")
                .is_none()
        );

        groups["permissions"]["allowed_actions"] = json!("selected");
        assert_eq!(
            exported(groups).await.unwrap()["selected_actions"]["verified_allowed"],
            json!(true)
        );
    }

    #[tokio::test]
    async fn an_exported_section_round_trips_through_the_schema() {
        let value = exported(a_public_repository()).await.unwrap();
        let parsed: ActionsSettings = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.enabled, Some(true));
        assert_eq!(parsed.allowed_actions, Some(AllowedActions::All));
    }

    #[tokio::test]
    async fn an_export_produces_an_empty_plan() {
        // The real test of normalisation: whatever `export` writes must diff to
        // nothing against the state it was read from.
        let groups = a_public_repository();
        let value = exported(groups.clone()).await.unwrap();
        let desired: ActionsSettings = serde_json::from_value(value).unwrap();
        assert!(
            Actions
                .diff(&desired, &current(groups), &PruneOpts::default())
                .is_empty()
        );
    }
}
