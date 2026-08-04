//! The synchronisation engine.
//!
//! The engine knows nothing about labels, topics or rulesets. It orchestrates
//! [`ErasedResource`]s: order them, plan them, render the plan, apply it. Adding
//! a GitHub feature never touches this module (ADR-001).

pub mod apply;
pub mod plan;
pub mod registry;

pub use apply::{ApplyOutcome, ApplyReport};
pub use plan::{Plan, PlanArtifact};
pub use registry::Registry;

use crate::config::{Config, Finding};
use crate::github::{GitHubClient, Result as GitHubResult, Target};
use crate::resources::{PruneOpts, ResourceId, ValidateCtx};

/// Everything a run needs.
pub struct Engine {
    /// The resources to orchestrate, in dependency order.
    registry: Registry,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Build an engine over the default registry.
    pub fn new() -> Self {
        Self {
            registry: Registry::default(),
        }
    }

    /// Build an engine over a specific registry, for tests.
    pub fn with_registry(registry: Registry) -> Self {
        Self { registry }
    }

    /// The registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Validate a configuration without touching the network.
    ///
    /// Collects *every* finding rather than stopping at the first, because
    /// fixing configuration one error per run is a miserable experience.
    pub fn validate(&self, config: &Config, only: &[ResourceId]) -> Vec<Finding> {
        let ctx = ValidateCtx::resolved(&config.spans, &config.provenance);
        let mut findings = config.settings.validate(&ctx);

        for resource in self.registry.selected(only) {
            findings.extend(resource.validate(&config.settings, &ctx));
        }

        // Deterministic order so snapshots are stable.
        findings.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .reverse()
                .then(a.code.cmp(&b.code))
        });
        findings
    }

    /// Compute the full plan.
    pub async fn plan(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        config: &Config,
        prune: &PruneOpts,
        only: &[ResourceId],
    ) -> GitHubResult<Plan> {
        let mut plan = Plan::new(target.clone());

        // Sequential rather than concurrent: `gh api` spawns a process per call,
        // and interleaving them makes failures much harder to attribute. The
        // transport is behind a port, so this can change without touching
        // resources if profiling ever justifies it.
        for resource in self.registry.selected(only) {
            let resource_plan = resource
                .plan(client, target, &config.settings, prune)
                .await?;
            plan.push(resource_plan);
        }

        Ok(plan)
    }

    /// Apply a plan.
    pub async fn apply(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        plan: &Plan,
        options: &apply::ApplyOptions,
    ) -> ApplyReport {
        apply::run(&self.registry, client, target, plan, options).await
    }

    /// Build a configuration document from the repository's current state.
    pub async fn export(
        &self,
        client: &dyn GitHubClient,
        target: &Target,
        only: &[ResourceId],
    ) -> GitHubResult<crate::config::Settings> {
        let mut document = serde_json::Map::new();
        document.insert(
            "version".into(),
            serde_json::json!(crate::config::settings::CURRENT_VERSION),
        );

        for resource in self.registry.selected(only) {
            if let Some(section) = resource.export(client, target).await? {
                document.insert(resource.id().as_str().to_string(), section);
            }
        }

        Ok(serde_json::from_value(serde_json::Value::Object(document))
            .unwrap_or_else(|error| panic!("export produced an invalid document: {error}")))
    }
}
