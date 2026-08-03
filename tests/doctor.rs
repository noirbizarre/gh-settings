//! End-to-end tests for `doctor`.
//!
//! The point of `doctor` (plan §6b) is to state plainly what a credential cannot
//! do, *before* the user spends an hour on an unexplained `403`. The most
//! important case is the Actions `GITHUB_TOKEN`, which structurally cannot manage
//! repository settings: the workflow `permissions:` block has no `administration`
//! key, so it is not something the user forgot to grant.

mod common;

use common::{Fixture, Sandbox, default_repository};

/// A sandbox whose stub reports the given token and scopes.
fn sandbox(token: &str, scopes: Option<&str>) -> Sandbox {
    let sandbox = Sandbox::new()
        .config("version: 1\n")
        // Also present as a header, so the fallback path stays exercised.
        .respond(
            "GET",
            "",
            match scopes {
                Some(scopes) => Fixture::ok("{}").header("x-oauth-scopes", scopes),
                None => Fixture::ok("{}"),
            },
        )
        .repository(&default_repository())
        .token(token);

    match scopes {
        Some(scopes) => sandbox.scopes(scopes),
        None => sandbox,
    }
}

#[test]
fn reports_a_healthy_classic_token() {
    let runner = sandbox("ghp_x", Some("repo, read:org")).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);

    output.expect_status(0);
    assert!(
        output.stdout.contains("gh version 2.62.0"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("as tester"), "{}", output.stdout);
    assert!(
        output.stdout.contains("repo, read:org"),
        "{}",
        output.stdout
    );
}

#[test]
fn every_resource_is_manageable_with_a_repo_scoped_token() {
    let runner = sandbox("ghp_x", Some("repo, read:org")).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);

    for resource in ["repository", "topics", "labels", "autolinks", "rulesets"] {
        assert!(
            output.stdout.contains(resource),
            "{resource} missing from the table"
        );
    }
    assert!(
        !output.stdout.contains("Administration: write"),
        "should not report a problem: {}",
        output.stdout
    );
}

#[test]
fn a_token_without_the_repo_scope_is_reported_as_blocked() {
    let runner = sandbox("ghp_x", Some("gist")).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);

    assert!(
        output.stdout.contains("missing the `repo` scope"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_fine_grained_token_reports_unknown_rather_than_guessing() {
    // Fine-grained tokens do not advertise scopes. Claiming to know would be
    // worse than admitting we cannot tell.
    let runner = sandbox("github_pat_x", None).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);

    assert!(
        output.stdout.contains("fine-grained personal access token"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("not reported by this token type"),
        "{}",
        output.stdout
    );
}

#[test]
fn strict_mode_fails_when_something_is_not_manageable() {
    let runner = sandbox("ghp_x", Some("gist")).build();
    runner
        .run(&["doctor", "-R", "o/r", "--strict"])
        .expect_status(1);
}

#[test]
fn strict_mode_succeeds_when_everything_is_manageable() {
    let runner = sandbox("ghp_x", Some("repo, read:org")).build();
    runner
        .run(&["doctor", "-R", "o/r", "--strict"])
        .expect_status(0);
}

#[test]
fn doctor_performs_no_writes() {
    let runner = sandbox("ghp_x", Some("repo")).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn doctor_needs_no_configuration_file() {
    // It has to work in exactly the situation where nothing is set up yet.
    let runner = Sandbox::new()
        .respond(
            "GET",
            "",
            Fixture::ok("{}").header("x-oauth-scopes", "repo"),
        )
        .repository(&default_repository())
        .build();

    runner.run(&["doctor", "-R", "o/r"]).expect_status(0);
}

#[test]
fn the_actions_token_is_reported_as_structurally_incapable() {
    // The headline case. `secrets.GITHUB_TOKEN` cannot manage repository
    // settings, rulesets, topics or autolinks — and this is not a permission the
    // user forgot to request, the workflow `permissions:` block has no
    // `administration` key at all. Saying so here saves an hour of confusion.
    let runner = sandbox("ghs_actionstoken", Some("issues")).build();
    let output = runner.run_with_env(&["doctor", "-R", "o/r"], &[("GITHUB_ACTIONS", "true")]);

    assert!(
        output.stdout.contains("Actions GITHUB_TOKEN"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("cannot be granted to GITHUB_TOKEN"),
        "{}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("noirbizarre.github.io/gh-settings/authentication/"),
        "{}",
        output.stdout
    );
}

#[test]
fn labels_remain_manageable_with_the_actions_token() {
    // Labels live under `Issues: write`, which GITHUB_TOKEN *can* hold. The
    // documented "labels-only" CI workflow depends on this staying true.
    let runner = sandbox("ghs_actionstoken", Some("issues")).build();
    let output = runner.run_with_env(&["doctor", "-R", "o/r"], &[("GITHUB_ACTIONS", "true")]);

    let labels_line = output
        .stdout
        .lines()
        .find(|line| line.contains("labels"))
        .unwrap_or_default()
        .to_string();
    assert!(
        !labels_line.contains("cannot be granted"),
        "labels should stay manageable: {labels_line}"
    );
}

#[test]
fn an_app_installation_token_outside_actions_is_not_restricted() {
    // Same `ghs_` prefix, entirely different capability. Only the environment
    // tells them apart.
    let runner = sandbox("ghs_appinstallation", None).build();
    let output = runner.run(&["doctor", "-R", "o/r"]);

    assert!(
        output.stdout.contains("GitHub App installation token"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("cannot be granted to GITHUB_TOKEN"),
        "{}",
        output.stdout
    );
}

#[test]
fn doctor_emits_json() {
    let runner = sandbox("ghp_x", Some("repo, read:org")).build();
    let output = runner.run(&["doctor", "-R", "o/r", "--format", "json"]);
    output.expect_status(0);

    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout was not JSON: {error}\n{}", output.stdout));

    assert_eq!(value["ok"], true);
    assert_eq!(value["authentication"]["token_kind"], "classic_pat");
    assert_eq!(value["authentication"]["account"], "tester");

    let resources = value["resources"].as_array().expect("resources");
    assert_eq!(resources.len(), 5);
    assert!(
        resources
            .iter()
            .all(|resource| resource["status"] == "manageable")
    );
}

#[test]
fn doctor_json_distinguishes_no_scopes_from_unknown_scopes() {
    // A fine-grained token cannot report its scopes. Emitting an empty list
    // would say "this token has no permissions", which is a different and
    // wrong answer. The field is omitted instead.
    let runner = sandbox("github_pat_x", None).build();
    let output = runner.run(&["doctor", "-R", "o/r", "--format", "json"]);

    let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(value["authentication"]["token_kind"], "fine_grained_pat");
    assert!(
        value["authentication"].get("scopes").is_none(),
        "unknown scopes must be absent, not an empty list: {}",
        output.stdout
    );
}

#[test]
fn doctor_json_reports_the_actions_token_as_impossible() {
    let runner = sandbox("ghs_actionstoken", Some("issues")).build();
    let output = runner.run_with_env(
        &["doctor", "-R", "o/r", "--format", "json"],
        &[("GITHUB_ACTIONS", "true")],
    );

    let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["authentication"]["token_kind"],
        "actions_github_token"
    );

    let resources = value["resources"].as_array().unwrap();
    let repository = resources
        .iter()
        .find(|resource| resource["resource"] == "repository")
        .expect("repository");
    assert_eq!(repository["status"], "impossible");
    assert!(
        repository["reason"]
            .as_str()
            .unwrap()
            .contains("cannot be granted"),
        "{repository}"
    );

    let labels = resources
        .iter()
        .find(|resource| resource["resource"] == "labels")
        .expect("labels");
    assert_eq!(labels["status"], "manageable");
}
