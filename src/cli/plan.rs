//! `gh settings plan`.
//!
//! Read-only by construction: the transport is built in read-only mode, so even a
//! bug in a resource cannot turn a plan into an apply.

use miette::Result;

use crate::cli::context::Context;
use crate::cli::exit;

/// Arguments for `plan`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Write the plan to a file for `sync --plan` to apply later.
    #[arg(long, value_name = "PATH")]
    pub out: Option<std::path::PathBuf>,

    /// Delete items present on GitHub but absent from the configuration.
    #[arg(long, conflicts_with = "no_prune")]
    pub prune: bool,

    /// Never delete anything, overriding the configuration.
    #[arg(long)]
    pub no_prune: bool,
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
    let config = ctx.load_config().await?;

    // Refuse to plan against a configuration we know is wrong: the resulting
    // diff would be meaningless.
    let findings = ctx.engine.validate(&config, &ctx.args.only);
    if findings.iter().any(crate::config::Finding::is_error) {
        let report = crate::config::Report::new(config.sources.clone(), findings);
        eprintln!("{:?}", miette::Report::new(report));
        return Ok(exit::FAILURE);
    }

    let plan = ctx
        .engine
        .plan(
            ctx.client(),
            &ctx.target,
            &config,
            &args.prune_opts(),
            &ctx.args.only,
        )
        .await?;

    if let Some(path) = &args.out {
        let artifact = serde_json::to_string_pretty(&plan.to_artifact())
            .map_err(|error| miette::miette!("could not serialise the plan: {error}"))?;
        std::fs::write(path, format!("{artifact}\n"))
            .map_err(|error| miette::miette!("could not write {}: {error}", path.display()))?;
        eprintln!("Wrote {}", path.display());
    }

    if ctx.args.is_json() {
        println!("{}", ctx.json.plan(&plan));
    } else {
        print!("{}", ctx.human.plan(&plan));
    }

    // A distinct exit code so CI can detect drift without treating it as an error.
    Ok(if plan.is_empty() {
        exit::SUCCESS
    } else {
        exit::CHANGES_PENDING
    })
}
