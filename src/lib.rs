//! Declarative GitHub repository settings for the GitHub CLI.
//!
//! `gh-settings` makes a repository's configuration behave like infrastructure as
//! code: a single `.github/settings.yml` describes the desired state, and the
//! tool computes and applies the difference. It requires no GitHub App and no
//! central service — only the GitHub CLI's authentication.
//!
//! # Architecture
//!
//! The crate is organised in rings, with dependencies pointing inward:
//!
//! * [`config`] parses and validates the configuration file;
//! * [`resources`] implements one GitHub feature per module, behind a common
//!   [`Resource`](resources::Resource) trait;
//! * [`engine`] orchestrates resources: order, plan, render, apply;
//! * [`github`] is the only place that talks to GitHub, behind the
//!   [`GitHubClient`](github::GitHubClient) port;
//! * [`output`] renders plans and reports.
//!
//! Adding support for a new GitHub setting means writing one module under
//! [`resources`] and adding one line to the registry.
#![warn(missing_docs)]
#![warn(clippy::all)]
// `ConfigError` and `GitHubError` deliberately carry their diagnostic payload —
// the source file, the span, the response body — because that is what makes the
// error messages good. Boxing them to shrink the `Result` would trade a real
// user-facing benefit for a micro-optimisation on a path that runs a handful of
// times per process.
#![allow(clippy::result_large_err)]

pub mod cli;
pub mod config;
pub mod diff;
pub mod engine;
pub mod github;
pub mod output;
pub mod resources;
pub mod schema;
