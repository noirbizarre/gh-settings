//! User-facing diagnostics.
//!
//! Validation output is a product surface, not an afterthought. Every diagnostic
//! carries a span into the configuration file so `miette` can underline the exact
//! offending node, and — wherever we can compute one — a concrete suggestion.

use miette::{Diagnostic, LabeledSpan};

use super::source::{FileSpan, SourceId, Sources};

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

/// Findings belonging to one document, rendered against that document's text.
///
/// Exists because miette resolves every label of a diagnostic against a single
/// `source_code`. A report spanning several documents must therefore *be*
/// several diagnostics, related to one another.
#[derive(Debug, thiserror::Error)]
#[error("inherited from {name}")]
struct FileReport {
    /// Display name of the document.
    name: String,
    /// Its text, for the excerpt.
    named_source: miette::NamedSource<String>,
    /// The findings that belong to it.
    findings: Vec<Finding>,
}

impl Diagnostic for FileReport {
    fn severity(&self) -> Option<miette::Severity> {
        // Its own severity, not the parent's: a base file contributing only
        // warnings must not be announced as an error.
        Some(if self.findings.iter().any(Finding::is_error) {
            miette::Severity::Error
        } else {
            miette::Severity::Warning
        })
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.named_source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        labels_for(&self.findings, |_| true)
    }

    fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        deduplicated_help(&self.findings)
    }
}

/// A collection of findings against a configuration.
///
/// Implements [`Diagnostic`] so the whole set renders as a single, coherent
/// report rather than as a stream of unrelated errors. Findings from documents
/// other than the root are grouped and surfaced through
/// [`Diagnostic::related`], each with its own text — because an offset computed
/// against one document is still a valid index into another, and rendering it
/// against the wrong one produces a confident underline over unrelated
/// characters instead of an error.
#[derive(Debug, thiserror::Error)]
#[error("{}", summary(.findings))]
pub struct Report {
    /// Every document that contributed, for rendering.
    pub sources: Sources,
    /// Everything we found, across every document.
    pub findings: Vec<Finding>,
    /// Findings from non-root documents, grouped so `related` can hand miette
    /// diagnostics that own their `source_code`.
    inherited: Vec<FileReport>,
    /// The root document's text, which `source_code` returns.
    root_source: miette::NamedSource<String>,
}

/// Build miette labels for the findings a predicate selects.
fn labels_for(
    findings: &[Finding],
    select: impl Fn(&Finding) -> bool,
) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
    let labels: Vec<_> = findings
        .iter()
        .filter(|finding| select(finding))
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

/// Advice from a set of findings, with repeats removed.
///
/// The same advice once per occurrence is noise, and the underlines already
/// show how many there are.
fn deduplicated_help(findings: &[Finding]) -> Option<Box<dyn std::fmt::Display + '_>> {
    let mut seen = std::collections::HashSet::new();
    let helps: Vec<String> = findings
        .iter()
        .filter_map(|finding| finding.help.clone())
        .filter(|help| seen.insert(help.clone()))
        .collect();
    (!helps.is_empty()).then(|| Box::new(helps.join("\n")) as Box<dyn std::fmt::Display>)
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
    /// Build a report over every document that contributed.
    pub fn new(sources: Sources, findings: Vec<Finding>) -> Self {
        // Group eagerly: `Diagnostic::related` yields borrowed diagnostics, so
        // the report has to own them.
        let inherited = sources
            .iter()
            .filter(|file| file.id != SourceId::ROOT)
            .filter_map(|file| {
                let mine: Vec<Finding> = findings
                    .iter()
                    .filter(|finding| finding.span.is_some_and(|span| span.source == file.id))
                    .cloned()
                    .collect();
                (!mine.is_empty()).then(|| FileReport {
                    name: file.name.clone(),
                    named_source: sources.named(file.id),
                    findings: mine,
                })
            })
            .collect();

        let root_source = sources.named(SourceId::ROOT);
        Self {
            sources,
            findings,
            inherited,
            root_source,
        }
    }

    /// Build a report over a single document.
    ///
    /// The common case, and what the tests use.
    pub fn single(
        name: impl Into<String>,
        source: impl AsRef<str>,
        findings: Vec<Finding>,
    ) -> Self {
        let (sources, _) = Sources::root(name, source);
        Self::new(sources, findings)
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
        Some(&self.root_source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        // Only the root document's spans: every label here is resolved against
        // `source_code` above, so a span from elsewhere would be read against
        // the wrong text. The rest go through `related`.
        labels_for(&self.findings, |finding| {
            finding
                .span
                .is_none_or(|span| span.source == SourceId::ROOT)
        })
    }

    fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        deduplicated_help(&self.findings)
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        (!self.inherited.is_empty()).then(|| {
            Box::new(
                self.inherited
                    .iter()
                    .map(|report| report as &dyn Diagnostic),
            ) as Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>
        })
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
        let report = Report::single(
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
        let report = Report::single("settings.yml", "", vec![Finding::warning("a", "meh")]);
        assert!(!report.has_errors());
        assert!(!report.is_empty());
    }

    mod several_documents {
        use super::{Finding, Report};
        use crate::config::{FileSpan, Sources};
        use miette::{Diagnostic, SourceSpan};
        use pretty_assertions::assert_eq;

        /// Two documents whose contents differ, and whose offsets overlap.
        ///
        /// The overlap is the point: an offset taken from the second is a
        /// perfectly valid index into the first, so nothing but the source id
        /// distinguishes a correct render from a confidently wrong one.
        fn two_documents() -> (Sources, Finding, Finding) {
            const LOCAL: &str = "topics:\n  - LOCAL_TOPIC\n";
            const BASE: &str = "labels:\n  - name: BASE_LABEL\n";

            let (mut sources, root) = Sources::root("local.yml", LOCAL);
            let base = sources.push("acme/.github@v1", BASE);

            let local_finding = Finding::error("gh_settings::test::local", "a local problem")
                .at(FileSpan::new(
                    root,
                    SourceSpan::new(
                        LOCAL.find("LOCAL_TOPIC").unwrap().into(),
                        "LOCAL_TOPIC".len(),
                    ),
                ))
                .labelled("declared here");

            let base_finding = Finding::error("gh_settings::test::base", "an inherited problem")
                .at(FileSpan::new(
                    base,
                    SourceSpan::new(BASE.find("BASE_LABEL").unwrap().into(), "BASE_LABEL".len()),
                ))
                .labelled("inherited from here");

            (sources, local_finding, base_finding)
        }

        fn render(report: Report) -> String {
            format!("{:?}", miette::Report::new(report))
        }

        #[test]
        fn a_finding_renders_against_the_file_it_came_from_not_the_file_beside_it() {
            let (sources, local, base) = two_documents();
            let rendered = render(Report::new(sources, vec![local, base]));

            // Each document's own text, quoted under its own name. Rendering the
            // inherited span against the local file would underline
            // `LOCAL_TOPIC` — a valid offset, an entirely wrong answer.
            assert!(rendered.contains("local.yml"), "{rendered}");
            assert!(rendered.contains("acme/.github@v1"), "{rendered}");
            assert!(rendered.contains("LOCAL_TOPIC"), "{rendered}");
            assert!(rendered.contains("BASE_LABEL"), "{rendered}");
            assert!(rendered.contains("inherited from here"), "{rendered}");
        }

        #[test]
        fn only_the_root_documents_findings_are_labelled_on_the_report_itself() {
            let (sources, local, base) = two_documents();
            let report = Report::new(sources, vec![local, base]);

            let labels: Vec<_> = report.labels().expect("root labels").collect();
            assert_eq!(
                labels.len(),
                1,
                "only the local finding belongs to the root"
            );

            let related: Vec<_> = report.related().expect("an inherited document").collect();
            assert_eq!(related.len(), 1);
        }

        #[test]
        fn an_inherited_document_announces_its_own_severity() {
            // A base file contributing only warnings must not be announced as an
            // error just because the local file has one.
            let (mut sources, root) = Sources::root("local.yml", "topics:\n  - rust\n");
            let base = sources.push("acme/.github@v1", "labels:\n  - name: bug\n");

            let report = Report::new(
                sources,
                vec![
                    Finding::error("gh_settings::test::local", "boom")
                        .at(FileSpan::new(root, SourceSpan::new(0.into(), 6usize))),
                    Finding::warning("gh_settings::test::base", "meh")
                        .at(FileSpan::new(base, SourceSpan::new(0.into(), 6usize))),
                ],
            );

            assert_eq!(report.severity(), Some(miette::Severity::Error));
            let related: Vec<_> = report.related().expect("one inherited").collect();
            assert_eq!(related[0].severity(), Some(miette::Severity::Warning));
        }

        #[test]
        fn a_single_document_produces_no_related_diagnostics() {
            // The property that keeps every existing snapshot byte-identical:
            // with one document the report renders exactly as it always did.
            let report = Report::single(
                "settings.yml",
                "topics:\n  - rust\n",
                vec![Finding::error("gh_settings::test::x", "boom")],
            );
            assert!(report.related().is_none());
        }

        #[test]
        fn a_document_that_contributed_no_findings_is_not_rendered() {
            // Fetching a base file that turns out to be fine should not add an
            // empty section to the output.
            let (mut sources, _) = Sources::root("local.yml", "topics: []\n");
            sources.push("acme/.github@v1", "labels: []\n");

            let report = Report::new(
                sources,
                vec![Finding::error("gh_settings::test::x", "boom")],
            );
            assert!(report.related().is_none());
        }
    }
}
