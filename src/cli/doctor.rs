//! `gh settings doctor`.
//!
//! Answers the question that otherwise costs users an hour: *why did that return
//! 403?* Plan §6b — the credential kind determines what is even possible, and the
//! Actions `GITHUB_TOKEN` cannot be granted `Administration: write` at all.

use miette::Result;

use crate::cli::context::Context;
use crate::cli::exit;
use crate::github::auth::{self, Scopes};
use crate::github::{AuthStatus, TokenKind};
use crate::resources::Capability;
use crate::resources::ResourceId;

/// Arguments for `doctor`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Also fail when a capability cannot be determined, not just when it is
    /// certainly impossible.
    #[arg(long)]
    pub strict: bool,
}

/// Run the command.
pub async fn run(args: &Args, ctx: &Context) -> Result<i32> {
    let gh_version = auth::gh_version(&gh_program()).await;

    let auth_status = if gh_version.is_some() {
        introspect(ctx).await
    } else {
        None
    };

    let capabilities = capabilities(ctx, auth_status.as_ref());
    let inheritance = crate::resources::Requirement::CONTENTS.verdict(auth_status.as_ref());

    if ctx.args.is_json() {
        println!(
            "{}",
            ctx.json.doctor(
                gh_version.as_deref(),
                auth_status.as_ref(),
                &capabilities,
                &inheritance
            )
        );
    } else {
        println!(
            "{}",
            ctx.human.doctor(
                gh_version.as_deref(),
                auth_status.as_ref(),
                &capabilities,
                &inheritance,
            )
        );
    }

    let verdicts = || capabilities.iter().map(|(_, c)| c).chain([&inheritance]);

    // Without `--strict` the exit code tracks the `ok` field exactly, so a
    // pipeline gets the same answer whichever it reads. `--strict` widens the
    // net to Unknown, which ADR-015 refuses to treat as a failure on its own.
    let blocked = gh_version.is_none()
        || auth_status.is_none()
        || verdicts().any(Capability::is_certainly_impossible)
        || (args.strict && verdicts().any(|c| !matches!(c, Capability::Manageable)));

    Ok(if blocked {
        exit::FAILURE
    } else {
        exit::SUCCESS
    })
}

/// Work out what each resource can and cannot do with the current credential.
///
/// The table is derived from each resource's own `Requirement`, through the same
/// [`Requirement::verdict`] the `sync` pre-flight consults, so `doctor` cannot
/// promise something `sync` then refuses — or the reverse.
fn capabilities(ctx: &Context, auth: Option<&AuthStatus>) -> Vec<(ResourceId, Capability)> {
    ctx.engine
        .registry()
        .all()
        .map(|resource| (resource.id(), resource.requirement().verdict(auth)))
        .collect()
}

/// The `gh` executable every probe here spawns.
fn gh_program() -> std::ffi::OsString {
    std::ffi::OsString::from("gh")
}

/// Introspect the current credential, degrading gracefully at every step.
pub(crate) async fn introspect(ctx: &Context) -> Option<AuthStatus> {
    let program = gh_program();
    let gh_auth = auth::gh_auth_status(&program).await.ok()?;
    let token = auth::gh_auth_token(&program).await;

    let mut status = match auth::introspect(ctx.client(), token.as_deref(), &gh_auth).await {
        Ok(status) => status,
        // Report what we do know rather than nothing: a failure here is usually
        // a network problem, not a reason to withhold the token type.
        Err(_) => AuthStatus {
            hostname: gh_auth.hostname.clone(),
            account: gh_auth.account.clone(),
            token_kind: token
                .as_deref()
                .map(|token| TokenKind::detect(token, auth::in_github_actions()))
                .unwrap_or(TokenKind::Unknown),
            scopes: Scopes::Unknown,
            admin_on_target: None,
        },
    };

    // Only probe when scopes are unavailable; a classic token already told us
    // everything and the extra request would be waste.
    if status.scopes == Scopes::Unknown {
        status.admin_on_target = auth::probe_admin(ctx.client(), ctx.target().ok()?).await;
    }

    Some(status)
}
