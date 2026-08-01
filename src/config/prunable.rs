//! The `list | { prune, items }` section shape.
//!
//! Collection sections accept two forms:
//!
//! ```yaml
//! labels:                 # bare list: additive, nothing is ever deleted
//!   - name: bug
//!
//! labels:                 # object form: opts in to deleting unmanaged labels
//!   prune: true
//!   items:
//!     - name: bug
//! ```
//!
//! The bare list is the default because pruning is opt-in (ADR-005). Keeping both
//! forms means the common case stays terse while the destructive case is
//! impossible to enable by accident.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A collection section that can opt into pruning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
// Without this, the derive infers a `T: Default` bound from the defaulted
// `items` field, which would force every item type to implement `Default`.
#[serde(bound(deserialize = "T: Deserialize<'de>", serialize = "T: Serialize"))]
pub enum Prunable<T> {
    /// Bare list form. Additive: unmanaged items are left alone.
    List(Vec<T>),
    /// Object form, able to enable pruning.
    Managed {
        /// Delete items that exist on GitHub but are absent here.
        ///
        /// Defaults to `false`. Overridden by `--prune` / `--no-prune`.
        #[serde(default)]
        prune: bool,
        /// The declared items.
        ///
        /// Modelled as an `Option` rather than a defaulted `Vec` so the section
        /// does not require its item type to implement `Default`.
        #[serde(default)]
        items: Option<Vec<T>>,
    },
}

impl<T> Prunable<T> {
    /// The declared items.
    pub fn items(&self) -> &[T] {
        match self {
            Self::List(items) => items,
            Self::Managed { items, .. } => items.as_deref().unwrap_or(&[]),
        }
    }

    /// Whether unmanaged items should be deleted.
    pub fn prune(&self) -> bool {
        match self {
            Self::List(_) => false,
            Self::Managed { prune, .. } => *prune,
        }
    }

    /// Whether the section declares nothing.
    ///
    /// Note that an empty section is still *managed*: with pruning enabled it
    /// means "there should be none of these", which is a meaningful instruction.
    pub fn is_empty(&self) -> bool {
        self.items().is_empty()
    }
}

impl<T> Default for Prunable<T> {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

impl<T> From<Vec<T>> for Prunable<T> {
    fn from(items: Vec<T>) -> Self {
        Self::List(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_bare_list_form_never_prunes() {
        let section: Prunable<String> = serde_yaml_ng::from_str("- a\n- b\n").unwrap();
        assert_eq!(section.items(), ["a", "b"]);
        assert!(!section.prune());
    }

    #[test]
    fn the_object_form_defaults_to_not_pruning() {
        let section: Prunable<String> = serde_yaml_ng::from_str("items:\n  - a\n").unwrap();
        assert!(!section.prune(), "prune must be opt-in even in object form");
    }

    #[test]
    fn the_object_form_can_enable_pruning() {
        let section: Prunable<String> =
            serde_yaml_ng::from_str("prune: true\nitems:\n  - a\n").unwrap();
        assert!(section.prune());
    }

    #[test]
    fn an_object_without_items_is_empty_but_managed() {
        let section: Prunable<String> = serde_yaml_ng::from_str("prune: true\n").unwrap();
        assert!(section.is_empty());
        assert!(section.prune());
    }

    #[test]
    fn round_trips_through_yaml() {
        let section = Prunable::Managed {
            prune: true,
            items: Some(vec!["a".to_string()]),
        };
        let yaml = serde_yaml_ng::to_string(&section).unwrap();
        let parsed: Prunable<String> = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, section);
    }
}
