//! End-to-end tests for `export`.
//!
//! The acceptance criterion for export is the round trip: `export` followed by
//! `plan` must report no changes. Anything less makes migrating an existing
//! repository a manual clean-up job.

mod common;

use common::{Sandbox, default_repository};

fn populated() -> Sandbox {
    Sandbox::new()
        .repository(&default_repository())
        .no_pages()
        .get("repos/o/r/topics", r#"{"names": ["rust", "github-cli"]}"#)
        .get(
            "repos/o/r/labels",
            r#"[{"name": "bug", "color": "d73a4a", "description": "Something isn't working"}]"#,
        )
        .get(
            "repos/o/r/autolinks",
            r#"[{"id": 1, "key_prefix": "OPS-", "url_template": "https://jira.example.com/browse/<num>", "is_alphanumeric": false}]"#,
        )
}

#[test]
fn writes_the_conventional_location() {
    let runner = populated().build();
    runner.run(&["export", "-R", "o/r"]).expect_status(0);

    let written = common::read(runner.path(), ".github/settings.yml");
    assert!(written.contains("version: 1"), "{written}");
    assert!(written.contains("bug"), "{written}");
    assert!(written.contains("OPS-"), "{written}");
}

#[test]
fn includes_the_schema_annotation() {
    // This one line is what gives users completion and validation in their
    // editor, so it must survive every export.
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);

    assert!(
        output.stdout.contains(
            "# $schema: https://noirbizarre.github.io/gh-settings/schema/v1/settings.json"
        ),
        "{}",
        output.stdout
    );
}

#[test]
fn exported_configuration_round_trips_to_an_empty_plan() {
    // The acceptance criterion for the whole command.
    let runner = populated().build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn exported_configuration_is_valid() {
    let runner = populated().build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);
    runner.run(&["validate", "-R", "o/r"]).expect_status(0);
}

/// A repository whose ruleset is bypassed by organisation administrators.
///
/// GitHub reports that actor as `{actor_id: 1, actor_type: OrganizationAdmin}`,
/// which the configuration schema spells `organization_admin: true`. Emitting
/// both is an ambiguous bypass actor and `validate` rejects it.
fn with_an_organization_admin_bypass() -> Sandbox {
    populated()
        .get(
            "repos/o/r/rulesets",
            r#"[{"id": 42, "name": "main", "target": "branch", "enforcement": "active"}]"#,
        )
        .get(
            "repos/o/r/rulesets/42",
            r#"{
                "id": 42,
                "name": "main",
                "target": "branch",
                "enforcement": "active",
                "bypass_actors": [
                    {"actor_id": 1, "actor_type": "OrganizationAdmin", "bypass_mode": "always"}
                ],
                "rules": [{"type": "non_fast_forward"}]
            }"#,
        )
}

#[test]
fn an_organization_admin_bypass_exports_in_the_form_the_schema_accepts() {
    let runner = with_an_organization_admin_bypass().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);

    assert!(
        output.stdout.contains("organization_admin"),
        "the readable form is missing:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("actor_id"),
        "the resolved form was emitted alongside it:\n{}",
        output.stdout
    );
}

#[test]
fn an_exported_organization_admin_bypass_validates_and_round_trips() {
    let runner = with_an_organization_admin_bypass().build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    runner.run(&["validate", "-R", "o/r"]).expect_status(0);

    let output = runner.run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn refuses_to_overwrite_without_force() {
    // Export cannot preserve comments, so silently replacing a hand-written file
    // would destroy work.
    let runner = populated()
        .config("version: 1\n# a treasured comment\n")
        .build();

    let output = runner.run(&["export", "-R", "o/r"]);
    output.expect_status(1);
    assert!(
        output.stderr.contains("--force"),
        "stderr:\n{}",
        output.stderr
    );
    assert!(
        common::read(runner.path(), ".github/settings.yml").contains("treasured"),
        "the existing file was modified"
    );
}

#[test]
fn force_overwrites() {
    let runner = populated()
        .config("version: 1\n# a treasured comment\n")
        .build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    assert!(!common::read(runner.path(), ".github/settings.yml").contains("treasured"));
}

#[test]
fn omits_sections_the_repository_has_nothing_for() {
    // An exported file full of `topics: []` is noise, and with pruning enabled it
    // would also be a loaded gun.
    let runner = Sandbox::new()
        .repository(&default_repository())
        .no_pages()
        .get("repos/o/r/topics", r#"{"names": []}"#)
        .get("repos/o/r/labels", "[]")
        .get("repos/o/r/autolinks", "[]")
        .build();

    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(!output.stdout.contains("topics:"), "{}", output.stdout);
    assert!(!output.stdout.contains("labels:"), "{}", output.stdout);
    assert!(
        !output.stdout.contains("environments:"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("variables:"), "{}", output.stdout);
}

#[test]
fn never_exports_the_archived_flag() {
    // A one-way, destructive flag has no business in a file people copy between
    // repositories.
    let runner = Sandbox::new()
        .repository(
            &common::repository_with(&[("description", "x")])
                .replace(r#""archived":false"#, r#""archived":true"#),
        )
        .no_pages()
        .build();

    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(!output.stdout.contains("archived"), "{}", output.stdout);
}

#[test]
fn export_performs_no_writes() {
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    assert!(output.writes().is_empty(), "{:?}", output.writes());
}

#[test]
fn only_restricts_what_is_exported() {
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout", "--only", "labels"]);
    output.expect_status(0);

    assert!(output.stdout.contains("labels:"), "{}", output.stdout);
    assert!(!output.stdout.contains("autolinks:"), "{}", output.stdout);
}

// --- rendering --------------------------------------------------------------

#[test]
fn export_renders_a_populated_repository() {
    // The file a user's first migration produces. Field order, quoting and the
    // schema annotation are all part of it.
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert_cli_snapshot!(output.stdout);
}

#[test]
fn export_renders_a_repository_with_nothing_to_export() {
    let runner = Sandbox::new()
        .repository(&default_repository())
        .no_pages()
        .build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert_cli_snapshot!(output.stdout);
}

/// An environment with every protection rule set, as the API reports one.
const ENVIRONMENTS_BODY: &str = r#"{
    "total_count": 1,
    "environments": [{
        "name": "production",
        "protection_rules": [
            {"type": "wait_timer", "wait_timer": 30},
            {
                "type": "required_reviewers",
                "prevent_self_review": true,
                "reviewers": [{"type": "Team", "reviewer": {"id": 7, "slug": "eng"}}]
            },
            {"type": "branch_policy"}
        ],
        "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}
    }]
}"#;

fn with_environments() -> Sandbox {
    populated()
        .get("repos/o/r/environments?per_page=100", ENVIRONMENTS_BODY)
        .get(
            "repos/o/r/environments/production/deployment-branch-policies?per_page=100",
            r#"{"total_count": 1, "branch_policies": [{"id": 9, "name": "main", "type": "branch"}]}"#,
        )
        .get(
            "repos/o/r/environments/production/variables?per_page=100",
            r#"{"total_count": 1, "variables": [{"name": "URL", "value": "https://example.com"}]}"#,
        )
        .get(
            "repos/o/r/actions/variables?per_page=100",
            r#"{"total_count": 1, "variables": [{"name": "REGION", "value": "eu"}]}"#,
        )
}

#[test]
fn environment_variables_are_exported_inside_their_environment() {
    // One resource writes both scopes, but the engine files an exported section
    // under the resource's own name — so the `environments` resource is the only
    // thing that can emit these (ADR-018).
    let output = with_environments()
        .build()
        .run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);

    let value: serde_norway::Value =
        serde_norway::from_str(&output.stdout).expect("valid configuration");
    let environment = &value["environments"][0];

    assert_eq!(environment["name"], "production");
    assert_eq!(environment["variables"][0]["name"], "URL");
    assert_eq!(environment["variables"][0]["value"], "https://example.com");
}

#[test]
fn repository_variables_are_exported_at_the_top_level() {
    let output = with_environments()
        .build()
        .run(&["export", "-R", "o/r", "--stdout"]);

    let value: serde_norway::Value =
        serde_norway::from_str(&output.stdout).expect("valid configuration");
    assert_eq!(value["variables"][0]["name"], "REGION");
    assert_eq!(value["variables"][0]["value"], "eu");
}

#[test]
fn exported_reviewers_use_slugs_never_identifiers() {
    // Numeric identifiers mean nothing to a reader and nothing at all in
    // another organisation, so an exported file carrying them would not be
    // reusable — which is the point of exporting.
    let output = with_environments()
        .build()
        .run(&["export", "-R", "o/r", "--stdout"]);

    assert!(output.stdout.contains("team: eng"), "{}", output.stdout);
    assert!(!output.stdout.contains("id:"), "{}", output.stdout);
}

#[test]
fn exported_environments_round_trip_to_an_empty_plan() {
    // The acceptance criterion: anything not normalised on both sides shows up
    // here as a permanent diff.
    let sandbox = with_environments();
    let runner = sandbox.build();
    runner.run(&["export", "-R", "o/r"]).expect_status(0);

    let exported = common::read(runner.path(), ".github/settings.yml");
    let output = Sandbox::new()
        .config(&exported)
        .repository(&default_repository())
        .no_pages()
        .get("repos/o/r/topics", r#"{"names": ["rust", "github-cli"]}"#)
        .get(
            "repos/o/r/labels",
            r#"[{"name": "bug", "color": "d73a4a", "description": "Something isn't working"}]"#,
        )
        .get(
            "repos/o/r/autolinks",
            r#"[{"id": 1, "key_prefix": "OPS-", "url_template": "https://jira.example.com/browse/<num>", "is_alphanumeric": false}]"#,
        )
        .get("repos/o/r/environments?per_page=100", ENVIRONMENTS_BODY)
        .get(
            "repos/o/r/environments/production/deployment-branch-policies?per_page=100",
            r#"{"total_count": 1, "branch_policies": [{"id": 9, "name": "main", "type": "branch"}]}"#,
        )
        .get(
            "repos/o/r/environments/production/variables?per_page=100",
            r#"{"total_count": 1, "variables": [{"name": "URL", "value": "https://example.com"}]}"#,
        )
        .get(
            "repos/o/r/actions/variables?per_page=100",
            r#"{"total_count": 1, "variables": [{"name": "REGION", "value": "eu"}]}"#,
        )
        .get("orgs/o/teams/eng", r#"{"id": 7}"#)
        .build()
        .run(&["plan", "-R", "o/r"]);

    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn a_repository_without_pages_exports_no_pages_section() {
    // `GET /pages` answers 404 when Pages is off, and an absent site is a state
    // to describe by saying nothing, not by writing an empty block.
    let runner = populated().build();
    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(!output.stdout.contains("pages:"), "{}", output.stdout);
}

#[test]
fn an_exported_site_round_trips_to_an_empty_plan() {
    let site = r#"{"build_type": "legacy", "source": {"branch": "gh-pages", "path": "/docs"}, "cname": "docs.example.com", "https_enforced": true, "public": true}"#;

    let runner = Sandbox::new()
        .repository(&default_repository())
        .get("repos/o/r/pages", site)
        .build();
    runner
        .run(&["export", "-R", "o/r", "--force"])
        .expect_status(0);

    let exported = common::read(runner.path(), ".github/settings.yml");
    assert!(exported.contains("pages:"), "{exported}");

    let output = Sandbox::new()
        .config(&exported)
        .repository(&default_repository())
        .get("repos/o/r/pages", site)
        .build()
        .run(&["plan", "-R", "o/r"]);
    output.expect_status(0);
    assert!(output.stdout.contains("up to date"), "{}", output.stdout);
}

#[test]
fn a_site_without_a_custom_domain_exports_no_cname() {
    // Exporting `cname: null` would turn a description of the current state into
    // an instruction to clear something.
    let runner = Sandbox::new()
        .repository(&default_repository())
        .get(
            "repos/o/r/pages",
            r#"{"build_type": "workflow", "cname": null}"#,
        )
        .build();

    let output = runner.run(&["export", "-R", "o/r", "--stdout"]);
    output.expect_status(0);
    assert!(
        output.stdout.contains("build_type: workflow"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("cname"), "{}", output.stdout);
}
