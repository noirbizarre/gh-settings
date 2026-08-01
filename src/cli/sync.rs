//! `gh settings sync`.
//!
//! Plan, show, confirm, apply. Confirmation is required for destructive changes
//! unless `--yes` is given, and is impossible to skip accidentally in CI because
//! a non-interactive terminal without `--yes` is refused rather than assumed.

use miette::Result;

use crate::cli::context::Context;
use crate::cli::exit;
use crate::engine::apply::ApplyOptions;
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
        print!("{}", ctx.human.plan(&plan));
        return Ok(exit::SUCCESS);
    }

    if !ctx.args.is_json() {
        print!("{}", ctx.human.plan(&plan));
        println!();
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
        // A 403 almost always means the wrong kind of token rather than a
        // mistake in the configuration, so say so explicitly.
        if report.has_permission_failure() {
            eprintln!();
            eprintln!("Some changes were refused for permission reasons.");
            eprintln!("Run `gh settings doctor` to see what this token can manage.");
        }
    }

    Ok(if report.is_success() {
        exit::SUCCESS
    } else {
        exit::FAILURE
    })
}

/// Compute a fresh plan from the configuration file.
async fn compute_plan(args: &Args, ctx: &Context) -> Result<crate::engine::Plan> {
    let config = ctx.load_config()?;

    let findings = ctx.engine.validate(&config, &ctx.args.only);
    if findings.iter().any(crate::config::Finding::is_error) {
        let report = crate::config::Report::new(
            config.path.display().to_string(),
            config.source.clone(),
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
