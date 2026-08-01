//! `gh settings schema`.
//!
//! Emits the JSON Schema that drives editor completion and validation. CI diffs
//! this against the committed copy so the published contract can never silently
//! drift from the code.

use miette::Result;

use crate::cli::exit;

/// Arguments for `schema`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Write to a file instead of standard output.
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<std::path::PathBuf>,
}

/// Run the command.
pub fn run(args: &Args) -> Result<i32> {
    let rendered = crate::schema::render();

    match &args.output {
        Some(path) => {
            std::fs::write(path, &rendered)
                .map_err(|error| miette::miette!("could not write {}: {error}", path.display()))?;
            eprintln!("Wrote {}", path.display());
        }
        None => print!("{rendered}"),
    }

    Ok(exit::SUCCESS)
}
