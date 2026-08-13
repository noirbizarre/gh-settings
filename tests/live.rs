//! The live suite: real `gh`, real repository, real GitHub.
//!
//! Every test is `#[ignore]`d and named `live_*`. They run only when
//! `GH_SETTINGS_TEST_REPO` names a repository free of managed configuration —
//! a public sandbox you own, never the one CI uses (ADR-019):
//!
//! ```sh
//! mise run test:live:setup you/sandbox
//! GH_SETTINGS_TEST_REPO=you/sandbox mise run test:live
//! ```
//!
//! The shape of each test is the same, and it is the point: after **every**
//! mutation, re-plan and assert the plan is empty. That is the idempotency
//! contract checked against reality rather than against our own fixtures — and
//! it is precisely the assertion that caught the ruleset permanent diff, which
//! the stub suite could not see.

mod common;

// The resource count below is derived rather than written down: a literal was
// correct when there were five resources and silently stopped covering the
// three added since.
use gh_settings::engine::Registry;
use gh_settings::resources::ResourceId;

// `Live` is referenced through the `live_or_skip!` macro, which spells out the
// full path so it works from any test file.

/// A settings file managing nothing but the given section.
fn only(section: &str) -> String {
    format!("version: 1\n{section}")
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_labels_create_update_and_prune() {
    let live = live_or_skip!();

    live.config(&only(
        "labels:\n  - name: gh-settings-live\n    color: ededed\n    description: created by the live suite\n",
    ));
    live.run(&["sync", "--yes", "--only", "labels"])
        .expect_status(0);
    live.run(&["plan", "--only", "labels"]).expect_up_to_date();

    // Update: colour and description.
    live.config(&only(
        "labels:\n  - name: gh-settings-live\n    color: b60205\n    description: updated by the live suite\n",
    ));
    live.run(&["sync", "--yes", "--only", "labels"])
        .expect_status(0);
    live.run(&["plan", "--only", "labels"]).expect_up_to_date();

    // Rename: must preserve the label rather than delete and recreate.
    live.config(&only(
        "labels:\n  - name: gh-settings-live\n    new_name: gh-settings-live-renamed\n    color: b60205\n",
    ));
    live.run(&["sync", "--yes", "--only", "labels"])
        .expect_status(0);

    // Prune: the label is gone from the file, and pruning is opted into.
    live.config(&only("labels:\n  prune: true\n  items: []\n"));
    live.run(&["sync", "--yes", "--only", "labels"])
        .expect_status(0);
    live.run(&["plan", "--only", "labels"]).expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_topics_are_normalised_the_way_github_normalises_them() {
    let live = live_or_skip!();

    // Deliberately not already normalised: GitHub lowercases and rewrites
    // separators, so an un-normalised comparison diffs for ever.
    live.config(&only("topics:\n  - Live_Test\n  - GH-Settings\n"));
    live.run(&["sync", "--yes", "--only", "topics"])
        .expect_status(0);
    live.run(&["plan", "--only", "topics"]).expect_up_to_date();

    live.config(&only("topics:\n  prune: true\n  items: []\n"));
    live.run(&["sync", "--yes", "--only", "topics"])
        .expect_status(0);
    live.run(&["plan", "--only", "topics"]).expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_autolinks_recreate_on_change() {
    let live = live_or_skip!();

    live.config(&only(
        "autolinks:\n  - key_prefix: LIVE-\n    url_template: https://example.com/a/<num>\n    is_alphanumeric: false\n",
    ));
    live.run(&["sync", "--yes", "--only", "autolinks"])
        .expect_status(0);
    live.run(&["plan", "--only", "autolinks"])
        .expect_up_to_date();

    // There is no update endpoint, so this exercises delete-then-create. The
    // ordering matters: creating first would collide on the prefix.
    live.config(&only(
        "autolinks:\n  - key_prefix: LIVE-\n    url_template: https://example.com/b/<num>\n    is_alphanumeric: false\n",
    ));
    live.run(&["sync", "--yes", "--only", "autolinks"])
        .expect_status(0);
    live.run(&["plan", "--only", "autolinks"])
        .expect_up_to_date();

    live.config(&only("autolinks:\n  prune: true\n  items: []\n"));
    live.run(&["sync", "--yes", "--only", "autolinks"])
        .expect_status(0);
    live.run(&["plan", "--only", "autolinks"])
        .expect_up_to_date();
}

/// A ruleset with every `pull_request` parameter supplied.
///
/// GitHub requires the complete set: omitting one yields
/// `Invalid property /rules/1: data matches no possible input`, which names
/// neither the rule nor the field. See [`live_an_incomplete_rule_is_rejected`].
fn ruleset(reviews: u32) -> String {
    only(&format!(
        "rulesets:\n  - name: gh-settings-live\n    target: branch\n    enforcement: active\n    conditions:\n      ref_name:\n        include: [\"~DEFAULT_BRANCH\"]\n    rules:\n      - type: non_fast_forward\n      - type: pull_request\n        parameters:\n          required_approving_review_count: {reviews}\n          dismiss_stale_reviews_on_push: false\n          require_code_owner_review: false\n          require_last_push_approval: false\n          required_review_thread_resolution: false\n"
    ))
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_rulesets_create_update_and_prune() {
    let live = live_or_skip!();

    live.config(&ruleset(1));
    live.run(&["sync", "--yes", "--only", "rulesets"])
        .expect_status(0);

    // The assertion that found the 0.1.0 permanent diff: GitHub defaults
    // `required_reviewers` and `allowed_merge_methods`, which we never sent, so
    // comparing parameter objects wholesale reported a change on every run.
    live.run(&["plan", "--only", "rulesets"])
        .expect_up_to_date();

    // Update exercises `PUT /rulesets/{id}` and the canonical rule ordering.
    live.config(&ruleset(2));
    live.run(&["sync", "--yes", "--only", "rulesets"])
        .expect_status(0);
    live.run(&["plan", "--only", "rulesets"])
        .expect_up_to_date();

    live.config(&only("rulesets:\n  prune: true\n  items: []\n"));
    live.run(&["sync", "--yes", "--only", "rulesets"])
        .expect_status(0);
    live.run(&["plan", "--only", "rulesets"])
        .expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_an_incomplete_rule_is_rejected() {
    // Pins a GitHub behaviour we learned the hard way: rule parameters are all
    // or nothing. If this ever starts passing, GitHub has relaxed the rule and
    // the documentation should say so.
    let live = live_or_skip!();

    live.config(&only(
        "rulesets:\n  - name: gh-settings-live-partial\n    rules:\n      - type: pull_request\n        parameters:\n          required_approving_review_count: 1\n",
    ));

    let output = live.run(&["sync", "--yes", "--only", "rulesets"]);
    output.expect_status(1);
    assert!(
        output.stdout.contains("422") || output.stderr.contains("422"),
        "expected a 422 for an incomplete rule\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_repository_security_travels_in_its_own_request() {
    let live = live_or_skip!();

    // `security_and_analysis` is rejected when sent alongside ordinary fields,
    // so this asserts the two-request split works against the real API.
    live.config(&only(
        "repository:\n  description: gh-settings live suite\n  security:\n    secret_scanning: true\n    secret_scanning_push_protection: true\n",
    ));
    live.run(&["sync", "--yes", "--only", "repository"])
        .expect_status(0);
    live.run(&["plan", "--only", "repository"])
        .expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_export_round_trips_to_an_empty_plan() {
    // The migration promise: point it at a repository, export, and the result
    // must describe that repository exactly.
    let live = live_or_skip!();

    live.config(&only(
        "labels:\n  - name: gh-settings-live\n    color: ededed\ntopics:\n  - gh-settings-live\n",
    ));
    live.run(&["sync", "--yes"]).expect_status(0);

    live.run(&["export", "--force"]).expect_status(0);
    live.run(&["validate"]).expect_status(0);
    live.run(&["plan"]).expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_plan_never_writes() {
    let live = live_or_skip!();

    live.config(&only(
        "labels:\n  - name: gh-settings-live-plan-only\n    color: ededed\n",
    ));

    // Exit 2 is drift, not failure.
    live.run(&["plan", "--only", "labels"]).expect_status(2);
    // And nothing was created, so the same plan is still pending.
    live.run(&["plan", "--only", "labels"]).expect_status(2);
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_doctor_reports_the_real_credential() {
    let live = live_or_skip!();

    let output = live.run(&["doctor", "--format", "json"]);
    output.expect_status(0);

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("doctor did not emit JSON: {error}\n{}", output.stdout));

    assert_eq!(value["ok"], true, "{}", output.stdout);
    assert!(value["gh_version"].is_string());
    // The credential's identity, and the only place it is visible. It decides
    // whether `live_declared_permissions_match_what_github_accepts` can run at
    // all, so a run that cannot classify the token should say so here first.
    assert!(
        value["authentication"]["token_kind"].is_string(),
        "{}",
        output.stdout
    );
    assert_eq!(
        value["resources"].as_array().map(Vec::len),
        Some(Registry::default().all().count()),
        "{}",
        output.stdout
    );
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_environments_carry_their_protection_rules() {
    let live = live_or_skip!();

    live.config(&only("environments:\n  - name: gh-settings-live\n"));
    live.run(&["sync", "--yes", "--only", "environments"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments"])
        .expect_up_to_date();

    // Every protection rule at once: the wait timer, the branch policy and the
    // explicit `null` reviewers state that the stub suite can only simulate.
    live.config(&only(
        "environments:\n  - name: gh-settings-live\n    wait_timer: 5\n    reviewers: []\n    deployment_branch_policy:\n      branches: [\"main\", \"release/*\"]\n      tags: [\"v*\"]\n",
    ));
    live.run(&["sync", "--yes", "--only", "environments"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments"])
        .expect_up_to_date();

    // Removing a pattern and keeping the rest: the delete goes by server id,
    // which only a real response supplies.
    live.config(&only(
        "environments:\n  - name: gh-settings-live\n    wait_timer: 5\n    reviewers: []\n    deployment_branch_policy:\n      branches: [\"main\"]\n",
    ));
    live.run(&["sync", "--yes", "--only", "environments"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments"])
        .expect_up_to_date();

    // Back to zero, which GitHub stores as no rule at all rather than as zero.
    live.config(&only(
        "environments:\n  - name: gh-settings-live\n    wait_timer: 0\n    deployment_branch_policy: null\n",
    ));
    live.run(&["sync", "--yes", "--only", "environments"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments"])
        .expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_variables_at_both_scopes() {
    let live = live_or_skip!();

    // The environment does not exist yet when the plan is computed, so this
    // also exercises the 404-means-no-variables path against real GitHub.
    live.config(&only(
        "variables:\n  - name: GH_SETTINGS_LIVE\n    value: one\nenvironments:\n  - name: gh-settings-live\n    variables:\n      - name: GH_SETTINGS_LIVE\n        value: scoped\n",
    ));
    live.run(&["sync", "--yes", "--only", "environments,variables"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments,variables"])
        .expect_up_to_date();

    // The same name at both scopes must stay two distinct variables.
    live.config(&only(
        "variables:\n  - name: GH_SETTINGS_LIVE\n    value: two\nenvironments:\n  - name: gh-settings-live\n    variables:\n      - name: GH_SETTINGS_LIVE\n        value: scoped\n",
    ));
    live.run(&["sync", "--yes", "--only", "environments,variables"])
        .expect_status(0);
    live.run(&["plan", "--only", "environments,variables"])
        .expect_up_to_date();

    // GitHub echoes names uppercased; a lowercase declaration must match rather
    // than be planned as a creation that then fails with a 409.
    live.config(&only(
        "variables:\n  - name: gh_settings_live\n    value: two\n",
    ));
    live.run(&["plan", "--only", "variables"])
        .expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_pages_enable_and_update() {
    // The one test that can settle what `POST /pages` and `PUT /pages` actually
    // accept — in particular that the settings `POST` refuses do land in the
    // follow-up `PUT`, and that a `source` may travel without a `build_type`.
    //
    // Deliberately no `cname`: a custom domain needs DNS that a sandbox
    // repository does not have, and `https_enforced` cannot be set until GitHub
    // has issued a certificate for it.
    let live = live_or_skip!();

    live.config(&only("pages:\n  build_type: workflow\n"));
    live.run(&["sync", "--yes", "--only", "pages"])
        .expect_status(0);
    live.run(&["plan", "--only", "pages"]).expect_up_to_date();

    // Switch to a branch build: `source` and `build_type` must travel together.
    live.config(&only(
        "pages:\n  build_type: legacy\n  source:\n    branch: gh-pages\n    path: /\n",
    ));
    live.run(&["sync", "--yes", "--only", "pages"])
        .expect_status(0);
    live.run(&["plan", "--only", "pages"]).expect_up_to_date();

    // The normalisation contract: `docs` and `/docs` are the same directory.
    live.config(&only(
        "pages:\n  build_type: legacy\n  source:\n    branch: gh-pages\n    path: docs\n",
    ));
    live.run(&["sync", "--yes", "--only", "pages"])
        .expect_status(0);
    live.run(&["plan", "--only", "pages"]).expect_up_to_date();
}

/// A permission GitHub named, in the spelling our declarations use.
///
/// The header says `pull_requests=read`; the token UI and our tables say
/// "Pull requests: read". Underscores become spaces and the first letter is
/// capitalised — deliberately dumb, because a clever parser here would be a
/// second thing that can be wrong.
fn permission_label(term: &str) -> Option<(String, String)> {
    let (name, access) = term.trim().split_once('=')?;
    let name = match name.trim() {
        // The header's identifier is not always the name the token UI and
        // GitHub's own reference table use. Repository variables are
        // `actions_variables` on the wire and "Variables" everywhere a human
        // reads them.
        "actions_variables" => "Variables".to_string(),
        other => {
            let mut name = other.replace('_', " ");
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            name
        }
    };
    Some((name, access.trim().to_string()))
}

/// Whether `requirement` covers every permission in one of the alternatives
/// GitHub offered.
///
/// The value is "a comma separated list of the permissions that are required.
/// Occasionally, you can choose from multiple permission sets. In these cases,
/// multiple comma-separated lists will be separated by a semicolon." So `,`
/// means **and**, `;` means **or** — the opposite way round from the intuition,
/// and the distinction is the entire reason this test exists: GitHub's
/// published table collapses both into a single checkmark.
fn satisfies(requirement: &gh_settings::resources::Requirement, header: &str) -> bool {
    header.split(';').any(|alternative| {
        alternative
            .split(',')
            .filter_map(permission_label)
            .all(|(name, access)| {
                requirement.fine_grained.iter().any(|declared| {
                    declared.name == name
                        // Write implies read within a category, so a resource
                        // that declares write is not under-declared when
                        // GitHub only asks for read.
                        && (declared.access.label() == access || declared.access.label() == "write")
                })
            })
    })
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_declared_permissions_match_what_github_accepts() {
    // The permission tables in `src/resources/requirement.rs` are read off
    // GitHub's published reference by hand, and that reference is ambiguous in
    // places: several endpoints are listed under two permissions with a marker
    // meaning either "both" or "either", without saying which.
    //
    // `X-Accepted-GitHub-Permissions` is GitHub answering the same question
    // unambiguously, per request. It is only sent to fine-grained tokens.
    let live = live_or_skip!();

    if !live.credential_is_fine_grained() {
        eprintln!(
            "skipped: X-Accepted-GitHub-Permissions is only sent to fine-grained \
             tokens; this credential is something else"
        );
        return;
    }

    // An environment must exist before its variables can be asked about.
    live.config(&only("environments:\n  - name: gh-settings-live\n"));
    live.run(&["sync", "--yes", "--only", "environments"])
        .expect_status(0);

    let repo = live.repo().to_string();
    let mut probes: Vec<(ResourceId, &str, String, Vec<&str>)> = vec![
        (
            ResourceId::Environments,
            "GET",
            format!("repos/{repo}/environments"),
            vec![],
        ),
        (
            ResourceId::Variables,
            "GET",
            format!("repos/{repo}/actions/variables"),
            vec![],
        ),
        (
            ResourceId::Variables,
            "GET",
            format!("repos/{repo}/environments/gh-settings-live/variables"),
            vec![],
        ),
        (
            ResourceId::Labels,
            "GET",
            format!("repos/{repo}/labels"),
            vec![],
        ),
        (
            ResourceId::Rulesets,
            "GET",
            format!("repos/{repo}/rulesets"),
            vec![],
        ),
        (
            ResourceId::Pages,
            "GET",
            format!("repos/{repo}/pages"),
            vec![],
        ),
    ];

    // Every Actions settings endpoint. The `Requirement::ACTIONS` fine-grained
    // mapping is the one declaration in this codebase that is still
    // `unverified`, because GitHub documents the 2025 endpoints against an
    // "Actions policies" permission that appears in no published table. These
    // probes are how that gets settled.
    //
    // Reads only: the writes here take effect, and a probe is not allowed to
    // change the sandbox as a side effect of asking a question.
    for suffix in [
        "",
        "/selected-actions",
        "/workflow",
        "/artifact-and-log-retention",
        "/fork-pr-contributor-approval",
        "/access",
        "/fork-pr-workflows-private-repos",
    ] {
        probes.push((
            ResourceId::Actions,
            "GET",
            format!("repos/{repo}/actions/permissions{suffix}"),
            vec![],
        ));
    }

    // The open question: are the Pages writes `Pages: write` alone, or that
    // *and* `Administration: write`? Probe whichever method applies — `PUT`
    // against a repository with no site answers 404, which would tell us
    // nothing — with a body GitHub must reject, so nothing is created.
    let method = if live.api(&["repos", &repo, "pages"]).is_ok() {
        "PUT"
    } else {
        "POST"
    };
    probes.push((
        ResourceId::Pages,
        method,
        format!("repos/{repo}/pages"),
        vec!["-f", "build_type=nonsense"],
    ));

    let registry = Registry::default();
    let mut failures = Vec::new();

    for (id, method, path, body) in &probes {
        let Some(header) = live.accepted_permissions(method, path, body) else {
            failures.push(format!(
                "{method} {path}: no X-Accepted-GitHub-Permissions header, \
                 so this probe proved nothing"
            ));
            continue;
        };

        let requirement = registry
            .all()
            .find(|resource| resource.id() == *id)
            .expect("probe names a registered resource")
            .requirement();

        // Printed whatever happens: the value is the finding, and a rerun to
        // see it costs a nightly cycle.
        eprintln!("{method} {path} -> {header}");

        if !satisfies(requirement, &header) {
            failures.push(format!(
                "{id} declares [{}] but {method} {path} accepts [{header}]",
                requirement.fine_grained_summary()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "declared permissions disagree with GitHub:\n{}",
        failures.join("\n")
    );
}

/// Tests for the header parser above.
///
/// Not named `live_*`, so they run in the ordinary suite: the parsing is the
/// part most likely to be wrong, and it is exercised only on a fine-grained
/// token, which the live suite usually does not have. A mechanical bug here
/// would surface nightly as a permission "finding" that is nothing of the kind.
#[cfg(test)]
mod accepted_permissions {
    use super::{permission_label, satisfies};
    use gh_settings::resources::Requirement;

    #[test]
    fn a_term_is_split_into_a_name_and_an_access_level() {
        assert_eq!(
            permission_label("administration=write"),
            Some(("Administration".to_string(), "write".to_string()))
        );
    }

    #[test]
    fn underscores_become_spaces_the_way_the_token_ui_spells_them() {
        assert_eq!(
            permission_label("pull_requests=read"),
            Some(("Pull requests".to_string(), "read".to_string()))
        );
    }

    #[test]
    fn a_comma_means_every_permission_is_required() {
        // Verbatim from the probe: `PUT /repos/{o}/{r}/pages` answers
        // `pages=write,administration=write`, and both are needed. Declaring
        // one of them is not enough.
        assert!(!satisfies(
            &Requirement::ISSUES,
            "pages=write,administration=write"
        ));
        assert!(satisfies(
            &Requirement::ENVIRONMENTS,
            "administration=write,actions=read"
        ));
    }

    #[test]
    fn a_semicolon_separates_alternatives() {
        // `GET /repos/{o}/{r}/labels` answers `issues=read; pull_requests=read`:
        // either alone suffices, which is why declaring Issues is correct.
        assert!(satisfies(
            &Requirement::ISSUES,
            "issues=read; pull_requests=read"
        ));
    }

    #[test]
    fn the_wire_name_for_repository_variables_is_actions_variables() {
        // The header says `actions_variables`; the token UI and GitHub's own
        // reference table both say "Variables".
        assert_eq!(
            permission_label("actions_variables=read"),
            Some(("Variables".to_string(), "read".to_string()))
        );
        assert!(satisfies(&Requirement::VARIABLES, "actions_variables=read"));
    }

    /// The answers GitHub actually gave, copied from the first run of the live
    /// probe against the sandbox.
    ///
    /// The probe itself only runs on a fine-grained token, which most runs do
    /// not have. Recording its output means the mappings are still checked on
    /// every ordinary `cargo test` — and that a change to the parser has to
    /// keep agreeing with reality, not just with itself.
    #[test]
    fn the_recorded_answers_are_all_satisfied() {
        for (header, requirement) in [
            ("actions=read", &Requirement::ENVIRONMENTS),
            ("actions=read", &Requirement::VARIABLES),
            ("actions_variables=read", &Requirement::VARIABLES),
            ("environments=read", &Requirement::VARIABLES),
            ("environments=read", &Requirement::ENVIRONMENTS),
            ("issues=read; pull_requests=read", &Requirement::ISSUES),
            ("metadata=read", &Requirement::ADMINISTRATION),
            ("pages=read", &Requirement::PAGES),
            ("pages=write,administration=write", &Requirement::PAGES),
        ] {
            assert!(
                satisfies(requirement, header),
                "[{}] does not cover {header}",
                requirement.fine_grained_summary()
            );
        }
    }

    #[test]
    fn declaring_write_covers_an_endpoint_that_only_needs_read() {
        assert!(satisfies(
            &Requirement::ADMINISTRATION,
            "administration=read"
        ));
    }

    #[test]
    fn a_permission_we_never_declared_is_not_satisfied() {
        assert!(!satisfies(&Requirement::ISSUES, "administration=write"));
    }
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_actions_settings_round_trip() {
    let live = live_or_skip!();

    // Deliberately not `enabled: false`: turning Actions off makes several of
    // the sibling endpoints stop answering, and a test that breaks the next
    // test is not a test.
    live.config(&only(
        "actions:\n  enabled: true\n  allowed_actions: local_only\n  \
         default_workflow_permissions: read\n  can_approve_pull_request_reviews: false\n  \
         artifact_and_log_retention_days: 30\n  \
         fork_pr_contributor_approval: first_time_contributors\n",
    ));
    live.run(&["sync", "--yes", "--only", "actions"])
        .expect_status(0);
    // The real assertion. A normalisation miss shows up here and nowhere else.
    live.run(&["plan", "--only", "actions"]).expect_up_to_date();

    // The allow list, which GitHub only accepts once the policy is `selected`.
    live.config(&only(
        "actions:\n  allowed_actions: selected\n  selected_actions:\n    \
         github_owned_allowed: true\n    verified_allowed: false\n    \
         patterns_allowed:\n      - docker/*\n      - actions/checkout@v4\n",
    ));
    live.run(&["sync", "--yes", "--only", "actions"])
        .expect_status(0);
    live.run(&["plan", "--only", "actions"]).expect_up_to_date();

    // An export of the live repository must plan to nothing against it.
    let exported = live.run(&["export", "--stdout"]);
    exported.expect_status(0);
    assert!(
        exported.stdout.contains("actions:"),
        "export omitted the actions section:\n{}",
        exported.stdout
    );
    live.config(&exported.stdout);
    live.run(&["plan", "--only", "actions"]).expect_up_to_date();
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_actions_private_only_settings_are_reported_not_swallowed() {
    // The sandbox is public (ADR-019), so `/access` does not exist on it. The
    // point of this test is that gh-settings says so rather than reporting
    // success: a read we could not perform must never become "up to date".
    let live = live_or_skip!();

    live.config(&only("actions:\n  access_level: organization\n"));

    // Pending, not up to date: the plan proposes the change it cannot verify.
    live.run(&["plan", "--only", "actions"]).expect_status(2);

    let output = live.run(&["sync", "--yes", "--only", "actions"]);
    assert_ne!(
        output.status, 0,
        "a write GitHub rejected was reported as success:\n{}\n{}",
        output.stdout, output.stderr
    );
}

#[test]
#[ignore = "live: requires GH_SETTINGS_TEST_REPO"]
fn live_actions_private_only_settings_apply_when_private() {
    // `/access` and `/fork-pr-workflows-private-repos` exist only on private
    // repositories, and ADR-019 requires the sandbox to be public. So flip it,
    // prove the happy path, and flip it back — from a guard, so a panic in the
    // middle cannot strand the sandbox private.
    let live = live_or_skip!();

    struct Visibility<'a>(&'a str);

    impl Visibility<'_> {
        fn set(&self, private: bool) {
            let status = std::process::Command::new("gh")
                .args(["api", &format!("repos/{}", self.0)])
                .args(["--method", "PATCH", "--silent"])
                .args(["-F", &format!("private={private}")])
                .status();
            if !matches!(status, Ok(status) if status.success()) {
                eprintln!("could not set private={private} on {}", self.0);
            }
        }
    }

    impl Drop for Visibility<'_> {
        fn drop(&mut self) {
            self.set(false);
        }
    }

    let visibility = Visibility(live.repo());
    visibility.set(true);

    // GitHub needs a moment after a visibility change before the private-only
    // endpoints answer. Asking once and giving up would make this test flake.
    let mut ready = false;
    for _ in 0..10 {
        if live
            .api(&["repos", live.repo(), "actions", "permissions", "access"])
            .is_ok()
        {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    assert!(
        ready,
        "the sandbox never became private enough for /access to answer; \
         it may be a personal repository on a plan without this feature"
    );

    live.config(&only(
        "actions:\n  access_level: user\n  fork_pr_workflows_private_repos:\n    \
         run_workflows_from_fork_pull_requests: true\n    \
         require_approval_for_fork_pr_workflows: true\n",
    ));
    live.run(&["sync", "--yes", "--only", "actions"])
        .expect_status(0);
    live.run(&["plan", "--only", "actions"]).expect_up_to_date();
}
