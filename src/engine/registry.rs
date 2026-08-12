//! The resource registry.
//!
//! Holds every resource and guarantees a dependency-respecting order.
//! `variables` depends on `environments`, because a variable cannot be written
//! into an environment that does not exist yet; rulesets may come to reference
//! custom properties the same way.

use crate::resources::{
    ErasedResource, ResourceId, autolinks::Autolinks, environments::Environments, labels::Labels,
    repository::Repository, rulesets::Rulesets, topics::Topics, variables::Variables,
};

/// The ordered set of resources the engine orchestrates.
pub struct Registry {
    resources: Vec<Box<dyn ErasedResource>>,
}

impl Default for Registry {
    /// Every resource this build supports.
    ///
    /// Adding a GitHub feature means adding one line here and nothing else.
    fn default() -> Self {
        Self::new(vec![
            Box::new(Repository),
            Box::new(Topics),
            Box::new(Labels),
            Box::new(Autolinks),
            Box::new(Rulesets),
            Box::new(Environments),
            Box::new(Variables),
        ])
    }
}

impl Registry {
    /// Build a registry, sorting the resources into dependency order.
    ///
    /// # Panics
    ///
    /// If the declared dependencies contain a cycle. That is a programming error
    /// in this crate, detectable by the test suite, never by a user's input.
    pub fn new(resources: Vec<Box<dyn ErasedResource>>) -> Self {
        let resources = topological_sort(resources);
        Self { resources }
    }

    /// Every resource, in application order.
    pub fn all(&self) -> impl Iterator<Item = &dyn ErasedResource> {
        self.resources.iter().map(AsRef::as_ref)
    }

    /// The resources selected by `--only`, in application order.
    ///
    /// An empty selection means "everything", so the common case needs no flag.
    pub fn selected<'a>(
        &'a self,
        only: &'a [ResourceId],
    ) -> impl Iterator<Item = &'a dyn ErasedResource> {
        self.all()
            .filter(move |resource| only.is_empty() || only.contains(&resource.id()))
    }

    /// Look up a resource by identifier.
    pub fn get(&self, id: ResourceId) -> Option<&dyn ErasedResource> {
        self.all().find(|resource| resource.id() == id)
    }

    /// Number of registered resources.
    pub fn len(&self) -> usize {
        self.resources.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }
}

/// Order resources so that every dependency precedes its dependants.
///
/// Ties are broken by declaration order, which keeps output stable and makes the
/// registry read top to bottom the way it executes.
fn topological_sort(resources: Vec<Box<dyn ErasedResource>>) -> Vec<Box<dyn ErasedResource>> {
    let mut remaining: Vec<Option<Box<dyn ErasedResource>>> =
        resources.into_iter().map(Some).collect();
    let mut placed: Vec<ResourceId> = Vec::new();
    let mut ordered: Vec<Box<dyn ErasedResource>> = Vec::new();

    while ordered.len() < remaining.len() {
        let mut progressed = false;

        for slot in remaining.iter_mut() {
            let ready = match slot.as_deref() {
                Some(resource) => resource
                    .depends_on()
                    .iter()
                    .all(|dependency| placed.contains(dependency)),
                None => false,
            };

            if ready {
                let resource = slot.take().expect("checked above");
                placed.push(resource.id());
                ordered.push(resource);
                progressed = true;
            }
        }

        if !progressed {
            let unresolved: Vec<ResourceId> = remaining
                .iter()
                .filter_map(|slot| slot.as_deref().map(ErasedResource::id))
                .collect();
            panic!(
                "resource dependencies contain a cycle or reference a resource that is not registered: {unresolved:?}"
            );
        }
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn ids(registry: &Registry) -> Vec<ResourceId> {
        registry.all().map(ErasedResource::id).collect()
    }

    #[test]
    fn the_default_registry_holds_every_resource() {
        let registry = Registry::default();
        assert_eq!(registry.len(), ResourceId::ALL.len());
        for id in ResourceId::ALL {
            assert!(registry.get(*id).is_some(), "{id} is not registered");
        }
    }

    #[test]
    fn declaration_order_is_preserved_when_nothing_depends_on_anything() {
        assert_eq!(
            ids(&Registry::default()),
            vec![
                ResourceId::Repository,
                ResourceId::Topics,
                ResourceId::Labels,
                ResourceId::Autolinks,
                ResourceId::Rulesets,
                ResourceId::Environments,
                ResourceId::Variables,
            ]
        );
    }

    #[test]
    fn a_dependency_is_ordered_before_the_resource_that_declares_it() {
        // Declared the wrong way round on purpose: a variable cannot be written
        // into an environment that does not exist yet, so the sort has to move
        // `environments` in front regardless of how the registry was written.
        let registry = Registry::new(vec![
            Box::new(crate::resources::variables::Variables),
            Box::new(crate::resources::environments::Environments),
        ]);

        assert_eq!(
            ids(&registry),
            vec![ResourceId::Environments, ResourceId::Variables]
        );
    }

    #[test]
    fn an_empty_selection_means_everything() {
        let registry = Registry::default();
        assert_eq!(registry.selected(&[]).count(), registry.len());
    }

    #[test]
    fn selection_filters_and_keeps_order() {
        let registry = Registry::default();
        let selected: Vec<ResourceId> = registry
            .selected(&[ResourceId::Labels, ResourceId::Repository])
            .map(ErasedResource::id)
            .collect();
        assert_eq!(selected, vec![ResourceId::Repository, ResourceId::Labels]);
    }

    #[test]
    fn every_resource_declares_a_requirement() {
        // A resource without a requirement would silently vanish from the doctor
        // table and the generated docs.
        for resource in Registry::default().all() {
            let requirement = resource.requirement();
            assert!(
                !requirement.fine_grained.is_empty(),
                "{} declares no fine-grained permissions",
                resource.id()
            );
            assert!(
                !requirement.classic.is_empty(),
                "{} declares no classic scopes",
                resource.id()
            );
        }
    }

    #[test]
    fn only_labels_are_reachable_with_the_actions_token() {
        // Pins the claim made in the documentation and by `doctor`. If GitHub
        // ever adds an `administration` permission to GITHUB_TOKEN, this test
        // fails and the docs get revisited.
        for resource in Registry::default().all() {
            let capable = resource.requirement().github_token_capable;
            let expected = resource.id() == ResourceId::Labels;
            assert_eq!(capable, expected, "{} capability changed", resource.id());
        }
    }

    #[test]
    fn dependencies_only_reference_registered_resources() {
        let registry = Registry::default();
        for resource in registry.all() {
            for dependency in resource.depends_on() {
                assert!(
                    registry.get(*dependency).is_some(),
                    "{} depends on unregistered {dependency}",
                    resource.id()
                );
            }
        }
    }
}
