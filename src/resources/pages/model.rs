//! GitHub Pages model.
//!
//! # Absent is not null
//!
//! `cname` is nullable in the same sense as `repository.description`: omitting
//! the key leaves the custom domain alone, `cname: null` removes it. See
//! [`Nullable`].
//!
//! # Normalisation
//!
//! Two traps, each of which produces a permanent diff if missed:
//!
//! * GitHub stores the source path as exactly `/` or `/docs`. People write
//!   `docs`, `/docs` and `docs/`, so [`normalize_path`] collapses all of them.
//! * A custom domain is DNS, and DNS is case-insensitive. GitHub also reports an
//!   unset domain as `null` in some responses and `""` in others, so
//!   [`normalize_cname`] treats both as unset.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Finding;
use crate::resources::ValidateCtx;
use crate::resources::repository::model::{Nullable, double_option};

/// How the site is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BuildType {
    /// GitHub builds the site from a branch, with its own Jekyll pipeline.
    Legacy,
    /// The site is published by an Actions workflow.
    Workflow,
}

impl BuildType {
    /// The value as the API spells it.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Workflow => "workflow",
        }
    }
}

/// The branch and directory a `legacy` build publishes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Branch the site is published from, for example `gh-pages`.
    pub branch: String,

    /// Directory within the branch, either `/` or `/docs`.
    ///
    /// GitHub supports no other value. Written without the leading slash it is
    /// added for you, so `docs` and `/docs` mean the same thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Source {
    /// A normalised copy, safe to compare against a normalised counterpart.
    pub fn normalized(&self) -> Self {
        Self {
            branch: self.branch.trim().to_string(),
            path: Some(normalize_path(self.path.as_deref())),
        }
    }

    /// The request body fragment.
    pub fn as_body(&self) -> serde_json::Value {
        let normalized = self.normalized();
        serde_json::json!({
            "branch": normalized.branch,
            "path": normalized.path.unwrap_or_else(|| "/".into()),
        })
    }

    /// Rendered for a [`FieldDiff`](crate::resources::FieldDiff).
    pub fn label(&self) -> String {
        let normalized = self.normalized();
        format!(
            "{}:{}",
            normalized.branch,
            normalized.path.unwrap_or_else(|| "/".into())
        )
    }
}

/// The `pages` configuration section.
///
/// Declaring the section enables GitHub Pages if it is off. It never turns Pages
/// back off: an omitted section means *unmanaged*, so there is no way to express
/// "disabled", and destroying a published site is not something a missing key
/// should do (ADR-005).
///
/// ```yaml
/// pages:
///   build_type: workflow
///   cname: docs.example.com
///   https_enforced: true
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PagesSettings {
    /// How the site is built: from a branch (`legacy`) or by a workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_type: Option<BuildType>,

    /// Branch and directory to publish from.
    ///
    /// Only meaningful with `build_type: legacy`; GitHub rejects a source sent
    /// for a workflow-built site.
    ///
    /// ```yaml
    /// source:
    ///   branch: gh-pages
    ///   path: /docs
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,

    /// Custom domain for the site.
    ///
    /// Set to `null` to remove it and fall back to the `github.io` address.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub cname: Nullable<String>,

    /// Whether HTTP requests are redirected to HTTPS.
    ///
    /// GitHub refuses to enable this until it has provisioned a certificate for
    /// the custom domain, which can take a while after `cname` is first set. A
    /// `sync` that sets both at once may need running twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https_enforced: Option<bool>,
}

impl PagesSettings {
    /// Whether the section declares enough to *create* a site.
    ///
    /// `POST /pages` needs a build type or a source; a section carrying only a
    /// `cname` can update an existing site but cannot bring one into being.
    pub fn is_creatable(&self) -> bool {
        self.build_type.is_some() || self.source.is_some()
    }
}

/// Normalise a source path to the `/` or `/docs` form GitHub stores.
pub fn normalize_path(path: Option<&str>) -> String {
    let path = path.unwrap_or("/").trim().trim_matches('/');
    if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}")
    }
}

/// Normalise a custom domain, mapping the unset spellings to `None`.
pub fn normalize_cname(cname: Option<&str>) -> Option<String> {
    cname
        .map(str::trim)
        .filter(|cname| !cname.is_empty())
        .map(str::to_lowercase)
}

/// Validate the pages section.
pub fn validate(settings: &PagesSettings, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    if !settings.is_creatable() {
        findings.push(
            Finding::warning(
                "gh_settings::pages::no_source",
                "`pages` declares neither `build_type` nor `source`",
            )
            .at(ctx.key_span("pages"))
            .labelled("cannot enable a site")
            .help(
                "such a section can update a site that already exists, but GitHub needs a build \
                 type or a source to create one",
            ),
        );
    }

    // GitHub answers a source sent for a workflow-built site with a 422 that
    // does not name the offending field.
    if settings.build_type == Some(BuildType::Workflow) && settings.source.is_some() {
        findings.push(
            Finding::error(
                "gh_settings::pages::source_with_workflow",
                "`source` cannot be set when `build_type` is `workflow`",
            )
            .at(ctx.key_span("pages.source"))
            .labelled("not accepted for workflow builds")
            .help("remove `source`, or set `build_type: legacy` to publish from a branch"),
        );
    }

    if let Some(source) = &settings.source {
        if source.branch.trim().is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::pages::empty_branch",
                    "`source.branch` is empty",
                )
                .at(ctx.span("pages.source.branch"))
                .labelled("a branch name is required"),
            );
        }

        // `/` and `/docs` are the only directories GitHub accepts, and it
        // rejects anything else without saying which values are allowed.
        let path = normalize_path(source.path.as_deref());
        if !matches!(path.as_str(), "/" | "/docs") {
            findings.push(
                Finding::error(
                    "gh_settings::pages::invalid_path",
                    format!("`{path}` is not a valid source path"),
                )
                .at(ctx.span("pages.source.path"))
                .labelled("unsupported directory")
                .help("GitHub only publishes from `/` or `/docs`"),
            );
        }
    }

    if let Some(Some(cname)) = &settings.cname
        && let Some(normalized) = normalize_cname(Some(cname))
        && (normalized.contains('/') || normalized.contains(':'))
    {
        findings.push(
            Finding::error(
                "gh_settings::pages::invalid_cname",
                format!("`{cname}` is not a valid custom domain"),
            )
            .at(ctx.span("pages.cname"))
            .labelled("expected a bare hostname")
            .help("write `docs.example.com`, not a URL"),
        );
    }

    findings
}
