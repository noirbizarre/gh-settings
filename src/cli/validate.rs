//! `gh settings validate`.
//!
//! Deliberately offline: validation must be usable as a fast pre-commit hook and
//! in pull request CI where no repository credentials are available.

use miette::Result;

use crate::cli::exit;
use crate::config::Report;

/// Arguments for `validate`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Treat warnings as errors.
    #[arg(long)]
    pub strict: bool,
}

/// Run the command.
pub fn run(
    args: &Args,
    config: &crate::config::Config,
    engine: &crate::engine::Engine,
    only: &[crate::resources::ResourceId],
    json: bool,
    renderer: &crate::output::JsonRenderer,
) -> Result<i32> {
    let findings = engine.validate(config, only);

    if json {
        println!("{}", renderer.validation(&findings));
        let failed = findings.iter().any(crate::config::Finding::is_error)
            || (args.strict && !findings.is_empty());
        return Ok(if failed { exit::FAILURE } else { exit::SUCCESS });
    }

    if findings.is_empty() {
        println!("✔ {} is valid.", config.path.display());
        return Ok(exit::SUCCESS);
    }

    let has_errors = findings.iter().any(crate::config::Finding::is_error);
    let report = Report::new(config.sources.clone(), findings);

    // Render through miette so the excerpt, underlines and helps are all laid
    // out consistently with every other diagnostic this tool emits.
    eprintln!("{:?}", miette::Report::new(report));

    Ok(if has_errors || args.strict {
        exit::FAILURE
    } else {
        exit::SUCCESS
    })
}
