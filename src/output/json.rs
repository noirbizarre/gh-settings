//! Machine-readable rendering.
//!
//! The JSON shapes here are a public interface: CI pipelines parse them. Fields
//! are added, never renamed or removed, within a major version.

use serde::Serialize;

use crate::config::{Finding, Sources};
use crate::engine::{ApplyReport, Plan};
use crate::github::AuthStatus;
use crate::github::auth::Scopes;
use crate::resources::Capability;
use crate::resources::{Counts, ResourceId};

/// Renders plans and reports as JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonRenderer;

/// JSON form of a validation run.
#[derive(Debug, Serialize)]
pub struct ValidationOutput<'a> {
    /// Whether validation passed.
    pub valid: bool,
    /// Every finding, errors and warnings alike.
    pub findings: Vec<FindingOutput<'a>>,
}

/// JSON form of a single finding.
#[derive(Debug, Serialize)]
pub struct FindingOutput<'a> {
    /// `error` or `warning`.
    pub severity: &'static str,
    /// Stable machine-readable code.
    pub code: &'a str,
    /// Human-readable message.
    pub message: &'a str,
    /// Byte offset of the offending node, when known.
    ///
    /// Only meaningful together with `file`: once a configuration can inherit
    /// from another document, an offset alone does not say what it indexes into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    /// Document the offset indexes into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<&'a str>,
    /// Actionable hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<&'a str>,
}

/// JSON form of an apply run.
#[derive(Debug, Serialize)]
pub struct ApplyOutput {
    /// Whether every change succeeded.
    pub success: bool,
    /// Tally of the changes actually applied.
    pub applied: Counts,
    /// Number of changes skipped after a failure.
    pub skipped: usize,
    /// Failures, in plan order.
    pub failures: Vec<FailureOutput>,
}

/// JSON form of a single failure.
#[derive(Debug, Serialize)]
pub struct FailureOutput {
    /// Resource the change belonged to.
    pub resource: String,
    /// Identity of the affected item.
    pub key: String,
    /// Failure message.
    pub error: String,
    /// HTTP status, when the failure came from the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

/// JSON form of a `doctor` run.
#[derive(Debug, Serialize)]
pub struct DoctorOutput<'a> {
    /// Whether every checked resource is manageable with this credential.
    pub ok: bool,
    /// The `gh` version string, when `gh` was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gh_version: Option<&'a str>,
    /// Authentication, when it could be determined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthOutput<'a>>,
    /// Per-resource capability.
    pub resources: Vec<ResourceCapabilityOutput>,
    /// Whether this credential can read a configuration named by `extends`.
    ///
    /// Reported unconditionally, unlike the human rendering which stays quiet
    /// when inheritance works: a pipeline cannot ask a follow-up question, so
    /// the field has to be there to be keyed on.
    pub inheritance: CapabilityOutput,
}

/// JSON form of the credential in use.
#[derive(Debug, Serialize)]
pub struct AuthOutput<'a> {
    /// Host authenticated against.
    pub hostname: &'a str,
    /// Login, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<&'a str>,
    /// Credential kind, e.g. `classic_pat`.
    pub token_kind: &'static str,
    /// Human label for the credential kind.
    pub token_label: &'static str,
    /// Granted scopes.
    ///
    /// `null` — not an empty list — when the credential does not report them,
    /// which is the case for fine-grained and App tokens. The distinction
    /// matters: "no scopes" and "we cannot tell" are different answers, and
    /// conflating them is what ADR-015 exists to prevent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<&'a [String]>,
    /// Whether the token holds admin rights on the target, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_on_target: Option<bool>,
}

/// JSON form of one resource's capability.
#[derive(Debug, Serialize)]
pub struct ResourceCapabilityOutput {
    /// Resource identifier.
    pub resource: String,
    /// `manageable`, `impossible` or `unknown`.
    pub status: &'static str,
    /// Why, when it is impossible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// JSON form of a capability that belongs to no resource.
#[derive(Debug, Serialize)]
pub struct CapabilityOutput {
    /// `manageable`, `impossible` or `unknown`.
    pub status: &'static str,
    /// Why, when it is impossible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<&Capability> for CapabilityOutput {
    fn from(capability: &Capability) -> Self {
        Self {
            status: capability_status(capability),
            reason: capability.reason().map(str::to_string),
        }
    }
}

/// The wire name for a verdict.
fn capability_status(capability: &Capability) -> &'static str {
    match capability {
        Capability::Manageable => "manageable",
        Capability::Impossible(_) => "impossible",
        Capability::Unknown => "unknown",
    }
}

impl JsonRenderer {
    /// Render a `doctor` run.
    pub fn doctor(
        &self,
        gh_version: Option<&str>,
        auth: Option<&AuthStatus>,
        capabilities: &[(ResourceId, Capability)],
        inheritance: &Capability,
    ) -> String {
        let resources: Vec<ResourceCapabilityOutput> = capabilities
            .iter()
            .map(|(id, capability)| ResourceCapabilityOutput {
                resource: id.as_str().to_string(),
                status: capability_status(capability),
                reason: capability.reason().map(str::to_string),
            })
            .collect();

        let output = DoctorOutput {
            // Unknown is not a failure: a fine-grained token cannot report its
            // scopes, and refusing to proceed on that basis would be guessing.
            ok: gh_version.is_some()
                && auth.is_some()
                && !inheritance.is_certainly_impossible()
                && !capabilities
                    .iter()
                    .any(|(_, capability)| capability.is_certainly_impossible()),
            gh_version,
            authentication: auth.map(|auth| AuthOutput {
                hostname: &auth.hostname,
                account: auth.account.as_deref(),
                token_kind: token_kind_key(auth),
                token_label: auth.token_kind.label(),
                scopes: match &auth.scopes {
                    Scopes::Known(scopes) => Some(scopes.as_slice()),
                    Scopes::Unknown => None,
                },
                admin_on_target: auth.admin_on_target,
            }),
            resources,
            inheritance: inheritance.into(),
        };

        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|error| panic!("a doctor report should always serialise: {error}"))
    }

    /// Render a plan.
    pub fn plan(&self, plan: &Plan) -> String {
        serde_json::to_string_pretty(&plan.to_artifact())
            .unwrap_or_else(|error| panic!("a plan should always serialise: {error}"))
    }

    /// Render validation findings.
    ///
    /// Takes the documents as well as the findings so each offset can name the
    /// text it belongs to.
    pub fn validation<'a>(&self, sources: &'a Sources, findings: &'a [Finding]) -> String {
        let output = ValidationOutput {
            valid: !findings.iter().any(Finding::is_error),
            findings: findings
                .iter()
                .map(|finding| FindingOutput {
                    severity: if finding.is_error() {
                        "error"
                    } else {
                        "warning"
                    },
                    code: &finding.code,
                    message: &finding.message,
                    offset: finding.span.map(|span| span.offset()),
                    file: finding
                        .span
                        .map(|span| sources.get(span.source).name.as_str()),
                    help: finding.help.as_deref(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|error| panic!("findings should always serialise: {error}"))
    }

    /// Render an apply report.
    pub fn apply(&self, report: &ApplyReport) -> String {
        let output = ApplyOutput {
            success: report.is_success(),
            applied: report.applied_counts(),
            skipped: report.skipped(),
            failures: report
                .failures()
                .map(|(change, error)| FailureOutput {
                    resource: change.resource.as_str().to_string(),
                    key: change.key.clone(),
                    error: error.to_string(),
                    status: error.status(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|error| panic!("a report should always serialise: {error}"))
    }

    /// Render a pre-flight refusal.
    ///
    /// Deliberately the same shape as [`Self::apply`]: to a consumer, "these
    /// changes did not happen because the token cannot make them" is the same
    /// answer whether we established it before trying or after. `status` is
    /// absent because no request was made — which is the whole point.
    pub fn refused(&self, blocked: &[(ResourceId, &'static str)]) -> String {
        let output = ApplyOutput {
            success: false,
            applied: Counts::default(),
            skipped: 0,
            failures: blocked
                .iter()
                .map(|(id, reason)| FailureOutput {
                    resource: id.as_str().to_string(),
                    key: String::new(),
                    error: (*reason).to_string(),
                    status: None,
                })
                .collect(),
        };
        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|error| panic!("a refusal should always serialise: {error}"))
    }
}

/// Stable machine-readable key for a credential kind.
///
/// Separate from the human label, which is prose and may be reworded.
fn token_kind_key(auth: &AuthStatus) -> &'static str {
    use crate::github::TokenKind;
    match auth.token_kind {
        TokenKind::OAuth => "oauth",
        TokenKind::ClassicPat => "classic_pat",
        TokenKind::FineGrainedPat => "fine_grained_pat",
        TokenKind::AppInstallation => "app_installation",
        TokenKind::ActionsGitHubToken => "actions_github_token",
        TokenKind::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Target;
    use crate::resources::{Change, Op, ResourceId, ResourcePlan};
    use serde_json::Value;

    fn plan_with(changes: Vec<Change>) -> Plan {
        let mut plan = Plan::new(Target::new("o", "r"));
        plan.push(ResourcePlan {
            id: ResourceId::Labels,
            changes,
        });
        plan
    }

    #[test]
    fn a_plan_serialises_to_the_documented_shape() {
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Create, "bug").summary("create label bug"),
        ]);
        let value: Value = serde_json::from_str(&JsonRenderer.plan(&plan)).unwrap();

        assert_eq!(value["version"], 1);
        assert_eq!(value["repository"], "o/r");
        assert_eq!(value["counts"]["create"], 1);
        assert_eq!(value["changes"][0]["resource"], "labels");
        assert_eq!(value["changes"][0]["op"], "create");
        assert_eq!(value["changes"][0]["key"], "bug");
    }

    #[test]
    fn an_empty_plan_still_serialises() {
        let value: Value =
            serde_json::from_str(&JsonRenderer.plan(&Plan::new(Target::new("o", "r")))).unwrap();
        assert_eq!(value["counts"]["create"], 0);
        assert_eq!(value["changes"].as_array().unwrap().len(), 0);
    }

    /// A single-document registry, for findings that carry no span.
    fn sources() -> Sources {
        Sources::root("settings.yml", "version: 1\n").0
    }

    #[test]
    fn validation_reports_validity_separately_from_warnings() {
        let findings = vec![
            Finding::warning("a::b", "just so you know"),
            Finding::error("c::d", "this is wrong").help("try this"),
        ];
        let value: Value =
            serde_json::from_str(&JsonRenderer.validation(&sources(), &findings)).unwrap();

        assert_eq!(value["valid"], false);
        assert_eq!(value["findings"][0]["severity"], "warning");
        assert_eq!(value["findings"][1]["severity"], "error");
        assert_eq!(value["findings"][1]["help"], "try this");
    }

    #[test]
    fn a_finding_names_the_document_its_offset_belongs_to() {
        use crate::config::FileSpan;
        use miette::SourceSpan;

        let (mut registry, root) = Sources::root("local.yml", "version: 1\n");
        let base = registry.push("acme/.github@v1", "labels: []\n");

        let findings = vec![
            Finding::error("a::b", "local problem")
                .at(FileSpan::new(root, SourceSpan::new(0.into(), 7usize))),
            Finding::error("c::d", "inherited problem")
                .at(FileSpan::new(base, SourceSpan::new(0.into(), 6usize))),
        ];

        let value: Value =
            serde_json::from_str(&JsonRenderer.validation(&registry, &findings)).unwrap();

        // Both offsets are small numbers that index into either document. Only
        // `file` tells a consumer which one to open.
        assert_eq!(value["findings"][0]["file"], "local.yml");
        assert_eq!(value["findings"][1]["file"], "acme/.github@v1");
    }

    #[test]
    fn a_finding_without_a_span_names_no_document() {
        let findings = vec![Finding::warning("a::b", "no location")];
        let value: Value =
            serde_json::from_str(&JsonRenderer.validation(&sources(), &findings)).unwrap();
        assert!(value["findings"][0].get("file").is_none());
        assert!(value["findings"][0].get("offset").is_none());
    }

    #[test]
    fn warnings_alone_are_still_valid() {
        let findings = vec![Finding::warning("a::b", "just so you know")];
        let value: Value =
            serde_json::from_str(&JsonRenderer.validation(&sources(), &findings)).unwrap();
        assert_eq!(value["valid"], true);
    }

    #[test]
    fn an_empty_validation_is_valid() {
        let value: Value = serde_json::from_str(&JsonRenderer.validation(&sources(), &[])).unwrap();
        assert_eq!(value["valid"], true);
    }
}
