use super::*;
use crate::config::SpanIndex;
use rstest::rstest;

fn settings(yaml: &str) -> PagesSettings {
    serde_norway::from_str(yaml).expect("valid pages section")
}

fn state(json: Value) -> PagesState {
    serde_json::from_value::<PagesState>(json)
        .expect("valid pages state")
        .normalized()
}

/// A site built from `gh-pages`, with nothing else set.
fn a_legacy_site() -> PagesState {
    state(json!({
        "build_type": "legacy",
        "source": { "branch": "gh-pages", "path": "/" },
        "cname": null,
        "https_enforced": true,
    }))
}

fn plan(desired: &str, current: Current) -> Vec<Change> {
    Pages.diff(&settings(desired), &current, &PruneOpts::default())
}

fn payload(change: &Change) -> Payload {
    change.decode().expect("a decodable payload")
}

mod normalisation {
    use super::*;
    use pretty_assertions::assert_eq;

    #[rstest]
    #[case(None, "/")]
    #[case(Some(""), "/")]
    #[case(Some("/"), "/")]
    #[case(Some("docs"), "/docs")]
    #[case(Some("/docs"), "/docs")]
    #[case(Some("docs/"), "/docs")]
    #[case(Some(" /docs/ "), "/docs")]
    fn every_spelling_of_a_source_path_collapses_to_one(
        #[case] input: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(model::normalize_path(input), expected);
    }

    #[rstest]
    #[case(None, None)]
    #[case(Some(""), None)]
    #[case(Some("  "), None)]
    #[case(Some("Docs.Example.COM"), Some("docs.example.com"))]
    #[case(Some(" docs.example.com "), Some("docs.example.com"))]
    fn a_custom_domain_is_compared_as_dns_sees_it(
        #[case] input: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(model::normalize_cname(input), expected.map(str::to_string));
    }

    #[test]
    fn a_domain_differing_only_in_case_is_not_a_change() {
        // The classic permanent diff: the user writes `Docs.Example.com`, GitHub
        // stores it lowercased.
        let current = state(json!({ "build_type": "legacy", "cname": "docs.example.com" }));
        assert!(plan("cname: Docs.Example.com", Some(current)).is_empty());
    }

    #[test]
    fn a_source_path_differing_only_in_slashes_is_not_a_change() {
        let current = state(json!({
            "build_type": "legacy",
            "source": { "branch": "gh-pages", "path": "/docs" },
        }));
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\n  path: docs\n";
        assert!(plan(yaml, Some(current)).is_empty());
    }

    #[test]
    fn an_empty_domain_reported_by_github_reads_as_unset() {
        // The API has answered both `null` and `""` for a site with no custom
        // domain; treating them differently would make one of them diff forever.
        assert_eq!(state(json!({ "cname": "" })).cname, None);
    }
}

mod creating {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn declaring_the_section_enables_pages() {
        let changes = plan("build_type: workflow", None);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].op, Op::Create);
        assert_eq!(changes[0].summary, "enable GitHub Pages");
    }

    #[test]
    fn the_create_body_carries_only_what_post_accepts() {
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\ncname: docs.example.com\n";
        let changes = plan(yaml, None);

        let Payload::Create { create, update } = payload(&changes[0]) else {
            panic!("expected a create payload");
        };

        assert_eq!(
            create,
            json!({ "build_type": "legacy", "source": { "branch": "gh-pages", "path": "/" } })
        );
        // `cname` is not accepted by POST, so it has to follow in a PUT.
        assert_eq!(update, Some(json!({ "cname": "docs.example.com" })));
    }

    #[test]
    fn nothing_follows_the_create_when_there_is_nothing_post_could_not_say() {
        let changes = plan("build_type: workflow", None);
        let Payload::Create { update, .. } = payload(&changes[0]) else {
            panic!("expected a create payload");
        };
        assert_eq!(update, None);
    }

    #[test]
    fn a_section_that_cannot_create_a_site_produces_no_change() {
        // Only a `cname`: GitHub has nothing to build a site from, so emitting a
        // create we know it would reject helps nobody. `validate` warns instead.
        assert!(plan("cname: docs.example.com", None).is_empty());
    }
}

mod updating {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_unchanged_site_produces_no_change() {
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\nhttps_enforced: true\n";
        assert!(plan(yaml, Some(a_legacy_site())).is_empty());
    }

    #[test]
    fn an_omitted_field_is_never_sent() {
        // The property the whole design rests on: a file setting only `cname`
        // must not reset the build type or the visibility.
        let changes = plan("cname: docs.example.com", Some(a_legacy_site()));
        let Payload::Update(body) = payload(&changes[0]) else {
            panic!("expected an update payload");
        };
        assert_eq!(body, json!({ "cname": "docs.example.com" }));
    }

    #[test]
    fn an_explicit_null_clears_the_custom_domain() {
        let current = state(json!({ "build_type": "legacy", "cname": "docs.example.com" }));
        let changes = plan("cname: null", Some(current));

        let Payload::Update(body) = payload(&changes[0]) else {
            panic!("expected an update payload");
        };
        assert_eq!(body, json!({ "cname": null }));
        assert_eq!(
            changes[0].fields,
            vec![FieldDiff::changed("cname", "docs.example.com", "(none)")]
        );
    }

    #[test]
    fn a_moved_source_travels_alone() {
        // `PUT` documents `build_type` and `source` as independently optional.
        // Sending the current build type alongside a moved source looked like
        // prudence and was the opposite: on a workflow-built site it produced
        // exactly the pairing GitHub refuses.
        let yaml = "source:\n  branch: main\n  path: /docs\n";
        let changes = plan(yaml, Some(a_legacy_site()));

        let Payload::Update(body) = payload(&changes[0]) else {
            panic!("expected an update payload");
        };
        assert_eq!(
            body,
            json!({ "source": { "branch": "main", "path": "/docs" } })
        );
    }

    #[test]
    fn a_source_always_carries_a_path() {
        // Optional on `POST`, required on `PUT`. Always sending it satisfies
        // both and costs nothing, since the value is normalised anyway.
        let yaml = "source:\n  branch: main\n";
        let changes = plan(yaml, Some(a_legacy_site()));

        let Payload::Update(body) = payload(&changes[0]) else {
            panic!("expected an update payload");
        };
        assert_eq!(body["source"]["path"], json!("/"));
    }

    #[test]
    fn a_declared_build_type_is_sent_when_it_differs() {
        let yaml = "build_type: legacy\nsource:\n  branch: main\n";
        let current = state(json!({ "build_type": "workflow" }));
        let changes = plan(yaml, Some(current));

        let Payload::Update(body) = payload(&changes[0]) else {
            panic!("expected an update payload");
        };
        assert_eq!(body["build_type"], json!("legacy"));
    }

    #[test]
    fn several_fields_are_one_change() {
        let yaml = "build_type: workflow\ncname: new.example.com\nhttps_enforced: false\n";
        let changes = plan(yaml, Some(a_legacy_site()));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].op, Op::Update);
        assert_eq!(changes[0].summary, "update pages (3 fields)");
    }

    #[test]
    fn a_single_field_is_named_in_the_summary() {
        let changes = plan("https_enforced: false", Some(a_legacy_site()));
        assert_eq!(changes[0].summary, "update pages https_enforced");
    }
}

mod pruning {
    use super::*;

    #[test]
    fn pruning_never_disables_a_site() {
        // There is no way to declare "off", so there is nothing for `--prune` to
        // act on. A destroyed published site is not something a missing key
        // should cause.
        let prune = PruneOpts { force: Some(true) };
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\nhttps_enforced: true\n";
        assert!(
            Pages
                .diff(&settings(yaml), &Some(a_legacy_site()), &prune)
                .is_empty()
        );
    }
}

mod validation {
    use super::*;
    use pretty_assertions::assert_eq;

    fn codes(yaml: &str) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        Pages
            .validate(&settings(yaml), &ctx)
            .into_iter()
            .map(|finding| finding.code)
            .collect()
    }

    #[test]
    fn a_section_that_cannot_create_a_site_is_flagged() {
        assert_eq!(
            codes("cname: docs.example.com"),
            vec!["gh_settings::pages::no_source"]
        );
    }

    #[test]
    fn a_source_alongside_a_workflow_build_is_rejected() {
        let yaml = "build_type: workflow\nsource:\n  branch: gh-pages\n";
        assert!(codes(yaml).contains(&"gh_settings::pages::source_with_workflow".to_string()));
    }

    #[test]
    fn an_unsupported_source_directory_is_rejected() {
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\n  path: /site\n";
        assert!(codes(yaml).contains(&"gh_settings::pages::invalid_path".to_string()));
    }

    #[test]
    fn an_empty_branch_is_rejected() {
        let yaml = "build_type: legacy\nsource:\n  branch: \"\"\n";
        assert!(codes(yaml).contains(&"gh_settings::pages::empty_branch".to_string()));
    }

    #[test]
    fn a_url_is_not_a_custom_domain() {
        let yaml = "build_type: workflow\ncname: https://docs.example.com\n";
        assert!(codes(yaml).contains(&"gh_settings::pages::invalid_cname".to_string()));
    }

    #[test]
    fn a_complete_section_is_silent() {
        let yaml = "build_type: legacy\nsource:\n  branch: gh-pages\n  path: /docs\ncname: docs.example.com\n";
        assert!(codes(yaml).is_empty());
    }
}

mod desired_projection {
    use super::*;

    #[test]
    fn is_none_when_no_section_is_declared() {
        assert!(Pages.desired(&Settings::default()).is_none());
    }

    #[test]
    fn is_some_when_the_section_is_declared() {
        let settings = Settings {
            pages: Some(settings("build_type: workflow")),
            ..Settings::default()
        };
        assert!(Pages.desired(&settings).is_some());
    }
}

mod what_is_not_managed {
    use super::*;

    #[test]
    fn the_public_flag_is_not_accepted() {
        // `GET /pages` reports it, but neither `POST` nor `PUT` takes it as a
        // body parameter. Accepting it in the file would publish a setting that
        // is silently ignored, which is worse than not offering it at all.
        let error = serde_norway::from_str::<PagesSettings>("build_type: workflow\npublic: true\n")
            .expect_err("`public` must not be accepted");
        assert!(error.to_string().contains("public"), "{error}");
    }
}
