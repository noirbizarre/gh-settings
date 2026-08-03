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
use crate::output::human::Capability;
use crate::resources::ResourceId;

/// Arguments for `doctor`.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Exit non-zero when any resource is unmanageable.
    #[arg(long)]
    pub strict: bool,
}

/// Run the command.
pub async fn run(args: &Args, ctx: &Context) -> Result<i32> {
    let gh_version = gh_version().await;

    let auth_status = if gh_version.is_some() {
        introspect(ctx).await
    } else {
        None
    };

    let capabilities = capabilities(ctx, auth_status.as_ref());

    if ctx.args.is_json() {
        println!(
            "{}",
            ctx.json
                .doctor(gh_version.as_deref(), auth_status.as_ref(), &capabilities)
        );
    } else {
        println!(
            "{}",
            ctx.human
                .doctor(gh_version.as_deref(), auth_status.as_ref(), &capabilities)
        );
    }

    let blocked = gh_version.is_none()
        || auth_status.is_none()
        || (args.strict
            && capabilities
                .iter()
                .any(|(_, capability)| !matches!(capability, Capability::Manageable)));

    Ok(if blocked {
        exit::FAILURE
    } else {
        exit::SUCCESS
    })
}

/// Work out what each resource can and cannot do with the current credential.
///
/// The table is derived from each resource's own `Requirement`, so it cannot
/// drift from the documentation or the pre-flight check.
fn capabilities(ctx: &Context, auth: Option<&AuthStatus>) -> Vec<(ResourceId, Capability)> {
    ctx.engine
        .registry()
        .all()
        .map(|resource| {
            let requirement = resource.requirement();
            let capability = match auth {
                None => Capability::Unknown,

                // The one case we can state with certainty. The workflow
                // `permissions:` block has no `administration` key, so this is
                // not a scope the user forgot to grant.
                Some(auth)
                    if auth.token_kind == TokenKind::ActionsGitHubToken
                        && !requirement.github_token_capable =>
                {
                    Capability::Impossible(
                        requirement
                            .github_token_note
                            .unwrap_or("not available to GITHUB_TOKEN"),
                    )
                }

                Some(auth) if auth.token_kind == TokenKind::ActionsGitHubToken => {
                    Capability::Manageable
                }

                // Classic tokens advertise their scopes, so we can be exact.
                Some(auth) => match requirement
                    .classic
                    .iter()
                    .map(|scope| auth.scopes.grants(scope))
                    .collect::<Option<Vec<bool>>>()
                {
                    Some(granted) if granted.iter().all(|granted| *granted) => {
                        Capability::Manageable
                    }
                    Some(_) => Capability::Impossible("missing the `repo` scope"),
                    // Fine-grained and App tokens do not report scopes. Saying
                    // "unknown" is more honest than guessing, and `sync` will
                    // still try.
                    None => match auth.admin_on_target {
                        Some(true) => Capability::Manageable,
                        Some(false) if !requirement.github_token_capable => Capability::Impossible(
                            "the token has no admin rights on this repository",
                        ),
                        _ => Capability::Unknown,
                    },
                },
            };
            (resource.id(), capability)
        })
        .collect()
}

/// Introspect the current credential, degrading gracefully at every step.
async fn introspect(ctx: &Context) -> Option<AuthStatus> {
    let program = std::ffi::OsString::from("gh");
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
        status.admin_on_target = auth::probe_admin(ctx.client(), &ctx.target).await;
    }

    Some(status)
}

/// Read the installed `gh` version.
async fn gh_version() -> Option<String> {
    let output = tokio::process::Command::new("gh")
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}
