//! User-facing diagnostics.
//!
//! Validation output is a product surface, not an afterthought. Every diagnostic
//! carries a span into the configuration file so `miette` can underline the exact
//! offending node, and — wherever we can compute one — a concrete suggestion.

use miette::{Diagnostic, LabeledSpan};

use super::source::FileSpan;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Advisory; does not prevent `sync`.
    Warning,
    /// Blocks `plan` and `sync`.
    Error,
}

/// A single validation finding.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Severity.
    pub severity: Severity,
    /// Stable machine-readable code, e.g. `gh_settings::labels::duplicate`.
    pub code: String,
    /// One-line description of what is wrong.
    pub message: String,
    /// Where in the configuration the problem is, and in which document.
    pub span: Option<FileSpan>,
    /// Short text rendered under the underline.
    pub label: Option<String>,
    /// Actionable next step.
    pub help: Option<String>,
}

impl Finding {
    /// Start building an error-level finding.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            span: None,
            label: None,
            help: None,
        }
    }

    /// Start building a warning-level finding.
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, message)
        }
    }

    /// Attach a source span.
    pub fn at(mut self, span: impl Into<Option<FileSpan>>) -> Self {
        self.span = span.into();
        self
    }

    /// Attach the text rendered under the underline.
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach an actionable hint.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Attach a "did you mean" hint computed from the candidates.
    ///
    /// Silently no-ops when nothing is close enough, because a bad suggestion is
    /// worse than none.
    pub fn suggest(self, unknown: &str, candidates: &[&str]) -> Self {
        match suggest(unknown, candidates) {
            Some(best) => self.help(format!("did you mean `{best}`?")),
            None => self,
        }
    }

    /// Whether this finding blocks execution.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Find the closest candidate to `unknown`, if one is close enough to be useful.
pub fn suggest(unknown: &str, candidates: &[&str]) -> Option<String> {
    // Jaro-Winkler favours common prefixes, which is what typos usually preserve.
    // 0.8 is tight enough that unrelated names are not proposed.
    const THRESHOLD: f64 = 0.8;

    candidates
        .iter()
        .map(|candidate| (candidate, strsim::jaro_winkler(unknown, candidate)))
        .filter(|(_, score)| *score >= THRESHOLD)
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(candidate, _)| (*candidate).to_string())
}

/// A collection of findings against one configuration file.
///
/// Implements [`Diagnostic`] so the whole set renders as a single, coherent
/// report rather than as a stream of unrelated errors.
#[derive(Debug, thiserror::Error)]
#[error("{}", summary(.findings))]
pub struct Report {
    /// The source file, for rendering the excerpt.
    pub named_source: miette::NamedSource<String>,
    /// Everything we found.
    pub findings: Vec<Finding>,
}

fn summary(findings: &[Finding]) -> String {
    let errors = findings.iter().filter(|f| f.is_error()).count();
    let warnings = findings.len() - errors;
    match (errors, warnings) {
        (0, 0) => "configuration is valid".into(),
        (0, warnings) => format!("configuration has {}", plural(warnings, "warning")),
        (errors, 0) => format!("configuration has {}", plural(errors, "error")),
        (errors, warnings) => format!(
            "configuration has {} and {}",
            plural(errors, "error"),
            plural(warnings, "warning")
        ),
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

impl Report {
    /// Build a report over a named source.
    pub fn new(name: impl AsRef<str>, source: impl Into<String>, findings: Vec<Finding>) -> Self {
        Self {
            named_source: miette::NamedSource::new(name, source.into()).with_language("yaml"),
            findings,
        }
    }

    /// Whether any finding blocks execution.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(Finding::is_error)
    }

    /// Whether there is nothing at all to report.
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

impl Diagnostic for Report {
    fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        Some(Box::new("gh_settings::config::invalid"))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(if self.has_errors() {
            miette::Severity::Error
        } else {
            miette::Severity::Warning
        })
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.named_source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let labels: Vec<_> = self
            .findings
            .iter()
            .filter_map(|finding| {
                finding.span.map(|span| {
                    LabeledSpan::new_with_span(
                        Some(
                            finding
                                .label
                                .clone()
                                .unwrap_or_else(|| finding.message.clone()),
                        ),
                        span.span,
                    )
                })
            })
            .collect();
        (!labels.is_empty()).then(|| Box::new(labels.into_iter()) as Box<dyn Iterator<Item = _>>)
    }

    fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        // Deduplicate: the same advice repeated once per occurrence is noise,
        // and the underlines already show how many there are.
        let mut seen = std::collections::HashSet::new();
        let helps: Vec<String> = self
            .findings
            .iter()
            .filter_map(|finding| finding.help.clone())
            .filter(|help| seen.insert(help.clone()))
            .collect();
        (!helps.is_empty()).then(|| Box::new(helps.join("\n")) as Box<dyn std::fmt::Display>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn suggests_close_matches() {
        assert_eq!(
            suggest("descrption", &["description", "homepage", "topics"]),
            Some("description".into())
        );
    }

    #[test]
    fn stays_quiet_when_nothing_is_close() {
        assert_eq!(suggest("zzzzzz", &["description", "homepage"]), None);
    }

    #[test]
    fn picks_the_best_of_several_candidates() {
        assert_eq!(
            suggest("has_issue", &["has_issues", "has_wiki", "has_projects"]),
            Some("has_issues".into())
        );
    }

    #[test]
    fn summarises_mixed_findings() {
        let findings = vec![
            Finding::error("a", "boom"),
            Finding::error("b", "bang"),
            Finding::warning("c", "meh"),
        ];
        assert_eq!(
            summary(&findings),
            "configuration has 2 errors and 1 warning"
        );
    }

    #[test]
    fn summarises_a_single_error() {
        assert_eq!(
            summary(&[Finding::error("a", "boom")]),
            "configuration has 1 error"
        );
    }

    #[test]
    fn repeated_advice_is_only_shown_once() {
        use miette::Diagnostic;
        let report = Report::new(
            "settings.yml",
            "",
            vec![
                Finding::error("a", "one").help("same advice"),
                Finding::error("b", "two").help("same advice"),
                Finding::error("c", "three").help("other advice"),
            ],
        );
        let help = report.help().expect("help").to_string();
        assert_eq!(help, "same advice\nother advice");
    }

    #[test]
    fn warnings_alone_do_not_make_a_report_fail() {
        let report = Report::new("settings.yml", "", vec![Finding::warning("a", "meh")]);
        assert!(!report.has_errors());
        assert!(!report.is_empty());
    }
}
