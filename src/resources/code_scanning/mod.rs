//! Code scanning: CodeQL default setup.
//!
//! A singleton resource, like `pages`, over one endpoint:
//! `GET`/`PATCH /repos/{o}/{r}/code-scanning/default-setup`. The `PATCH` carries
//! `state: configured` or `state: not-configured` alongside whichever settings
//! change, so enabling, updating and disabling are all one request.
//!
//! # Settings a disabled setup reports
//!
//! A repository that has never enabled default setup still answers the `GET`,
//! with the languages GitHub detected and the query suite and threat model it
//! would use. None of those describe anything that runs, so the diff compares
//! them only while the setup is enabled, and enabling sends every declared
//! setting rather than only the ones that differ.
//!
//! # Changes that land later
//!
//! A `PATCH` answers `202 Accepted` and starts a *CodeQL Setup* workflow run.
//! Turning the setup on or off, and removing a language, show up on the next
//! read; a new query suite, threat model or added language reads back only
//! once that run has finished, so a plan made in the meantime reports them
//! again. Verified against a real repository.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::{Finding, Settings};
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

pub mod model;

pub use model::{CodeScanningSettings, DefaultSetup, QuerySuite, RunnerType, ThreatModel};

/// The `code_scanning` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodeScanning;

/// Default setup as the API reports it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DefaultSetupState {
    /// `configured` or `not-configured`.
    #[serde(default)]
    pub state: Option<String>,
    /// Languages analysed, or detected while not configured.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Query suite.
    #[serde(default)]
    pub query_suite: Option<String>,
    /// Threat model.
    #[serde(default)]
    pub threat_model: Option<String>,
    /// Runner type.
    #[serde(default)]
    pub runner_type: Option<String>,
    /// Runner label, `null` or `""` when unset.
    #[serde(default)]
    pub runner_label: Option<String>,
}

impl DefaultSetupState {
    /// A normalised copy, safe to compare against a normalised counterpart.
    pub fn normalized(mut self) -> Self {
        fn text(value: Option<String>) -> Option<String> {
            value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_lowercase)
        }

        self.state = text(self.state);
        self.query_suite = text(self.query_suite);
        self.threat_model = text(self.threat_model);
        self.runner_type = text(self.runner_type);
        self.runner_label = model::normalize_label(self.runner_label.as_deref());
        self.languages = model::normalize_languages(&self.languages)
            .into_iter()
            .collect();
        self
    }

    /// Whether default setup is enabled.
    pub fn is_configured(&self) -> bool {
        self.state.as_deref() == Some("configured")
    }

    /// The analysed languages, as a set.
    pub fn language_set(&self) -> BTreeSet<String> {
        self.languages.iter().cloned().collect()
    }
}

/// Current state: `None` when the repository has no code scanning endpoint.
pub type Current = Option<DefaultSetupState>;

/// Payload of a code scanning change: a `PATCH /code-scanning/default-setup`
/// body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    /// The request body.
    pub body: Value,
}

#[async_trait]
impl Resource for CodeScanning {
    type Desired = CodeScanningSettings;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::CodeScanning
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::ADMINISTRATION
    }

    fn depends_on(&self) -> &'static [ResourceId] {
        // Code scanning on a private repository needs a GitHub Advanced Security
        // licence, so a run that makes a repository public must do so first.
        &[ResourceId::Repository]
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        settings.code_scanning.clone()
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(desired, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let current: Option<DefaultSetupState> = client
            .send_optional(Request::get(target.endpoint("code-scanning/default-setup")))
            .await?;
        Ok(current.map(DefaultSetupState::normalized))
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        // Unused on purpose: there is nothing to prune, only a setting to
        // switch off, and that takes an explicit `enabled: false`.
        _prune: &PruneOpts,
    ) -> Vec<Change> {
        let Some(setup) = &desired.default_setup else {
            return Vec::new();
        };
        let empty = DefaultSetupState::default();
        let current = current.as_ref().unwrap_or(&empty);

        match (setup.enabled, current.is_configured()) {
            (false, false) => Vec::new(),
            (false, true) => vec![
                Change::new(ResourceId::CodeScanning, Op::Update, "default_setup")
                    .summary("disable CodeQL default setup")
                    .fields(vec![FieldDiff::changed("enabled", "true", "false")])
                    .payload(Payload {
                        body: json!({ "state": "not-configured" }),
                    }),
            ],
            (true, false) => vec![self.enable(setup)],
            (true, true) => self.update(setup, current).into_iter().collect(),
        }
    }

    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()> {
        let payload: Payload = change.decode().unwrap_or_else(|error| {
            panic!("code scanning change carried an undecodable payload: {error}")
        });

        client
            .execute(Request::patch(
                target.endpoint("code-scanning/default-setup"),
                payload.body,
            ))
            .await
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        // A disabled setup exports nothing, as Pages does for a repository
        // without a site: the settings GitHub reports for it describe nothing
        // that runs, and an exported file should not switch it on or off.
        let Some(current) = self.current(client, target).await? else {
            return Ok(None);
        };
        if !current.is_configured() {
            return Ok(None);
        }

        let settings = CodeScanningSettings {
            default_setup: Some(DefaultSetup {
                enabled: true,
                query_suite: current.query_suite.as_deref().and_then(parse_enum),
                threat_model: current.threat_model.as_deref().and_then(parse_enum),
                languages: (!current.languages.is_empty()).then(|| current.languages.clone()),
                runner_type: current.runner_type.as_deref().and_then(parse_enum),
                runner_label: current.runner_label.clone(),
            }),
        };

        Ok(Some(serde_json::to_value(settings).unwrap_or(Value::Null)))
    }
}

impl CodeScanning {
    /// The change that enables a setup that is off.
    fn enable(&self, setup: &DefaultSetup) -> Change {
        let mut body = Map::new();
        body.insert("state".into(), json!("configured"));
        let mut fields = vec![FieldDiff::changed("enabled", "false", "true")];

        for (name, value) in declared(setup) {
            fields.push(FieldDiff::added(name, value.label()));
            body.insert(name.into(), value.body());
        }

        Change::new(ResourceId::CodeScanning, Op::Update, "default_setup")
            .summary("enable CodeQL default setup")
            .fields(fields)
            .payload(Payload {
                body: Value::Object(body),
            })
    }

    /// The change, if any, that brings an enabled setup in line.
    fn update(&self, setup: &DefaultSetup, current: &DefaultSetupState) -> Option<Change> {
        let mut body = Map::new();
        let mut fields = Vec::new();

        for (name, value) in declared(setup) {
            let before = match name {
                "query_suite" => Setting::Text(current.query_suite.clone()),
                "threat_model" => Setting::Text(current.threat_model.clone()),
                "runner_type" => Setting::Text(current.runner_type.clone()),
                "runner_label" => Setting::Text(current.runner_label.clone()),
                "languages" => Setting::Languages(current.language_set()),
                _ => unreachable!("`declared` yields only the fields above"),
            };
            if before != value {
                fields.push(FieldDiff::changed(name, before.label(), value.label()));
                body.insert(name.into(), value.body());
            }
        }

        if body.is_empty() {
            return None;
        }
        // Sent with every update, so the request does not depend on how GitHub
        // reads a body that leaves the state out.
        body.insert("state".into(), json!("configured"));

        let summary = if fields.len() == 1 {
            format!("update CodeQL default setup {}", fields[0].field)
        } else {
            format!("update CodeQL default setup ({} fields)", fields.len())
        };

        Some(
            Change::new(ResourceId::CodeScanning, Op::Update, "default_setup")
                .summary(summary)
                .fields(fields)
                .payload(Payload {
                    body: Value::Object(body),
                }),
        )
    }
}

/// One comparable setting.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Setting {
    /// A string-valued setting, `None` when unset.
    Text(Option<String>),
    /// The analysed languages.
    Languages(BTreeSet<String>),
}

impl Setting {
    /// Rendered for a [`FieldDiff`].
    fn label(&self) -> String {
        match self {
            Self::Text(Some(value)) => value.clone(),
            Self::Text(None) => "(none)".into(),
            Self::Languages(languages) if languages.is_empty() => "(none)".into(),
            Self::Languages(languages) => languages
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// The request body fragment.
    fn body(&self) -> Value {
        match self {
            Self::Text(value) => json!(value),
            Self::Languages(languages) => json!(languages),
        }
    }
}

/// Every setting the configuration declares, normalised, in a stable order.
fn declared(setup: &DefaultSetup) -> Vec<(&'static str, Setting)> {
    let text = |value: Option<&str>| value.map(|value| Setting::Text(Some(value.to_string())));
    [
        ("query_suite", text(setup.query_suite.map(|v| v.as_str()))),
        ("threat_model", text(setup.threat_model.map(|v| v.as_str()))),
        (
            "languages",
            setup.normalized_languages().map(Setting::Languages),
        ),
        ("runner_type", text(setup.runner_type.map(|v| v.as_str()))),
        (
            "runner_label",
            setup
                .runner_label
                .as_deref()
                .map(|label| Setting::Text(model::normalize_label(Some(label)))),
        ),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|value| (name, value)))
    .collect()
}

/// Parse the API's spelling of an enum back into a configuration enum.
fn parse_enum<T: for<'de> Deserialize<'de>>(value: &str) -> Option<T> {
    serde_json::from_value(Value::String(value.to_string())).ok()
}

#[cfg(test)]
mod tests;
