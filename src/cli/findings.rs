//! One way to report configuration findings.
//!
//! `plan`, `sync` and `validate` all refuse to proceed on an invalid file, and
//! all three used to do it differently: one printed and returned a failure code,
//! one returned an error for `main` to render, and only the third honoured
//! `--format json`. So `plan --format json` on a broken file wrote a human
//! diagnostic to stderr and *nothing* to stdout, which is not something a
//! pipeline can act on.

use crate::cli::context::Context;
use crate::cli::exit;
use crate::config::{Config, Finding, Report};

/// Print findings in whichever form the caller asked for.
///
/// Machine output goes to stdout, human diagnostics to stderr, exactly as
/// everywhere else: stdout stays parseable.
pub fn emit(ctx: &Context, config: &Config, findings: &[Finding]) {
    if ctx.args.is_json() {
        println!("{}", ctx.json.validation(&config.sources, findings));
    } else {
        // Render through miette so the excerpt, underlines and helps are laid
        // out consistently with every other diagnostic this tool emits.
        let report = Report::new(config.sources.clone(), findings.to_vec());
        eprintln!("{:?}", miette::Report::new(report));
    }
}

/// Report the findings and stop, when the configuration cannot be used.
///
/// Returns `Some(exit_code)` when the caller must give up. Warnings alone are
/// not a reason to: a plan computed from a file with a warning is still a
/// truthful plan.
pub fn reject(ctx: &Context, config: &Config, findings: &[Finding]) -> Option<i32> {
    if !findings.iter().any(Finding::is_error) {
        return None;
    }

    emit(ctx, config, findings);
    Some(exit::FAILURE)
}
