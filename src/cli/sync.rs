//! `gh settings sync`.
//!
//! Plan, show, confirm, apply. Confirmation is required for destructive changes
//! unless `--yes` is given, and is impossible to skip accidentally in CI because
//! a non-interactive terminal without `--yes` is refused rather than assumed.

use miette::Result;

use crate::cli::context::Context;
use crate::cli::exit;
use crate::engine::apply::{ApplyOptions, ApplyReport};
use crate::engine::plan::{ArtifactError, PlanArtifact};

/// Arguments for `sync`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Apply without asking for confirmation.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Apply a plan previously written by `plan --out`.
    #[arg(long, value_name = "PATH")]
    pub plan: Option<std::path::PathBuf>,

    /// Delete items present on GitHub but absent from the configuration.
    #[arg(long, conflicts_with = "no_prune")]
    pub prune: bool,

    /// Never delete anything, overriding the configuration.
    #[arg(long)]
    pub no_prune: bool,

    /// Keep going after a failure.
    #[arg(long)]
    pub continue_on_error: bool,

    /// Show what would happen without changing anything.
    #[arg(long)]
    pub dry_run: bool,
}

impl Args {
    /// The prune override implied by the flags.
    pub fn prune_opts(&self) -> crate::resources::PruneOpts {
        crate::resources::PruneOpts {
            force: match (self.prune, self.no_prune) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            },
        }
    }
}

/// Run the command.
pub async fn run(args: &Args, ctx: &Context) -> Result<i32> {
    let plan = match &args.plan {
        Some(path) => load_saved_plan(args, ctx, path).await?,
        None => compute_plan(args, ctx).await?,
    };

    if plan.is_empty() {
        // Nothing to do is the *common* case for anything automated, so this
        // path must still honour `--format json`. Emitting human text here left
        // a consumer parsing stdout broken precisely when everything was fine.
        if ctx.args.is_json() {
            println!("{}", ctx.json.apply(&ApplyReport::empty()));
        } else {
            print!("{}", ctx.human.plan(&plan));
        }
        return Ok(exit::SUCCESS);
    }

    if !ctx.args.is_json() {
        print!("{}", ctx.human.plan(&plan));
        println!();
    }

    // Before asking for confirmation, not after: there is no point confirming
    // changes we are about to refuse.
    if !args.dry_run {
        let blocked = preflight(ctx, &plan).await;
        if !blocked.is_empty() {
            if ctx.args.is_json() {
                println!("{}", ctx.json.refused(&blocked));
            } else {
                eprint!("{}", render_refusal(&blocked));
            }
            return Ok(exit::FAILURE);
        }
    }

    if !args.dry_run && !confirm(args, &plan)? {
        eprintln!("Aborted.");
        return Ok(exit::SUCCESS);
    }

    let report = ctx
        .engine
        .apply(
            ctx.client(),
            &ctx.target,
            &plan,
            &ApplyOptions {
                continue_on_error: args.continue_on_error,
                dry_run: args.dry_run,
            },
        )
        .await;

    if ctx.args.is_json() {
        println!("{}", ctx.json.apply(&report));
    } else {
        print!("{}", ctx.human.apply(&report));
        if report.has_permission_failure() {
            eprint!("{}", permission_explanation(ctx, &report));
        }
    }

    Ok(if report.is_success() {
        exit::SUCCESS
    } else {
        exit::FAILURE
    })
}

/// Decide whether any pending change is *certain* to be rejected.
///
/// Returns the blocked resources and the reason for each; empty means proceed.
///
/// Without this, a permission problem is discovered only after a failed write —
/// and with `--continue-on-error`, after several. The information needed to say
/// so up front was already available; it was simply never consulted.
///
/// # Conservatism
///
/// This only ever refuses on [`Capability::Impossible`], which
/// [`Requirement::verdict`](crate::resources::Requirement::verdict) returns only
/// from evidence: an advertised classic scope that is absent, or the Actions
/// `GITHUB_TOKEN` against a permission no workflow can be granted.
///
/// A token we could not introspect yields [`Capability::Unknown`] and is allowed
/// through, so GitHub's own answer is what the user sees. A pre-flight that
/// blocked a token it merely failed to understand would be unappealable — there
/// is no flag to overrule it — and would be worse than the problem it solves.
async fn preflight(
    ctx: &Context,
    plan: &crate::engine::Plan,
) -> Vec<(crate::resources::ResourceId, &'static str)> {
    let pending: Vec<crate::resources::ResourceId> = plan
        .resources
        .iter()
        .filter(|resource| !resource.changes.is_empty())
        .map(|resource| resource.id)
        .collect();

    if pending.is_empty() {
        return Vec::new();
    }

    // Costs a few requests, so it happens only when there is something to write.
    let auth = super::doctor::introspect(ctx).await;

    pending
        .iter()
        .filter_map(|id| {
            let resource = ctx.engine.registry().get(*id)?;
            let reason = resource.requirement().verdict(auth.as_ref()).reason()?;
            Some((*id, reason))
        })
        .collect()
}

/// Render a pre-flight refusal for a human.
fn render_refusal(blocked: &[(crate::resources::ResourceId, &'static str)]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("Refusing to start: this token cannot make some of these changes.\n\n");
    for (id, reason) in blocked {
        let _ = writeln!(out, "  ✘ {:<12} {reason}", id.title());
    }
    out.push_str("\nNothing was changed. Run `gh settings doctor` for the full picture.\n");
    out
}

/// Explain a permission failure in terms of the permission that was missing.
///
/// A `403` almost always means the wrong kind of token rather than a mistake in
/// the configuration. Telling the user to run `doctor` made them fetch
/// information we were already holding: the failing resource is known here, and
/// so is its [`Requirement`](crate::resources::Requirement). This names what to
/// grant instead of where to go and look it up.
fn permission_explanation(ctx: &Context, report: &ApplyReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push('\n');
    out.push_str("Some changes were refused for permission reasons.\n");

    for id in report.permission_denied_resources() {
        let Some(resource) = ctx.engine.registry().get(id) else {
            continue;
        };
        let requirement = resource.requirement();

        let _ = writeln!(out, "\n  {} needs:", id.title());
        let _ = writeln!(
            out,
            "    fine-grained token   {}",
            requirement.fine_grained_summary()
        );
        let _ = writeln!(
            out,
            "    classic token        {}",
            requirement.classic_summary()
        );

        // The one thing no amount of granting will fix — but only worth saying
        // when it could actually be the cause. Shown to someone using a
        // personal access token it is a false lead, sending them to look for a
        // setting that has no bearing on their failure.
        if crate::github::auth::in_github_actions()
            && let Some(note) = requirement.github_token_note
        {
            let _ = writeln!(out, "    note                 {note}");
        }
    }

    out.push_str("\nRun `gh settings doctor` for the full picture.\n");
    out
}

/// Compute a fresh plan from the configuration file.
async fn compute_plan(args: &Args, ctx: &Context) -> Result<crate::engine::Plan> {
    let config = ctx.load_config()?;

    let findings = ctx.engine.validate(&config, &ctx.args.only);
    if findings.iter().any(crate::config::Finding::is_error) {
        let report = crate::config::Report::new(
            config.path.display().to_string(),
            config.source().to_string(),
            findings,
        );
        return Err(miette::Report::new(report));
    }

    Ok(ctx
        .engine
        .plan(
            ctx.client(),
            &ctx.target,
            &config,
            &args.prune_opts(),
            &ctx.args.only,
        )
        .await?)
}

/// Load a saved plan and verify it still describes reality.
///
/// A reviewed plan that silently applies something else would defeat the purpose
/// of having a plan artifact at all, so drift is a hard error (ADR-010).
async fn load_saved_plan(
    args: &Args,
    ctx: &Context,
    path: &std::path::Path,
) -> Result<crate::engine::Plan> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| miette::miette!("could not read {}: {error}", path.display()))?;

    let artifact: PlanArtifact = serde_json::from_str(&contents)
        .map_err(|error| miette::miette!("{} is not a valid plan file: {error}", path.display()))?;

    let saved = artifact.to_plan()?;

    if saved.target != ctx.target {
        return Err(ArtifactError::WrongRepository {
            expected: saved.target.slug(),
            actual: ctx.target.slug(),
        }
        .into());
    }

    // Recompute and compare: the repository may have changed since the plan was
    // reviewed, in which case applying it blind would be wrong.
    let fresh = compute_plan(args, ctx).await?;
    if fresh.fingerprint() != saved.fingerprint() {
        return Err(ArtifactError::Drift.into());
    }

    Ok(saved)
}

/// Ask for confirmation when it is warranted.
fn confirm(args: &Args, plan: &crate::engine::Plan) -> Result<bool> {
    if args.yes {
        return Ok(true);
    }

    // Never guess in a pipeline. Requiring `--yes` makes the intent explicit in
    // the workflow file, where a reviewer can see it.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err(miette::miette!(
            help = "pass `--yes` to apply without confirmation",
            "refusing to apply without confirmation on a non-interactive terminal"
        ));
    }

    let prompt = if plan.has_destructive() {
        "This will delete existing configuration. Apply?"
    } else {
        "Apply these changes?"
    };

    demand::Confirm::new(prompt)
        .affirmative("Yes")
        .negative("No")
        .run()
        .map_err(|error| miette::miette!("could not read a response: {error}"))
}
