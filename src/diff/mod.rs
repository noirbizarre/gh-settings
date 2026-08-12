//! Generic diffing helpers.
//!
//! Nearly every resource is a keyed collection: labels by name, autolinks by
//! prefix, rulesets by name. [`diff_keyed`] captures that shape once so each
//! resource only has to say what "the same item" and "changed" mean.

use std::collections::BTreeMap;
use std::hash::Hash;

/// The outcome of comparing two keyed collections.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyedDiff<K, D, C> {
    /// Present in the desired state only.
    pub created: Vec<(K, D)>,
    /// Present in both. Whether they actually differ is up to the caller.
    pub matched: Vec<(K, D, C)>,
    /// Present in the current state only.
    pub deleted: Vec<(K, C)>,
}

/// Compare two keyed collections.
///
/// Order is deterministic (keys are sorted) so plan output and snapshots are
/// stable regardless of the order GitHub happens to return items in.
pub fn diff_keyed<K, D, C>(
    desired: impl IntoIterator<Item = (K, D)>,
    current: impl IntoIterator<Item = (K, C)>,
) -> KeyedDiff<K, D, C>
where
    K: Ord + Hash + Clone,
{
    let mut desired: BTreeMap<K, D> = desired.into_iter().collect();
    let current: BTreeMap<K, C> = current.into_iter().collect();

    let mut created = Vec::new();
    let mut matched = Vec::new();
    let mut deleted = Vec::new();

    for (key, current_value) in current {
        match desired.remove(&key) {
            Some(desired_value) => matched.push((key, desired_value, current_value)),
            None => deleted.push((key, current_value)),
        }
    }

    // Whatever is left in `desired` was not present in `current`.
    created.extend(desired);

    KeyedDiff {
        created,
        matched,
        deleted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn partitions_into_created_matched_and_deleted() {
        let diff = diff_keyed(vec![("a", 1), ("b", 2)], vec![("b", 20), ("c", 30)]);
        assert_eq!(diff.created, vec![("a", 1)]);
        assert_eq!(diff.matched, vec![("b", 2, 20)]);
        assert_eq!(diff.deleted, vec![("c", 30)]);
    }

    #[test]
    fn output_order_is_deterministic() {
        let diff = diff_keyed(
            vec![("z", 1), ("a", 2), ("m", 3)],
            Vec::<(&str, i32)>::new(),
        );
        let keys: Vec<_> = diff.created.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
    }

    #[test]
    fn empty_inputs_produce_an_empty_diff() {
        let diff = diff_keyed(Vec::<(&str, i32)>::new(), Vec::<(&str, i32)>::new());
        assert!(diff.created.is_empty());
        assert!(diff.matched.is_empty());
        assert!(diff.deleted.is_empty());
    }
}
