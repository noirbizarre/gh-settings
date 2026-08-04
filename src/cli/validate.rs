//! `gh settings validate`.
//!
//! Deliberately offline: validation must be usable as a fast pre-commit hook and
//! in pull request CI where no repository credentials are available. It acts on
//! no repository at all, so it works outside a checkout too.

use miette::Result;

use crate::cli::context::Context;
use crate::cli::{exit, findings};

/// Arguments for `validate`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Treat warnings as errors.
    #[arg(long)]
    pub strict: bool,
}

/// Run the command.
pub async fn run(args: &Args, ctx: &Context) -> Result<i32> {
    let config = ctx.load_config().await?;
    let findings = ctx.engine.validate(&config, &ctx.args.only);

    let failed = findings.iter().any(crate::config::Finding::is_error)
        || (args.strict && !findings.is_empty());

    // A clean file still gets a JSON document, so a pipeline always has
    // something to parse; the human form says so in one line instead.
    if findings.is_empty() && !ctx.args.is_json() {
        println!("✔ {} is valid.", config.path.display());
    } else {
        findings::emit(ctx, &config, &findings);
    }

    Ok(if failed { exit::FAILURE } else { exit::SUCCESS })
}
