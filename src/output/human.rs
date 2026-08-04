//! Human-readable rendering.

use std::fmt::Write as _;

use crate::engine::{ApplyReport, Plan};
use crate::github::{AuthStatus, TokenKind};
use crate::resources::{Capability, Change, Counts, Op};

use super::theme::Theme;

/// Renders plans and reports for humans.
#[derive(Debug, Clone, Default)]
pub struct HumanRenderer {
    theme: Theme,
    /// Whether to show field-level detail under each change.
    verbose: bool,
}

impl HumanRenderer {
    /// Build a renderer.
    pub fn new(theme: Theme, verbose: bool) -> Self {
        Self { theme, verbose }
    }

    /// Colourise a change line according to its operation.
    fn paint_op(&self, op: Op, text: &str) -> String {
        match op {
            Op::Create => self.theme.create(text),
            Op::Update => self.theme.update(text),
            Op::Delete | Op::Recreate => self.theme.delete(text),
        }
    }

    /// Render a plan.
    pub fn plan(&self, plan: &Plan) -> String {
        let mut out = String::new();

        if plan.is_empty() {
            let _ = writeln!(
                out,
                "{} {} is up to date.",
                self.theme.success("✔"),
                plan.target
            );
            return out;
        }

        let _ = writeln!(
            out,
            "{}",
            self.theme.heading(&format!("Plan for {}", plan.target))
        );
        let _ = writeln!(out);

        for resource in &plan.resources {
            let _ = writeln!(out, "{}", self.theme.heading(resource.id.title()));
            for change in &resource.changes {
                let _ = writeln!(out, "  {}", self.change_line(change));
                if self.verbose {
                    for field in &change.fields {
                        let detail = match (&field.before, &field.after) {
                            (Some(before), Some(after)) => {
                                format!("{}: {before} → {after}", field.field)
                            }
                            (None, Some(after)) => format!("{}: {after}", field.field),
                            (Some(before), None) => {
                                format!("{}: {before} → (removed)", field.field)
                            }
                            (None, None) => field.field.clone(),
                        };
                        let _ = writeln!(out, "      {}", self.theme.dim(&detail));
                    }
                }
            }
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "{}", self.summary(&plan.counts()));

        if plan.has_destructive() {
            let _ = writeln!(
                out,
                "{} this plan deletes existing configuration.",
                self.theme.warn("!")
            );
        }

        out
    }

    /// A single `+ create label bug` line.
    fn change_line(&self, change: &Change) -> String {
        self.paint_op(
            change.op,
            &format!("{} {}", change.op.sigil(), change.summary),
        )
    }

    /// The `3 to create, 1 to update` summary line.
    pub fn summary(&self, counts: &Counts) -> String {
        if counts.is_empty() {
            return "No changes.".to_string();
        }

        let mut parts = Vec::new();
        if counts.create > 0 {
            parts.push(self.theme.create(&format!("{} to create", counts.create)));
        }
        if counts.update > 0 {
            parts.push(self.theme.update(&format!("{} to update", counts.update)));
        }
        if counts.recreate > 0 {
            parts.push(
                self.theme
                    .delete(&format!("{} to recreate", counts.recreate)),
            );
        }
        if counts.delete > 0 {
            parts.push(self.theme.delete(&format!("{} to delete", counts.delete)));
        }

        format!("{}.", parts.join(", "))
    }

    /// Render the outcome of an apply.
    pub fn apply(&self, report: &ApplyReport) -> String {
        let mut out = String::new();

        for outcome in &report.outcomes {
            match outcome {
                crate::engine::ApplyOutcome::Applied(change) => {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        self.theme.success("✔"),
                        self.paint_op(change.op, &change.summary)
                    );
                }
                crate::engine::ApplyOutcome::Failed { change, error } => {
                    let _ = writeln!(
                        out,
                        "{} {} — {error}",
                        self.theme.error("✘"),
                        change.summary
                    );
                }
                crate::engine::ApplyOutcome::Skipped(change) => {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        self.theme.dim("·"),
                        self.theme.dim(&format!("skipped {}", change.summary))
                    );
                }
            }
        }

        let _ = writeln!(out);

        let applied = report.applied_counts();
        if report.is_success() {
            let _ = writeln!(
                out,
                "{} applied {} change{}.",
                self.theme.success("✔"),
                applied.total(),
                if applied.total() == 1 { "" } else { "s" }
            );
        } else {
            let failed = report.failures().count();
            let _ = writeln!(
                out,
                "{} {failed} change{} failed, {} applied, {} skipped.",
                self.theme.error("✘"),
                if failed == 1 { "" } else { "s" },
                applied.total(),
                report.skipped()
            );
        }

        out
    }

    /// Render the `doctor` report.
    ///
    /// See plan §6b: the point is to say plainly what *cannot* work and why,
    /// rather than letting the user discover it through a bare `HTTP 403`.
    pub fn doctor(
        &self,
        gh_version: Option<&str>,
        auth: Option<&AuthStatus>,
        capabilities: &[(crate::resources::ResourceId, Capability)],
    ) -> String {
        let mut out = String::new();

        let _ = writeln!(out, "{}", self.theme.heading("Environment"));
        match gh_version {
            Some(version) => {
                let _ = writeln!(
                    out,
                    "  {} gh CLI           {version}",
                    self.theme.success("✔")
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "  {} gh CLI           not found on PATH",
                    self.theme.error("✘")
                );
            }
        }

        match auth {
            Some(auth) => {
                let account = auth.account.as_deref().unwrap_or("unknown account");
                let _ = writeln!(
                    out,
                    "  {} Authentication   {} as {account}",
                    self.theme.success("✔"),
                    auth.hostname
                );

                let marker = if auth.token_kind == TokenKind::ActionsGitHubToken {
                    self.theme.warn("!")
                } else {
                    self.theme.success("✔")
                };
                let _ = writeln!(
                    out,
                    "  {marker} Token type       {}",
                    auth.token_kind.label()
                );

                let scopes = match &auth.scopes {
                    crate::github::auth::Scopes::Known(scopes) if scopes.is_empty() => {
                        "(none)".to_string()
                    }
                    crate::github::auth::Scopes::Known(scopes) => scopes.join(", "),
                    // Fine-grained and App tokens do not advertise scopes. Saying
                    // so is more useful than guessing.
                    crate::github::auth::Scopes::Unknown => {
                        "not reported by this token type".to_string()
                    }
                };
                let _ = writeln!(out, "    Scopes           {scopes}");
            }
            None => {
                let _ = writeln!(
                    out,
                    "  {} Authentication   not authenticated — run `gh auth login`",
                    self.theme.error("✘")
                );
            }
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "{}", self.theme.heading("Resources"));

        let width = capabilities
            .iter()
            .map(|(id, _)| id.as_str().len())
            .max()
            .unwrap_or(10);

        for (id, capability) in capabilities {
            let (marker, note) = match capability {
                Capability::Manageable => (self.theme.success("✔"), String::new()),
                Capability::Impossible(reason) => (self.theme.error("✘"), (*reason).to_string()),
                Capability::Unknown => (
                    self.theme.warn("?"),
                    "cannot determine from this token type".to_string(),
                ),
            };
            let line = format!("  {marker} {:<width$}  {note}", id.as_str(), width = width);
            let _ = writeln!(out, "{}", line.trim_end());
        }

        if capabilities
            .iter()
            .any(|(_, capability)| matches!(capability, Capability::Impossible(_)))
        {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "→ Use a personal access token or a GitHub App installation token."
            );
            let _ = writeln!(
                out,
                "  See https://noirbizarre.github.io/gh-settings/authentication/"
            );
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plan::Plan;
    use crate::github::Target;
    use crate::resources::{ResourceId, ResourcePlan};

    fn renderer() -> HumanRenderer {
        HumanRenderer::new(Theme::plain(), false)
    }

    fn plan_with(changes: Vec<Change>) -> Plan {
        let mut plan = Plan::new(Target::new("noirbizarre", "gh-settings"));
        let mut grouped: Vec<ResourcePlan> = Vec::new();
        for change in changes {
            match grouped.last_mut() {
                Some(last) if last.id == change.resource => last.changes.push(change),
                _ => grouped.push(ResourcePlan {
                    id: change.resource,
                    changes: vec![change],
                }),
            }
        }
        for resource in grouped {
            plan.push(resource);
        }
        plan
    }

    #[test]
    fn an_empty_plan_says_so_plainly() {
        let plan = Plan::new(Target::new("o", "r"));
        let rendered = renderer().plan(&plan);
        assert!(rendered.contains("up to date"), "{rendered}");
    }

    #[test]
    fn groups_changes_under_resource_headings() {
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Create, "bug").summary("create label bug"),
            Change::new(ResourceId::Topics, Op::Create, "rust").summary("add topic rust"),
        ]);
        let rendered = renderer().plan(&plan);
        assert!(rendered.contains("Labels"));
        assert!(rendered.contains("Topics"));
        assert!(rendered.contains("+ create label bug"));
        assert!(rendered.contains("+ add topic rust"));
    }

    #[test]
    fn sigils_distinguish_operations_without_colour() {
        // The output must remain unambiguous in a log file.
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Create, "a").summary("create a"),
            Change::new(ResourceId::Labels, Op::Update, "b").summary("update b"),
            Change::new(ResourceId::Labels, Op::Delete, "c").summary("delete c"),
        ]);
        let rendered = renderer().plan(&plan);
        assert!(rendered.contains("+ create a"));
        assert!(rendered.contains("~ update b"));
        assert!(rendered.contains("- delete c"));
    }

    #[test]
    fn warns_when_a_plan_is_destructive() {
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Delete, "legacy").summary("delete label legacy"),
        ]);
        let rendered = renderer().plan(&plan);
        assert!(
            rendered.contains("deletes existing configuration"),
            "{rendered}"
        );
    }

    #[test]
    fn does_not_warn_about_a_purely_additive_plan() {
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Create, "bug").summary("create label bug"),
        ]);
        assert!(!renderer().plan(&plan).contains("deletes existing"));
    }

    #[test]
    fn verbose_mode_shows_field_detail() {
        let plan = plan_with(vec![
            Change::new(ResourceId::Labels, Op::Update, "bug")
                .summary("update label bug")
                .fields(vec![crate::resources::FieldDiff::changed(
                    "color", "d73a4a", "b60205",
                )]),
        ]);
        let verbose = HumanRenderer::new(Theme::plain(), true).plan(&plan);
        assert!(verbose.contains("color: d73a4a → b60205"), "{verbose}");
        assert!(!renderer().plan(&plan).contains("d73a4a"));
    }

    #[test]
    fn summarises_counts() {
        let counts = Counts {
            create: 2,
            update: 1,
            delete: 3,
            recreate: 0,
        };
        assert_eq!(
            renderer().summary(&counts),
            "2 to create, 1 to update, 3 to delete."
        );
    }

    #[test]
    fn an_empty_summary_reads_naturally() {
        assert_eq!(renderer().summary(&Counts::default()), "No changes.");
    }

    #[test]
    fn doctor_explains_why_the_actions_token_cannot_work() {
        let auth = AuthStatus {
            hostname: "github.com".into(),
            account: Some("noirbizarre".into()),
            token_kind: TokenKind::ActionsGitHubToken,
            scopes: crate::github::auth::Scopes::Known(vec!["issues:write".into()]),
            admin_on_target: None,
        };
        let capabilities = vec![
            (
                ResourceId::Repository,
                Capability::Impossible("requires Administration: write"),
            ),
            (ResourceId::Labels, Capability::Manageable),
        ];
        let rendered = renderer().doctor(Some("gh 2.62.0"), Some(&auth), &capabilities);

        assert!(rendered.contains("Actions GITHUB_TOKEN"));
        assert!(rendered.contains("requires Administration: write"));
        assert!(rendered.contains("noirbizarre.github.io/gh-settings/authentication/"));
    }

    #[test]
    fn doctor_reports_unknown_scopes_honestly() {
        // Fine-grained tokens do not advertise scopes; claiming otherwise would
        // be worse than admitting we cannot tell.
        let auth = AuthStatus {
            hostname: "github.com".into(),
            account: None,
            token_kind: TokenKind::FineGrainedPat,
            scopes: crate::github::auth::Scopes::Unknown,
            admin_on_target: None,
        };
        let rendered = renderer().doctor(Some("gh 2.62.0"), Some(&auth), &[]);
        assert!(
            rendered.contains("not reported by this token type"),
            "{rendered}"
        );
    }

    #[test]
    fn doctor_reports_a_missing_gh() {
        let rendered = renderer().doctor(None, None, &[]);
        assert!(rendered.contains("not found on PATH"));
        assert!(rendered.contains("gh auth login"));
    }
}
