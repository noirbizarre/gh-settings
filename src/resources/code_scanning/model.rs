//! Code scanning model.
//!
//! # Normalisation
//!
//! Two traps, each of which produces a permanent diff if missed:
//!
//! * GitHub reports a configured language under every name it answers to:
//!   a repository set up for `javascript-typescript` reads back as
//!   `javascript`, `javascript-typescript` and `typescript`. [`normalize_language`]
//!   folds each alias into the name the API accepts on write, on both sides.
//! * An unset runner label reads back as `null` before default setup was
//!   first enabled and as `""` after, so [`normalize_label`] treats both as
//!   unset.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Finding;
use crate::resources::ValidateCtx;

/// Which CodeQL queries default setup runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuerySuite {
    /// The default suite, tuned for a low false-positive rate.
    Default,
    /// The default suite plus lower-severity and lower-precision queries.
    Extended,
}

impl QuerySuite {
    /// The value as the API spells it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Extended => "extended",
        }
    }
}

/// Which sources of untrusted data CodeQL considers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreatModel {
    /// Remote sources only, such as network requests.
    Remote,
    /// Remote sources and local ones, such as files and environment variables.
    RemoteAndLocal,
}

impl ThreatModel {
    /// The value as the API spells it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::RemoteAndLocal => "remote_and_local",
        }
    }
}

/// Which runners default setup's analysis runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunnerType {
    /// GitHub-hosted runners.
    Standard,
    /// Runners carrying the label named by `runner_label`.
    Labeled,
}

impl RunnerType {
    /// The value as the API spells it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Labeled => "labeled",
        }
    }
}

/// CodeQL default setup.
///
/// ```yaml
/// default_setup:
///   enabled: true
///   query_suite: extended
///   languages: [actions, python]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DefaultSetup {
    /// Whether default setup is enabled.
    ///
    /// `false` turns it off, and the other fields are then ignored.
    pub enabled: bool,

    /// Which CodeQL queries to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_suite: Option<QuerySuite>,

    /// Which sources of untrusted data CodeQL considers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threat_model: Option<ThreatModel>,

    /// Languages to analyse, for example `actions`, `python` or
    /// `javascript-typescript`.
    ///
    /// Omit it to let GitHub analyse every supported language it detects.
    /// `javascript` and `typescript` both mean `javascript-typescript`; likewise
    /// `c` and `cpp` mean `c-cpp`, and `java` and `kotlin` mean `java-kotlin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,

    /// Which runners the analysis runs on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_type: Option<RunnerType>,

    /// Label of the self-hosted runners to use, with `runner_type: labeled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
}

impl DefaultSetup {
    /// The declared languages, normalised; `None` when unmanaged.
    pub fn normalized_languages(&self) -> Option<BTreeSet<String>> {
        self.languages
            .as_ref()
            .map(|languages| normalize_languages(languages))
    }
}

/// The `code_scanning` configuration section.
///
/// ```yaml
/// code_scanning:
///   default_setup:
///     enabled: true
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeScanningSettings {
    /// CodeQL default setup, which analyses the repository on GitHub's own
    /// workflow without a workflow file.
    ///
    /// GitHub applies a change to the query suite, threat model or languages
    /// of a setup that is already enabled through a *CodeQL Setup* workflow
    /// run, and reports the previous values until that run finishes. A plan
    /// made in the meantime shows the change again.
    ///
    /// GitHub also accepts enabling it on a repository with no language CodeQL
    /// can analyse, then reports no languages and does not start analysing
    /// when one is added. Enable it once the repository has code, or turn it
    /// off and on again after adding some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_setup: Option<DefaultSetup>,
}

/// Normalise a language to the name the API accepts on write.
pub fn normalize_language(language: &str) -> String {
    let language = language.trim().to_lowercase();
    match language.as_str() {
        "javascript" | "typescript" => "javascript-typescript".to_string(),
        "c" | "cpp" => "c-cpp".to_string(),
        "java" | "kotlin" => "java-kotlin".to_string(),
        _ => language,
    }
}

/// Normalise a list of languages into a comparable set.
pub fn normalize_languages<S: AsRef<str>>(languages: &[S]) -> BTreeSet<String> {
    languages
        .iter()
        .map(|language| normalize_language(language.as_ref()))
        .filter(|language| !language.is_empty())
        .collect()
}

/// Normalise a runner label, mapping the unset spellings to `None`.
pub fn normalize_label(label: Option<&str>) -> Option<String> {
    label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
}

/// Validate the code scanning section.
pub fn validate(settings: &CodeScanningSettings, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(setup) = &settings.default_setup else {
        return findings;
    };
    let path = "code_scanning.default_setup";

    if !setup.enabled {
        let ignored: Vec<&str> = [
            ("query_suite", setup.query_suite.is_some()),
            ("threat_model", setup.threat_model.is_some()),
            ("languages", setup.languages.is_some()),
            ("runner_type", setup.runner_type.is_some()),
            ("runner_label", setup.runner_label.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, declared)| declared.then_some(name))
        .collect();

        if !ignored.is_empty() {
            findings.push(
                Finding::warning(
                    "gh_settings::code_scanning::ignored_when_disabled",
                    format!(
                        "`{}` has no effect while default setup is disabled",
                        ignored.join("`, `")
                    ),
                )
                .at(ctx.span(&format!("{path}.enabled")))
                .labelled("disabled here")
                .help("remove the other fields, or set `enabled: true`"),
            );
        }
        return findings;
    }

    if setup
        .languages
        .as_ref()
        .is_some_and(|languages| normalize_languages(languages).is_empty())
    {
        findings.push(
            Finding::error(
                "gh_settings::code_scanning::no_languages",
                "`languages` is empty",
            )
            .at(ctx.key_span(&format!("{path}.languages")))
            .labelled("nothing to analyse")
            .help("name at least one language, or omit `languages` to analyse every one GitHub detects"),
        );
    }

    // GitHub rejects both mismatches with a 422 that names neither field.
    let label = normalize_label(setup.runner_label.as_deref());
    match (setup.runner_type, &label) {
        (Some(RunnerType::Labeled), None) => findings.push(
            Finding::error(
                "gh_settings::code_scanning::missing_runner_label",
                "`runner_type: labeled` needs a `runner_label`",
            )
            .at(ctx.span(&format!("{path}.runner_type")))
            .labelled("no label to select runners by")
            .help("set `runner_label` to the label of your self-hosted runners"),
        ),
        (runner_type, Some(_)) if runner_type != Some(RunnerType::Labeled) => findings.push(
            Finding::error(
                "gh_settings::code_scanning::unused_runner_label",
                "`runner_label` needs `runner_type: labeled`",
            )
            .at(ctx.key_span(&format!("{path}.runner_label")))
            .labelled("only used for labeled runners")
            .help("set `runner_type: labeled`, or remove `runner_label`"),
        ),
        _ => {}
    }

    findings
}
