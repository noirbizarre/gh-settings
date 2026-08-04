//! `gh settings` — declarative GitHub repository settings.

use std::process::ExitCode;

use clap::Parser;
use gh_settings::cli::{Cli, Command, context::Context, exit};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    install_logging(&cli);
    install_diagnostics(&cli);

    match run(cli).await {
        Ok(code) => ExitCode::from(code as u8),
        Err(report) => {
            eprintln!("{report:?}");
            ExitCode::from(exit::FAILURE as u8)
        }
    }
}

async fn run(cli: Cli) -> miette::Result<i32> {
    match &cli.command {
        // `validate` and `schema` deliberately need no network and no
        // repository, so they work in a pull request CI job with no credentials.
        Command::Schema(args) => gh_settings::cli::schema::run(args),

        // Documentation generators. Like `schema`, they need no network and no
        // repository, so they run in any CI job.
        Command::Internal(args) => gh_settings::cli::internal::run(args),

        Command::Validate(args) => {
            let ctx = Context::new(cli.global.clone(), true).await?;
            let config = ctx.load_config().await?;
            gh_settings::cli::validate::run(
                args,
                &config,
                &ctx.engine,
                &ctx.args.only,
                ctx.args.is_json(),
                &ctx.json,
            )
        }

        Command::Plan(args) => {
            // Read-only transport: a bug in a resource cannot turn a plan into an
            // apply.
            let ctx = Context::new(cli.global.clone(), true).await?;
            gh_settings::cli::plan::run(args, &ctx).await
        }

        Command::Sync(args) => {
            let ctx = Context::new(cli.global.clone(), args.dry_run).await?;
            gh_settings::cli::sync::run(args, &ctx).await
        }

        Command::Export(args) => {
            let ctx = Context::new(cli.global.clone(), true).await?;
            gh_settings::cli::export::run(args, &ctx).await
        }

        Command::Doctor(args) => {
            let ctx = Context::new(cli.global.clone(), true).await?;
            gh_settings::cli::doctor::run(args, &ctx).await
        }
    }
}

/// Set up tracing, honouring `RUST_LOG` over `--debug`.
fn install_logging(cli: &Cli) {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(cli.global.log_filter()));

    // Logs go to stderr so that `--format json` on stdout stays machine-readable.
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// Configure how diagnostics are rendered.
fn install_diagnostics(cli: &Cli) {
    let color = cli.global.color_override().unwrap_or_else(|| {
        std::env::var_os("NO_COLOR").is_none()
            && std::io::IsTerminal::is_terminal(&std::io::stderr())
    });

    let _ = miette::set_hook(Box::new(move |_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .color(color)
                .unicode(color)
                .context_lines(2)
                .build(),
        )
    }));
}
