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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
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

    live.cleanup();
}

/// A permission GitHub named, in the spelling our declarations use.
///
/// The header says `pull_requests=read`; the token UI and our tables say
/// "Pull requests: read". Underscores become spaces and the first letter is
/// capitalised — deliberately dumb, because a clever parser here would be a
/// second thing that can be wrong.
fn permission_label(term: &str) -> Option<(String, String)> {
    let (name, access) = term.trim().split_once('=')?;
    let mut name = name.trim().replace('_', " ");
    if let Some(first) = name.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Some((name, access.trim().to_string()))
}

/// Whether `requirement` covers every permission in one of the alternatives
/// GitHub offered.
///
/// `;` separates permissions that are **all** required; `,` separates
/// alternatives, any one of which suffices. That distinction is the entire
/// reason this test exists: GitHub's published table collapses both into a
/// single checkmark.
fn satisfies(requirement: &gh_settings::resources::Requirement, header: &str) -> bool {
    header.split(',').any(|alternative| {
        alternative
            .split(';')
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

    live.cleanup();

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
    fn a_semicolon_means_every_permission_is_required() {
        // Neither alone is enough, so a declaration of one must not pass.
        assert!(!satisfies(
            &Requirement::PAGES,
            "pages=write; administration=write"
        ));
        assert!(satisfies(
            &Requirement::ENVIRONMENTS,
            "administration=write; actions=read"
        ));
    }

    #[test]
    fn a_comma_means_any_one_of_them_is_enough() {
        assert!(satisfies(
            &Requirement::PAGES,
            "pages=write, administration=write"
        ));
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
