//! Shared command context.
//!
//! Resolving the target repository, loading the configuration and building the
//! client are the same in every command, so they live here rather than being
//! repeated five times.

use std::path::PathBuf;
use std::sync::Arc;

use miette::{Diagnostic, Result};

use crate::cli::GlobalArgs;
use crate::config::{Config, ConfigError, discover};
use crate::engine::Engine;
use crate::github::{GhCliTransport, GitHubClient, Target};
use crate::output::{HumanRenderer, JsonRenderer, Theme};

/// Everything a command needs.
pub struct Context {
    /// Parsed global options.
    pub args: GlobalArgs,
    /// The repository being acted upon, when the command needs one.
    ///
    /// `None` for `validate`, which reads a file and says whether it is
    /// well-formed. Nothing about that question involves a repository, and
    /// demanding one is what stopped it running on a fork's pull request or in
    /// a bare container. Use [`Context::target`] to reach it.
    target: Option<Target>,
    /// The GitHub client.
    pub client: Arc<dyn GitHubClient>,
    /// The resource engine.
    pub engine: Engine,
    /// Human renderer.
    pub human: HumanRenderer,
    /// JSON renderer.
    pub json: JsonRenderer,
}

/// Failures while assembling a context.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ContextError {
    /// The repository could not be determined.
    #[error("could not determine which repository to act on")]
    #[diagnostic(
        code(gh_settings::context::no_target),
        help("pass `-R owner/repo`, or run from inside a git repository with a GitHub remote")
    )]
    NoTarget,

    /// Loading the configuration failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Config(#[from] ConfigError),
}

impl Context {
    /// Build a context, resolving the repository but not loading configuration.
    ///
    /// `export` and `doctor` need a repository but no configuration file, so this
    /// is deliberately separate from [`Self::load_config`].
    pub async fn new(args: GlobalArgs, read_only: bool) -> Result<Self, ContextError> {
        let target = resolve_target(&args).await?;
        Ok(Self::build(args, read_only, Some(target)))
    }

    /// Build a context for a command that acts on no repository.
    ///
    /// Only `validate` qualifies. `extends` names its base absolutely, as
    /// `owner/repo@ref`, so even an inheriting configuration never needs to know
    /// which repository it would be applied to.
    pub fn without_target(args: GlobalArgs) -> Self {
        Self::build(args, true, None)
    }

    /// Assemble the parts every context shares.
    fn build(args: GlobalArgs, read_only: bool, target: Option<Target>) -> Self {
        let theme = Theme::from_flag(args.color_override());
        let client = Arc::new(GhCliTransport::new().read_only(read_only));

        Self {
            human: HumanRenderer::new(theme, args.verbose),
            json: JsonRenderer,
            engine: Engine::new(),
            client,
            target,
            args,
        }
    }

    /// The repository being acted upon.
    ///
    /// Errors rather than panics: the only context without one is `validate`'s,
    /// and a future command reaching for a target it never asked for should say
    /// what a user can do about it.
    pub fn target(&self) -> Result<&Target, ContextError> {
        self.target.as_ref().ok_or(ContextError::NoTarget)
    }

    /// Load and parse the configuration file, resolving anything it inherits.
    ///
    /// Async because a configuration may inherit from another repository, which
    /// has to be fetched. Nothing is fetched unless the file declares `extends`,
    /// so a configuration that does not inherit is still loaded without
    /// touching the network.
    pub async fn load_config(&self) -> Result<Config, ContextError> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let source = discover(&cwd, self.args.config.as_deref())?;
        let path = source.path().to_path_buf();

        let contents = std::fs::read_to_string(&path).map_err(|error| ConfigError::Unreadable {
            path: path.display().to_string(),
            source: error,
        })?;

        let loader = crate::github::GitHubBaseLoader::new(self.client());
        Ok(crate::config::load(&path, &contents, &loader).await?)
    }

    /// The client as a trait object reference.
    pub fn client(&self) -> &dyn GitHubClient {
        self.client.as_ref()
    }
}

/// Determine the repository to act on.
///
/// An explicit `-R` always wins; otherwise the git remotes are consulted, exactly
/// as the GitHub CLI does.
async fn resolve_target(args: &GlobalArgs) -> Result<Target, ContextError> {
    if let Some(target) = &args.repo {
        return Ok(target.clone());
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let remotes = crate::config::discover::git_remotes(&cwd).await;

    crate::config::discover::infer_target(&remotes).map_err(|_| ContextError::NoTarget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;

    fn args(repo: Option<Target>) -> GlobalArgs {
        GlobalArgs {
            repo,
            config: None,
            only: Vec::new(),
            format: Format::Text,
            verbose: false,
            color: None,
            debug: 0,
        }
    }

    #[tokio::test]
    async fn an_explicit_repository_wins() {
        let target = resolve_target(&args(Some(Target::new("o", "r"))))
            .await
            .unwrap();
        assert_eq!(target, Target::new("o", "r"));
    }
}
