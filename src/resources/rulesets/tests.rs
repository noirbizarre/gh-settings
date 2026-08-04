//! Ruleset resource tests.
//!
//! Rulesets are the abstraction's stress test (plan §10, M3). The properties that
//! matter most are normalisation ones: the API returns server-only fields and
//! arbitrarily ordered rules, and any of those leaking into the comparison would
//! produce a plan that never converges.

use super::*;
use crate::config::SpanIndex;
use pretty_assertions::assert_eq;
use serde_json::json;

fn plan(desired_sets: Vec<Ruleset>, current_sets: Vec<(u64, Ruleset)>, prune: bool) -> Vec<Change> {
    let desired = Desired {
        rulesets: desired_sets.iter().map(Ruleset::normalized).collect(),
        prune,
    };
    let current = Current {
        rulesets: current_sets
            .into_iter()
            .map(|(id, ruleset)| (ruleset.name.clone(), (id, ruleset.normalized())))
            .collect(),
    };
    let mut changes = Rulesets.diff(&desired, &current, &PruneOpts::default());
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    changes
}

fn protection() -> Ruleset {
    Ruleset::new("main-protection").with_rules(vec![
        Rule::with(
            "pull_request",
            json!({"required_approving_review_count": 1}),
        ),
        Rule::new("non_fast_forward"),
    ])
}

#[test]
fn creates_a_missing_ruleset() {
    let changes = plan(vec![protection()], vec![], false);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "create ruleset main-protection");
}

#[test]
fn an_identical_ruleset_produces_no_change() {
    let changes = plan(vec![protection()], vec![(1, protection())], false);
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn rule_order_does_not_matter() {
    // The API returns rules in an arbitrary order; without canonical sorting
    // this would diff on every single run.
    let reordered = Ruleset::new("main-protection").with_rules(vec![
        Rule::new("non_fast_forward"),
        Rule::with(
            "pull_request",
            json!({"required_approving_review_count": 1}),
        ),
    ]);
    let changes = plan(vec![protection()], vec![(1, reordered)], false);
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn an_empty_parameters_object_equals_an_absent_one() {
    let with_empty = Ruleset::new("r").with_rules(vec![Rule::with("creation", json!({}))]);
    let without = Ruleset::new("r").with_rules(vec![Rule::new("creation")]);
    assert_eq!(with_empty.normalized(), without.normalized());
}

#[test]
fn server_only_fields_are_stripped() {
    // `id`, `created_at`, `_links` and friends are not configuration; letting
    // them into the comparison would make every ruleset permanently dirty.
    let mut payload = serde_json::Map::new();
    for key in [
        "id",
        "node_id",
        "created_at",
        "updated_at",
        "_links",
        "source",
    ] {
        payload.insert(key.into(), json!("x"));
    }
    payload.insert("name".into(), json!("keep"));

    strip_server_fields(&mut payload);

    assert_eq!(payload.keys().collect::<Vec<_>>(), vec!["name"]);
}

#[test]
fn identity_is_the_name_not_the_id() {
    // Ids are not portable between repositories, so an exported file must never
    // contain one — and matching must not depend on one.
    let changes = plan(vec![protection()], vec![(999, protection())], false);
    assert!(changes.is_empty());

    let yaml = serde_norway::to_string(&protection()).unwrap();
    assert!(!yaml.contains("id"), "{yaml}");
}

#[test]
fn reports_changed_rules_individually() {
    // "rules changed" is useless in a plan.
    let desired = Ruleset::new("main-protection").with_rules(vec![Rule::with(
        "pull_request",
        json!({"required_approving_review_count": 2}),
    )]);
    let changes = plan(vec![desired], vec![(1, protection())], false);

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Update);

    let fields: Vec<&str> = changes[0].fields.iter().map(|f| f.field.as_str()).collect();
    assert!(fields.contains(&"rule pull_request"), "{fields:?}");
    assert!(fields.contains(&"rule non_fast_forward"), "{fields:?}");
}

#[test]
fn server_defaulted_rule_parameters_do_not_diff() {
    // Verified against the real API: creating a `pull_request` rule with five
    // parameters returns seven — GitHub adds `required_reviewers` and
    // `allowed_merge_methods`. Comparing the objects wholesale reported an
    // update on every single run, which is the permanent diff ADR-002 exists to
    // prevent, and it made rulesets non-idempotent.
    let desired = Ruleset::new("main-protection").with_rules(vec![Rule::with(
        "pull_request",
        json!({
            "required_approving_review_count": 1,
            "dismiss_stale_reviews_on_push": false,
            "require_code_owner_review": false,
            "require_last_push_approval": false,
            "required_review_thread_resolution": false,
        }),
    )]);

    let as_github_returns_it = Ruleset::new("main-protection").with_rules(vec![Rule::with(
        "pull_request",
        json!({
            "required_approving_review_count": 1,
            "dismiss_stale_reviews_on_push": false,
            "require_code_owner_review": false,
            "require_last_push_approval": false,
            "required_review_thread_resolution": false,
            // Defaulted by the server; never written by the user.
            "required_reviewers": [],
            "allowed_merge_methods": ["merge", "squash", "rebase"],
        }),
    )]);

    let changes = plan(vec![desired], vec![(1, as_github_returns_it)], false);
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn a_declared_parameter_that_changed_still_diffs() {
    // The subset comparison must not blind us to a real change.
    let desired = Ruleset::new("r").with_rules(vec![Rule::with(
        "pull_request",
        json!({"required_approving_review_count": 2}),
    )]);
    let current = Ruleset::new("r").with_rules(vec![Rule::with(
        "pull_request",
        json!({"required_approving_review_count": 1, "allowed_merge_methods": ["merge"]}),
    )]);

    let changes = plan(vec![desired], vec![(1, current)], false);
    assert_eq!(changes.len(), 1, "{changes:#?}");
    assert_eq!(changes[0].op, Op::Update);
}

#[test]
fn declaring_a_parameter_the_server_omits_diffs() {
    let desired = Ruleset::new("r").with_rules(vec![Rule::with(
        "pull_request",
        json!({"required_approving_review_count": 1}),
    )]);
    let current = Ruleset::new("r").with_rules(vec![Rule::new("pull_request")]);

    assert_eq!(plan(vec![desired], vec![(1, current)], false).len(), 1);
}

#[test]
fn a_rule_with_no_declared_parameters_never_diffs_on_defaults() {
    // `- type: non_fast_forward` takes no parameters, but a future GitHub
    // release adding one must not make every repository dirty.
    let desired = Ruleset::new("r").with_rules(vec![Rule::new("non_fast_forward")]);
    let current = Ruleset::new("r").with_rules(vec![Rule::with(
        "non_fast_forward",
        json!({"some_future_parameter": true}),
    )]);

    assert!(plan(vec![desired], vec![(1, current)], false).is_empty());
}

#[test]
fn an_enforcement_change_is_reported() {
    let desired = protection().with_enforcement(Enforcement::Disabled);
    let changes = plan(vec![desired], vec![(1, protection())], false);
    assert_eq!(
        changes[0].fields[0],
        FieldDiff::changed("enforcement", "active", "disabled")
    );
}

#[test]
fn does_not_delete_unmanaged_rulesets_by_default() {
    let changes = plan(
        vec![protection()],
        vec![(1, protection()), (2, Ruleset::new("legacy"))],
        false,
    );
    assert!(changes.is_empty(), "prune is off by default: {changes:#?}");
}

#[test]
fn deletes_unmanaged_rulesets_when_pruning() {
    let changes = plan(
        vec![protection()],
        vec![(1, protection()), (2, Ruleset::new("legacy"))],
        true,
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].decode::<Payload>().unwrap().id, Some(2));
}

#[test]
fn an_unknown_rule_type_round_trips_untouched() {
    // GitHub adds rule types faster than any client tracks them. Dropping one on
    // export would silently delete it on the next sync.
    let future = Ruleset::new("r").with_rules(vec![Rule::with(
        "some_future_rule",
        json!({"setting": true}),
    )]);

    let yaml = serde_norway::to_string(&future).unwrap();
    let parsed: Ruleset = serde_norway::from_str(&yaml).unwrap();
    assert_eq!(parsed, future);

    let body = parsed.as_body();
    let rules = body["rules"].as_array().unwrap();
    assert_eq!(rules[0]["type"], "some_future_rule");
    assert_eq!(rules[0]["parameters"]["setting"], true);
}

#[test]
fn bypass_actor_order_does_not_matter() {
    let a = Ruleset {
        bypass_actors: vec![
            BypassActor {
                actor_id: Some(2),
                actor_type: Some("Team".into()),
                ..BypassActor::team("b")
            },
            BypassActor {
                actor_id: Some(1),
                actor_type: Some("Team".into()),
                ..BypassActor::team("a")
            },
        ],
        ..Ruleset::new("r")
    };
    let b = Ruleset {
        bypass_actors: vec![
            BypassActor {
                actor_id: Some(1),
                actor_type: Some("Team".into()),
                ..BypassActor::team("a")
            },
            BypassActor {
                actor_id: Some(2),
                actor_type: Some("Team".into()),
                ..BypassActor::team("b")
            },
        ],
        ..Ruleset::new("r")
    };
    // Asserted through the diff rather than through `normalized`, because that
    // is where order is now made not to matter. `normalized` deliberately
    // preserves declaration order so that validation's positional paths line up
    // with the document the user wrote.
    let changes = plan(vec![a], vec![(1, b)], false);
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn normalisation_preserves_the_order_the_user_wrote() {
    // Sorting here used to misalign every positional span path: validation
    // indexes into this list and looks the position up in the authored
    // document, so a reordered list underlined the wrong rule.
    let ruleset = Ruleset::new("r").with_rules(vec![
        Rule::new("update"),
        Rule::new("creation"),
        Rule::new("non_fast_forward"),
    ]);

    let normalized = ruleset.normalized();
    let types: Vec<&str> = normalized
        .rules
        .iter()
        .map(|rule| rule.rule_type.as_str())
        .collect();
    assert_eq!(types, ["update", "creation", "non_fast_forward"]);
}

#[test]
fn a_resolved_slug_compares_equal_to_the_api_shape() {
    // The configuration says `{ team: eng }`; the API returns
    // `{ actor_id: 42, actor_type: Team }`. After resolution they must match, or
    // every ruleset with a bypass actor would diff forever.
    let configured = BypassActor {
        actor_id: Some(42),
        actor_type: Some("Team".into()),
        ..BypassActor::team("eng")
    };
    let from_api = BypassActor::from_api(&json!({
        "actor_id": 42,
        "actor_type": "Team",
        "bypass_mode": "always"
    }))
    .unwrap();

    assert_eq!(configured.comparable(), from_api.comparable());
}

#[test]
fn normalisation_keeps_the_slug_that_still_needs_resolving() {
    // Dropping it here would leave `prepare` with nothing to look up, and
    // validation with nothing to complain about.
    let actor = BypassActor::team("  eng  ").normalized();
    assert_eq!(actor.team.as_deref(), Some("eng"));
    assert!(actor.needs_resolution());
}

#[test]
fn conditions_are_order_insensitive() {
    let a = Conditions {
        ref_name: Some(RefNameCondition {
            include: vec!["b".into(), "a".into()],
            exclude: vec![],
        }),
    };
    let b = Conditions {
        ref_name: Some(RefNameCondition {
            include: vec!["a".into(), "b".into()],
            exclude: vec![],
        }),
    };
    assert_eq!(a.normalized(), b.normalized());
}

#[test]
fn resolution_is_needed_until_an_actor_has_an_identifier() {
    assert!(BypassActor::team("eng").needs_resolution());
    assert!(BypassActor::app("dependabot").needs_resolution());

    // No lookup, but still an identifier to fill in — and the diff compares on
    // identifiers, so skipping it made this actor differ from itself forever.
    assert!(BypassActor::organization_admin().needs_resolution());

    assert!(
        !BypassActor {
            actor_id: Some(5),
            actor_type: Some("RepositoryRole".into()),
            ..BypassActor::organization_admin()
        }
        .needs_resolution()
    );
}

mod validation {
    use super::*;

    fn codes(rulesets: Vec<Ruleset>) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        let normalised: Vec<Ruleset> = rulesets.iter().map(Ruleset::normalized).collect();
        model::validate(&normalised, &ctx)
            .into_iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn accepts_a_reasonable_ruleset() {
        assert!(codes(vec![protection()]).is_empty());
    }

    #[test]
    fn warns_about_a_ruleset_with_no_rules() {
        let codes = codes(vec![Ruleset::new("empty")]);
        assert!(codes.contains(&"gh_settings::rulesets::no_rules".to_string()));
    }

    #[test]
    fn rejects_duplicate_names() {
        let codes = codes(vec![protection(), protection()]);
        assert!(codes.contains(&"gh_settings::rulesets::duplicate".to_string()));
    }

    #[test]
    fn rejects_a_rule_declared_twice() {
        let ruleset =
            Ruleset::new("r").with_rules(vec![Rule::new("creation"), Rule::new("creation")]);
        assert!(
            codes(vec![ruleset]).contains(&"gh_settings::rulesets::duplicate_rule".to_string())
        );
    }

    #[test]
    fn rejects_a_branch_only_rule_on_a_tag_ruleset() {
        // GitHub answers this with an opaque 422; catching it here is far kinder.
        let ruleset = Ruleset {
            target: RulesetTarget::Tag,
            ..Ruleset::new("tags").with_rules(vec![Rule::new("pull_request")])
        };
        assert!(
            codes(vec![ruleset])
                .contains(&"gh_settings::rulesets::rule_not_valid_for_target".to_string())
        );
    }

    #[test]
    fn an_unknown_rule_is_a_warning_not_an_error() {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        let ruleset = Ruleset::new("r").with_rules(vec![Rule::new("some_future_rule")]);
        let findings = model::validate(&[ruleset.normalized()], &ctx);

        assert!(
            findings.iter().all(|finding| !finding.is_error()),
            "an unknown rule must never block a sync"
        );
    }

    #[test]
    fn rejects_a_bypass_actor_with_no_target() {
        let ruleset = Ruleset {
            bypass_actors: vec![BypassActor {
                team: None,
                app: None,
                organization_admin: false,
                actor_id: None,
                actor_type: None,
                bypass_mode: BypassMode::Always,
            }],
            ..protection()
        };
        assert!(
            codes(vec![ruleset]).contains(&"gh_settings::rulesets::empty_bypass_actor".to_string())
        );
    }

    #[test]
    fn rejects_a_bypass_actor_with_several_targets() {
        let ruleset = Ruleset {
            bypass_actors: vec![BypassActor {
                app: Some("dependabot".into()),
                ..BypassActor::team("eng")
            }],
            ..protection()
        };
        assert!(
            codes(vec![ruleset])
                .contains(&"gh_settings::rulesets::ambiguous_bypass_actor".to_string())
        );
    }
}
