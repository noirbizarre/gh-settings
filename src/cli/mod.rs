//! Command line interface.
//!
//! The extension should be indistinguishable from a native `gh` command, so the
//! conventions here mirror the GitHub CLI: `-R owner/repo`, repository inference
//! from the git remote, `--json` for machine output, and `NO_COLOR` support.

pub mod context;
pub mod doctor;
pub mod export;
pub mod findings;
pub mod internal;
pub mod plan;
pub mod schema;
pub mod sync;
pub mod validate;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::github::Target;
use crate::output::Format;
use crate::resources::ResourceId;

/// Exit codes, which CI pipelines depend upon.
pub mod exit {
    /// Everything succeeded and nothing needs doing.
    pub const SUCCESS: i32 = 0;
    /// Something went wrong.
    pub const FAILURE: i32 = 1;
    /// `plan` found pending changes. Distinct from failure so a pipeline can
    /// detect drift without treating it as an error.
    pub const CHANGES_PENDING: i32 = 2;
}

/// Declarative GitHub repository settings.
#[derive(Debug, Parser)]
#[command(
    name = "gh-settings",
    bin_name = "gh settings",
    version,
    about = "Declarative GitHub repository settings",
    long_about = "Manage GitHub repository settings declaratively from `.github/settings.yml`.\n\n\
                  Requires the GitHub CLI for authentication. Note that most settings need a \n\
                  personal access token or GitHub App token: the Actions GITHUB_TOKEN cannot \n\
                  manage repository settings. Run `gh settings doctor` to check.",
    propagate_version = true
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,

    /// Global options.
    #[command(flatten)]
    pub global: GlobalArgs,
}

/// Options accepted by every subcommand.
#[derive(Debug, clap::Args, Clone)]
pub struct GlobalArgs {
    /// Repository to act on, as `owner/repo`.
    ///
    /// Inferred from the git remote when omitted.
    #[arg(short = 'R', long = "repo", global = true, value_name = "OWNER/REPO")]
    pub repo: Option<Target>,

    /// Path to the configuration file.
    ///
    /// Defaults to `.github/settings.yml`, searched for upwards from the current
    /// directory.
    #[arg(
        short,
        long,
        global = true,
        value_name = "PATH",
        env = "GH_SETTINGS_CONFIG"
    )]
    pub config: Option<PathBuf>,

    /// Limit the run to specific resources.
    ///
    /// Repeat or comma-separate, e.g. `--only labels,topics`.
    #[arg(long, global = true, value_name = "RESOURCE", value_delimiter = ',')]
    pub only: Vec<ResourceId>,

    /// Output format.
    #[arg(long, global = true, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Show field-level detail.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Colourise output.
    ///
    /// Detected from the terminal by default; `NO_COLOR` is honoured.
    #[arg(long, global = true, value_name = "WHEN", value_enum)]
    pub color: Option<ColorChoice>,

    /// Increase log verbosity. Repeat for more.
    #[arg(long, global = true, action = clap::ArgAction::Count)]
    pub debug: u8,
}

impl GlobalArgs {
    /// The colour override, if any.
    pub fn color_override(&self) -> Option<bool> {
        match self.color {
            Some(ColorChoice::Always) => Some(true),
            Some(ColorChoice::Never) => Some(false),
            Some(ColorChoice::Auto) | None => None,
        }
    }

    /// Whether machine-readable output was requested.
    pub fn is_json(&self) -> bool {
        self.format == Format::Json
    }

    /// The tracing filter implied by `--debug`.
    pub fn log_filter(&self) -> &'static str {
        match self.debug {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    }
}

/// When to colourise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorChoice {
    /// Detect from the terminal.
    Auto,
    /// Always colourise.
    Always,
    /// Never colourise.
    Never,
}

/// The available commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the configuration file without contacting GitHub.
    #[command(visible_alias = "check")]
    Validate(validate::Args),

    /// Show the changes required to reach the desired state.
    Plan(plan::Args),

    /// Apply the configuration to the repository.
    #[command(visible_alias = "apply")]
    Sync(sync::Args),

    /// Generate a configuration file from the repository's current state.
    Export(export::Args),

    /// Check that the environment can actually manage these settings.
    Doctor(doctor::Args),

    /// Print the JSON Schema for the configuration file.
    Schema(schema::Args),

    /// Generate documentation from the code.
    ///
    /// Hidden: these are build-time tools, not product surface. They exist so
    /// that documentation describing the code is produced *by* the code and
    /// cannot drift from it.
    #[command(hide = true)]
    Internal(internal::Args),
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("gh-settings").chain(args.iter().copied()))
            .expect("should parse")
    }

    #[test]
    fn parses_the_repository_flag() {
        let cli = parse(&["plan", "-R", "noirbizarre/gh-settings"]);
        assert_eq!(
            cli.global.repo,
            Some(Target::new("noirbizarre", "gh-settings"))
        );
    }

    #[test]
    fn rejects_a_malformed_repository() {
        assert!(Cli::try_parse_from(["gh-settings", "plan", "-R", "nope"]).is_err());
    }

    #[test]
    fn only_accepts_a_comma_separated_list() {
        let cli = parse(&["plan", "--only", "labels,topics"]);
        assert_eq!(
            cli.global.only,
            vec![ResourceId::Labels, ResourceId::Topics]
        );
    }

    #[test]
    fn only_accepts_repetition() {
        let cli = parse(&["plan", "--only", "labels", "--only", "topics"]);
        assert_eq!(
            cli.global.only,
            vec![ResourceId::Labels, ResourceId::Topics]
        );
    }

    #[test]
    fn an_unknown_resource_suggests_a_correction() {
        let error = Cli::try_parse_from(["gh-settings", "plan", "--only", "lables"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("did you mean `labels`?"), "{error}");
    }

    #[test]
    fn global_flags_work_after_the_subcommand() {
        // `gh` users expect this; requiring flags before the subcommand would
        // feel foreign.
        let cli = parse(&["sync", "--verbose", "--format", "json"]);
        assert!(cli.global.verbose);
        assert!(cli.global.is_json());
    }

    #[test]
    fn check_is_an_alias_for_validate() {
        assert!(matches!(parse(&["check"]).command, Command::Validate(_)));
    }

    #[test]
    fn apply_is_an_alias_for_sync() {
        assert!(matches!(parse(&["apply"]).command, Command::Sync(_)));
    }

    #[test]
    fn colour_choice_maps_to_an_override() {
        assert_eq!(parse(&["plan"]).global.color_override(), None);
        assert_eq!(
            parse(&["plan", "--color", "always"])
                .global
                .color_override(),
            Some(true)
        );
        assert_eq!(
            parse(&["plan", "--color", "never"]).global.color_override(),
            Some(false)
        );
    }

    #[test]
    fn debug_flags_escalate_the_log_filter() {
        assert_eq!(parse(&["plan"]).global.log_filter(), "warn");
        assert_eq!(parse(&["plan", "--debug"]).global.log_filter(), "info");
        assert_eq!(
            parse(&["plan", "--debug", "--debug"]).global.log_filter(),
            "debug"
        );
    }

    #[test]
    fn exit_codes_are_distinct() {
        // A pipeline distinguishes "drift detected" from "the run failed".
        assert_ne!(exit::SUCCESS, exit::CHANGES_PENDING);
        assert_ne!(exit::FAILURE, exit::CHANGES_PENDING);
    }
}
