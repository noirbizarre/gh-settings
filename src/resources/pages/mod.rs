//! GitHub Pages.
//!
//! A singleton resource, like `repository`, but with one thing none of the others
//! have: the underlying object may not exist at all. `GET /repos/{o}/{r}/pages`
//! answers `404` when Pages is off, which is why [`Current`] is an `Option` — an
//! absent site is a state to be created, not an error.
//!
//! # Two calls to enable
//!
//! `POST /pages` accepts only `build_type` and `source`. `cname` and
//! `https_enforced` have to follow in a `PUT`, so a creation carries both bodies
//! and [`apply`](Resource::apply) issues them in order.
//!
//! # What is deliberately not managed
//!
//! The site's `public` flag is reported by `GET /pages` but is not a body
//! parameter of either `POST` or `PUT`. Accepting it in the configuration would
//! publish a setting that is silently ignored, so it is not offered at all.
//!
//! # This resource never deletes
//!
//! There is no `DELETE /pages` here and `prune` is ignored. An omitted section
//! means unmanaged, so "disabled" is not expressible in the file, and inferring
//! it from absence would let a partial configuration take a published site down.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::{Finding, Settings};
use crate::github::{GitHubClient, GitHubClientExt, Request, Result as GitHubResult, Target};
use crate::resources::{
    Change, FieldDiff, Op, PruneOpts, Requirement, Resource, ResourceId, ValidateCtx,
};

pub mod model;

pub use model::{BuildType, PagesSettings, Source};

/// The `pages` resource.
#[derive(Debug, Default, Clone, Copy)]
pub struct Pages;

/// A source as the API reports it.
///
/// Separate from the configuration [`Source`], which rejects unknown fields:
/// a field GitHub adds to this response must not turn every read into an error.
#[derive(Debug, Clone, Default, Deserialize)]
struct SourceState {
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl SourceState {
    /// The comparable form, or `None` when GitHub named no branch.
    fn as_source(&self) -> Option<Source> {
        let branch = self.branch.as_deref()?;
        Some(
            Source {
                branch: branch.to_string(),
                path: self.path.clone(),
            }
            .normalized(),
        )
    }
}

/// A Pages site as the API reports it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PagesState {
    /// How the site is built.
    #[serde(default)]
    pub build_type: Option<String>,
    /// Branch and directory, absent for workflow-built sites.
    #[serde(default)]
    source: Option<SourceState>,
    /// Custom domain, `null` or `""` when unset.
    #[serde(default)]
    pub cname: Option<String>,
    /// Whether HTTPS is enforced.
    #[serde(default)]
    pub https_enforced: Option<bool>,
}

impl PagesState {
    /// The published source, normalised.
    pub fn source(&self) -> Option<Source> {
        self.source.as_ref().and_then(SourceState::as_source)
    }

    /// A normalised copy, safe to compare against a normalised counterpart.
    pub fn normalized(mut self) -> Self {
        self.build_type = self
            .build_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        self.cname = model::normalize_cname(self.cname.as_deref());
        self
    }
}

/// Current state: `None` when Pages is not enabled on the repository.
pub type Current = Option<PagesState>;

/// Payload of a pages change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    /// A `POST /pages` body, plus the follow-up `PUT` for what `POST` cannot
    /// carry. The second is `None` when there is nothing left to say.
    Create {
        /// The `POST` body.
        create: Value,
        /// The `PUT` body, when one is needed.
        update: Option<Value>,
    },
    /// A `PUT /pages` body.
    Update(Value),
}

#[async_trait]
impl Resource for Pages {
    type Desired = PagesSettings;
    type Current = Current;

    fn id(&self) -> ResourceId {
        ResourceId::Pages
    }

    fn requirement(&self) -> &'static Requirement {
        &Requirement::PAGES
    }

    fn depends_on(&self) -> &'static [ResourceId] {
        // Whether Pages is available at all depends on repository visibility, so
        // a run that makes a repository public must do so before enabling a site.
        &[ResourceId::Repository]
    }

    fn desired(&self, settings: &Settings) -> Option<Self::Desired> {
        settings.pages.clone()
    }

    fn validate(&self, desired: &Self::Desired, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
        model::validate(desired, ctx)
    }

    async fn current(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Self::Current> {
        let current: Option<PagesState> = client
            .send_optional(Request::get(target.endpoint("pages")))
            .await?;
        Ok(current.map(PagesState::normalized))
    }

    fn diff(
        &self,
        desired: &Self::Desired,
        current: &Self::Current,
        // Unused on purpose: this resource has no delete path. See the module
        // documentation.
        _prune: &PruneOpts,
    ) -> Vec<Change> {
        match current {
            Some(current) => self.update(desired, current),
            None => self.create(desired),
        }
    }

    async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        change: &Change,
    ) -> GitHubResult<()> {
        let payload: Payload = change
            .decode()
            .unwrap_or_else(|error| panic!("pages change carried a bad payload: {error}"));

        let endpoint = target.endpoint("pages");
        match payload {
            Payload::Create { create, update } => {
                client
                    .execute(Request::post(endpoint.clone(), create))
                    .await?;
                // The settings `POST` would not accept, applied to the site it
                // has just brought into existence.
                if let Some(update) = update {
                    client.execute(Request::put(endpoint, update)).await?;
                }
                Ok(())
            }
            Payload::Update(body) => client.execute(Request::put(endpoint, body)).await,
        }
    }

    async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
    ) -> GitHubResult<Option<Value>> {
        // No site means no section: an exported file should not carry a `pages`
        // block for a repository that does not publish one.
        let Some(current) = self.current(client, target).await? else {
            return Ok(None);
        };

        let settings = PagesSettings {
            build_type: current.build_type.as_deref().and_then(parse_build_type),
            source: current.source(),
            // Only exported when a domain is actually set. Writing `cname: null`
            // for a site that never had one turns a description of the current
            // state into an instruction to clear something.
            cname: current.cname.clone().map(Some),
            https_enforced: current.https_enforced,
        };

        Ok(Some(serde_json::to_value(settings).unwrap_or(Value::Null)))
    }
}

impl Pages {
    /// Changes needed when the repository has no site yet.
    fn create(&self, desired: &PagesSettings) -> Vec<Change> {
        // Nothing creatable was declared. `validate` has already warned about
        // this; emitting a change we know GitHub would reject helps nobody.
        if !desired.is_creatable() {
            return Vec::new();
        }

        let mut create = Map::new();
        let mut update = Map::new();
        let mut fields = Vec::new();

        if let Some(build_type) = desired.build_type {
            create.insert("build_type".into(), json!(build_type.as_str()));
            fields.push(FieldDiff::added("build_type", build_type.as_str()));
        }

        if let Some(source) = &desired.source {
            create.insert("source".into(), source.as_body());
            fields.push(FieldDiff::added("source", source.label()));
        }

        if let Some(cname) = desired.cname.as_ref().and_then(Option::as_ref)
            && let Some(cname) = model::normalize_cname(Some(cname))
        {
            update.insert("cname".into(), json!(cname));
            fields.push(FieldDiff::added("cname", cname));
        }

        if let Some(https_enforced) = desired.https_enforced {
            update.insert("https_enforced".into(), json!(https_enforced));
            fields.push(FieldDiff::added(
                "https_enforced",
                https_enforced.to_string(),
            ));
        }

        vec![
            Change::new(ResourceId::Pages, Op::Create, "site")
                .summary("enable GitHub Pages")
                .fields(fields)
                .payload(Payload::Create {
                    create: Value::Object(create),
                    update: (!update.is_empty()).then_some(Value::Object(update)),
                }),
        ]
    }

    /// Changes needed when a site already exists.
    fn update(&self, desired: &PagesSettings, current: &PagesState) -> Vec<Change> {
        let mut body = Map::new();
        let mut fields = Vec::new();

        if let Some(build_type) = desired.build_type
            && current.build_type.as_deref() != Some(build_type.as_str())
        {
            body.insert("build_type".into(), json!(build_type.as_str()));
            fields.push(FieldDiff::changed(
                "build_type",
                current
                    .build_type
                    .clone()
                    .unwrap_or_else(|| "(unknown)".into()),
                build_type.as_str(),
            ));
        }

        if let Some(source) = &desired.source {
            let source = source.normalized();
            let existing = current.source();
            if existing.as_ref() != Some(&source) {
                body.insert("source".into(), source.as_body());
                fields.push(FieldDiff::changed(
                    "source",
                    existing
                        .as_ref()
                        .map(Source::label)
                        .unwrap_or_else(|| "(none)".into()),
                    source.label(),
                ));
            }
        }

        if let Some(cname) = &desired.cname {
            let cname = model::normalize_cname(cname.as_deref());
            if cname != current.cname {
                // An explicit `null` clears the domain, which is the whole point
                // of the double option.
                body.insert("cname".into(), json!(cname));
                fields.push(FieldDiff::changed(
                    "cname",
                    current.cname.clone().unwrap_or_else(|| "(none)".into()),
                    cname.unwrap_or_else(|| "(none)".into()),
                ));
            }
        }

        if let Some(https_enforced) = desired.https_enforced
            && current.https_enforced != Some(https_enforced)
        {
            body.insert("https_enforced".into(), json!(https_enforced));
            fields.push(FieldDiff::changed(
                "https_enforced",
                render_bool(current.https_enforced),
                https_enforced.to_string(),
            ));
        }

        if body.is_empty() {
            return Vec::new();
        }

        // Nothing is added to the body here. An earlier version sent the current
        // `build_type` alongside a moved `source`, on the assumption that GitHub
        // rejects one without the other. It does not — `PUT` documents both as
        // independently optional — and the assumption was actively harmful: on a
        // workflow-built site it produced `build_type: workflow` *with* a source,
        // which is the one combination GitHub really does refuse.

        let summary = if fields.len() == 1 {
            format!("update pages {}", fields[0].field)
        } else {
            format!("update pages ({} fields)", fields.len())
        };

        vec![
            Change::new(ResourceId::Pages, Op::Update, "settings")
                .summary(summary)
                .fields(fields)
                .payload(Payload::Update(Value::Object(body))),
        ]
    }
}

/// Render a tri-state boolean the API may not have reported at all.
fn render_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "(not reported)".into())
}

/// Parse the API's build type spelling back into a configuration enum.
fn parse_build_type(value: &str) -> Option<BuildType> {
    match value {
        "legacy" => Some(BuildType::Legacy),
        "workflow" => Some(BuildType::Workflow),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
