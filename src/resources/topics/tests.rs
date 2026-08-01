//! Topics resource tests.

use super::*;
use crate::config::SpanIndex;
use rstest::rstest;

fn desired(topics: &[&str], prune: bool) -> Desired {
    Desired {
        topics: topics.iter().map(|topic| normalize(topic)).collect(),
        prune,
    }
}

fn current(topics: &[&str]) -> Current {
    Current {
        topics: topics.iter().map(|topic| normalize(topic)).collect(),
    }
}

fn plan(desired_topics: &[&str], current_topics: &[&str], prune: bool) -> Vec<Change> {
    Topics.diff(
        &desired(desired_topics, prune),
        &current(current_topics),
        &PruneOpts::default(),
    )
}

#[rstest]
#[case("Rust", "rust")]
#[case("GitHub CLI", "github-cli")]
#[case("machine_learning", "machine-learning")]
#[case("  Rust  ", "rust")]
fn normalises_the_way_github_does(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(normalize(input), expected);
}

#[test]
fn a_topic_differing_only_in_case_is_not_a_change() {
    // The classic permanent diff: the user writes `Rust`, GitHub stores `rust`.
    assert!(plan(&["Rust"], &["rust"], false).is_empty());
}

#[test]
fn underscores_and_spaces_are_equivalent_to_hyphens() {
    assert!(plan(&["machine_learning"], &["machine-learning"], false).is_empty());
}

#[test]
fn adds_missing_topics() {
    let changes = plan(&["rust", "cli"], &["rust"], false);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Create);
    assert_eq!(changes[0].summary, "add topic cli");
}

#[test]
fn does_not_remove_unmanaged_topics_by_default() {
    let changes = plan(&["rust"], &["rust", "archived"], false);
    assert!(changes.is_empty(), "prune is off by default: {changes:#?}");
}

#[test]
fn removes_unmanaged_topics_when_pruning() {
    let changes = plan(&["rust"], &["rust", "archived"], true);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].op, Op::Delete);
    assert_eq!(changes[0].summary, "remove topic archived");
}

#[test]
fn without_pruning_the_payload_is_the_union() {
    // The endpoint replaces the whole list, so a non-pruning run must resend the
    // topics it does not manage, or it would delete them by omission.
    let changes = plan(&["rust"], &["archived"], false);
    let payload: Payload = changes[0].decode().unwrap();
    assert_eq!(payload.names, vec!["archived", "rust"]);
}

#[test]
fn with_pruning_the_payload_is_exactly_the_desired_set() {
    let changes = plan(&["rust"], &["archived"], true);
    let payload: Payload = changes[0].decode().unwrap();
    assert_eq!(payload.names, vec!["rust"]);
}

#[test]
fn every_change_carries_the_same_convergent_payload() {
    // Applying any single change reaches the final state, so a partial apply
    // still converges rather than leaving a half-written list.
    let changes = plan(&["rust", "cli"], &["archived"], true);
    let payloads: Vec<Payload> = changes.iter().map(|c| c.decode().unwrap()).collect();
    assert!(payloads.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(payloads[0].names, vec!["cli", "rust"]);
}

#[test]
fn plan_order_is_deterministic() {
    let first = plan(&["zebra", "alpha", "middle"], &[], false);
    let second = plan(&["middle", "zebra", "alpha"], &[], false);
    let keys =
        |changes: &[Change]| -> Vec<String> { changes.iter().map(|c| c.key.clone()).collect() };
    assert_eq!(keys(&first), keys(&second));
    assert_eq!(keys(&first), vec!["alpha", "middle", "zebra"]);
}

#[test]
fn an_identical_set_produces_no_change() {
    assert!(plan(&["rust", "cli"], &["cli", "rust"], true).is_empty());
}

#[rstest]
#[case("rust", true)]
#[case("github-cli", true)]
#[case("web3", true)]
#[case("3d", true)]
#[case("-leading", false)]
#[case("has space", false)]
#[case("UPPER", false)]
#[case("", false)]
fn validates_topic_syntax(#[case] topic: &str, #[case] expected: bool) {
    assert_eq!(is_valid(topic), expected, "{topic:?}");
}

#[test]
fn rejects_an_overlong_topic() {
    assert!(!is_valid(&"a".repeat(51)));
    assert!(is_valid(&"a".repeat(50)));
}

mod validation {
    use super::*;

    fn codes(topics: &[&str]) -> Vec<String> {
        let spans = SpanIndex::default();
        let ctx = ValidateCtx::new(&spans);
        Topics
            .validate(&desired(topics, false), &ctx)
            .into_iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn accepts_a_valid_set() {
        assert!(codes(&["rust", "github-cli"]).is_empty());
    }

    #[test]
    fn rejects_more_than_twenty() {
        let topics: Vec<String> = (0..21).map(|index| format!("topic{index}")).collect();
        let refs: Vec<&str> = topics.iter().map(String::as_str).collect();
        assert!(codes(&refs).contains(&"gh_settings::topics::too_many".to_string()));
    }

    #[test]
    fn accepts_exactly_twenty() {
        let topics: Vec<String> = (0..20).map(|index| format!("topic{index}")).collect();
        let refs: Vec<&str> = topics.iter().map(String::as_str).collect();
        assert!(codes(&refs).is_empty());
    }

    #[test]
    fn rejects_a_topic_that_cannot_be_normalised_into_validity() {
        assert!(codes(&["not/valid"]).contains(&"gh_settings::topics::invalid".to_string()));
    }
}

mod desired_projection {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn is_none_when_no_section_is_declared() {
        assert!(Topics.desired(&Settings::default()).is_none());
    }

    #[test]
    fn reads_the_top_level_section() {
        let settings: Settings = serde_yaml_ng::from_str("topics: [rust]").unwrap();
        assert_eq!(
            Topics.desired(&settings).unwrap().topics,
            ["rust".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn falls_back_to_the_safe_settings_spelling() {
        // One-way compatibility: we read their layout, we do not adopt it.
        let settings: Settings = serde_yaml_ng::from_str("repository:\n  topics: [rust]").unwrap();
        assert_eq!(
            Topics.desired(&settings).unwrap().topics,
            ["rust".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn the_top_level_section_wins_when_both_are_present() {
        // The combination is rejected by `Settings::validate`; this only pins the
        // behaviour so it is deterministic rather than arbitrary.
        let settings: Settings =
            serde_yaml_ng::from_str("topics: [top]\nrepository:\n  topics: [nested]").unwrap();
        assert_eq!(
            Topics.desired(&settings).unwrap().topics,
            ["top".to_string()].into_iter().collect()
        );
    }
}
