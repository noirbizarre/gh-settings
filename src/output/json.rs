//! Machine-readable rendering.
//!
//! The JSON shapes here are a public interface: CI pipelines parse them. Fields
//! are added, never renamed or removed, within a major version.

use serde::Serialize;

use crate::config::Finding;
use crate::engine::{ApplyReport, Plan};
use crate::resources::Counts;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
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

impl JsonRenderer {
    /// Render a plan.
    pub fn plan(&self, plan: &Plan) -> String {
        serde_json::to_string_pretty(&plan.to_artifact())
            .unwrap_or_else(|error| panic!("a plan should always serialise: {error}"))
    }

    /// Render validation findings.
    pub fn validation(&self, findings: &[Finding]) -> String {
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

    #[test]
    fn validation_reports_validity_separately_from_warnings() {
        let findings = vec![
            Finding::warning("a::b", "just so you know"),
            Finding::error("c::d", "this is wrong").help("try this"),
        ];
        let value: Value = serde_json::from_str(&JsonRenderer.validation(&findings)).unwrap();

        assert_eq!(value["valid"], false);
        assert_eq!(value["findings"][0]["severity"], "warning");
        assert_eq!(value["findings"][1]["severity"], "error");
        assert_eq!(value["findings"][1]["help"], "try this");
    }

    #[test]
    fn warnings_alone_are_still_valid() {
        let findings = vec![Finding::warning("a::b", "just so you know")];
        let value: Value = serde_json::from_str(&JsonRenderer.validation(&findings)).unwrap();
        assert_eq!(value["valid"], true);
    }

    #[test]
    fn an_empty_validation_is_valid() {
        let value: Value = serde_json::from_str(&JsonRenderer.validation(&[])).unwrap();
        assert_eq!(value["valid"], true);
    }
}
