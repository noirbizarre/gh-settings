//! Merging an inherited configuration with a local one.
//!
//! Two rules, and everything else follows from them.
//!
//! **A field the child declares wins outright.** Not field by field within an
//! item — whole item by identity. `Label::color` defaults to `ededed` and
//! `Ruleset::target`/`enforcement` default to `branch`/`active`, and none of
//! them is an `Option`, so once a document is parsed there is no way to tell a
//! field the user left out from one they wrote the default into. A field-wise
//! merge would silently repaint an inherited label grey the moment a child
//! mentioned it.
//!
//! **`prune` never inherits** (ADR-005). A shared base file must not be able to
//! start deleting things across every repository that extends it, at the
//! decision of someone who does not own them.
//!
//! The merge also produces the [`Provenance`] that says which document each
//! merged path came from. Positional paths do not survive a merge — base
//! `[a, b, c]` and child `[b', d]` give `[a, b', c, d]`, in which `labels.1` is
//! the child's item zero — so the map is the only thing that can answer where a
//! finding belongs.

use super::provenance::Provenance;
use super::settings::Settings;
use super::source::SourceId;
use super::spans::SpanIndex;
use crate::config::Prunable;
use crate::resources::labels::model::key as label_key;
use crate::resources::repository::{RepositorySettings, SecuritySettings};

/// One document taking part in a merge.
pub struct Layer<'a> {
    /// Which document.
    pub id: SourceId,
    /// Its settings, already canonicalised.
    pub settings: &'a Settings,
    /// Its span index, needed to tell the two `Prunable` forms apart.
    pub spans: &'a SpanIndex,
}

/// Merge an inherited configuration with the one that extends it.
///
/// `base` is the inherited document, `child` the local one. Returns the
/// effective settings and the provenance of every path in them.
pub fn merge(base: &Layer<'_>, child: &Layer<'_>) -> (Settings, Provenance) {
    let mut provenance = Provenance::merged();

    let version = pick(
        &mut provenance,
        "version",
        base.id,
        base.settings.version.as_ref(),
        child.id,
        child.settings.version.as_ref(),
    );

    let repository = merge_repository(&mut provenance, base, child);

    let topics = merge_section(
        &mut provenance,
        "topics",
        base,
        child,
        |settings| settings.topics.as_ref(),
        |topic| crate::resources::topics::normalize(topic),
    );
    let labels = merge_section(
        &mut provenance,
        "labels",
        base,
        child,
        |settings| settings.labels.as_ref(),
        |label| label_key(&label.name),
    );
    let autolinks = merge_section(
        &mut provenance,
        "autolinks",
        base,
        child,
        |settings| settings.autolinks.as_ref(),
        |autolink| autolink.key_prefix.trim().to_string(),
    );
    let rulesets = merge_section(
        &mut provenance,
        "rulesets",
        base,
        child,
        |settings| settings.rulesets.as_ref(),
        |ruleset| ruleset.name.trim().to_string(),
    );

    let settings = Settings {
        version,
        // Consumed by the loader. Carrying it into the merged result would
        // make an already-resolved configuration look like it still had work
        // to do, and would be re-emitted by `export`.
        extends: None,
        repository,
        topics,
        labels,
        autolinks,
        rulesets,
    };

    (settings, provenance)
}

/// Take the child's value when it has one, recording where it came from.
fn pick<T: Clone>(
    provenance: &mut Provenance,
    path: &str,
    base_id: SourceId,
    base: Option<&T>,
    child_id: SourceId,
    child: Option<&T>,
) -> Option<T> {
    match (base, child) {
        (_, Some(value)) => {
            provenance.record(path, child_id, path);
            Some(value.clone())
        }
        (Some(value), None) => {
            provenance.record(path, base_id, path);
            Some(value.clone())
        }
        (None, None) => None,
    }
}

/// Merge the repository section, field by field.
///
/// Field-wise is right *here* — unlike collections — because every field is an
/// `Option`, so "absent" and "set to the default" are genuinely distinguishable.
fn merge_repository(
    provenance: &mut Provenance,
    base: &Layer<'_>,
    child: &Layer<'_>,
) -> Option<RepositorySettings> {
    let base_repository = base.settings.repository.as_ref();
    let child_repository = child.settings.repository.as_ref();

    if base_repository.is_none() && child_repository.is_none() {
        return None;
    }

    let empty = RepositorySettings::default();
    let b = base_repository.unwrap_or(&empty);
    let c = child_repository.unwrap_or(&empty);

    // Destructured exhaustively, with no `..`: a field added later is a compile
    // error here rather than a setting the merge silently stops managing.
    let RepositorySettings {
        description,
        homepage,
        topics,
        private,
        has_issues,
        has_wiki,
        has_projects,
        has_discussions,
        is_template,
        allow_merge_commit,
        allow_squash_merge,
        allow_rebase_merge,
        allow_auto_merge,
        allow_update_branch,
        delete_branch_on_merge,
        squash_merge_commit_title,
        squash_merge_commit_message,
        merge_commit_title,
        merge_commit_message,
        default_branch,
        anonymous_access_enabled,
        archived,
        security,
    } = c;

    macro_rules! field {
        ($name:ident) => {
            pick(
                provenance,
                concat!("repository.", stringify!($name)),
                base.id,
                b.$name.as_ref(),
                child.id,
                $name.as_ref(),
            )
        };
    }

    Some(RepositorySettings {
        // `Option::or` on a `Nullable<T> = Option<Option<T>>` keeps the outer
        // layer, which is what "is this managed" means — so a child writing
        // `description: null` clears the field instead of falling back to the
        // base's value.
        description: field!(description),
        homepage: field!(homepage),
        topics: field!(topics),
        private: field!(private),
        has_issues: field!(has_issues),
        has_wiki: field!(has_wiki),
        has_projects: field!(has_projects),
        has_discussions: field!(has_discussions),
        is_template: field!(is_template),
        allow_merge_commit: field!(allow_merge_commit),
        allow_squash_merge: field!(allow_squash_merge),
        allow_rebase_merge: field!(allow_rebase_merge),
        allow_auto_merge: field!(allow_auto_merge),
        allow_update_branch: field!(allow_update_branch),
        delete_branch_on_merge: field!(delete_branch_on_merge),
        squash_merge_commit_title: field!(squash_merge_commit_title),
        squash_merge_commit_message: field!(squash_merge_commit_message),
        merge_commit_title: field!(merge_commit_title),
        merge_commit_message: field!(merge_commit_message),
        default_branch: field!(default_branch),
        anonymous_access_enabled: field!(anonymous_access_enabled),
        archived: field!(archived),
        security: merge_security(provenance, base.id, b.security, child.id, *security),
    })
}

/// Merge the security block, which must recurse rather than be taken whole.
///
/// Taking the child's block outright would unmanage every setting it did not
/// mention: a child enabling `secret_scanning` would silently drop a base's
/// `advanced_security`.
fn merge_security(
    provenance: &mut Provenance,
    base_id: SourceId,
    base: Option<SecuritySettings>,
    child_id: SourceId,
    child: Option<SecuritySettings>,
) -> Option<SecuritySettings> {
    if base.is_none() && child.is_none() {
        return None;
    }
    let b = base.unwrap_or_default();
    let c = child.unwrap_or_default();

    let SecuritySettings {
        advanced_security,
        secret_scanning,
        secret_scanning_push_protection,
        dependabot_security_updates,
        secret_scanning_validity_checks,
    } = c;

    macro_rules! field {
        ($name:ident) => {
            pick(
                provenance,
                concat!("repository.security.", stringify!($name)),
                base_id,
                b.$name.as_ref(),
                child_id,
                $name.as_ref(),
            )
        };
    }

    Some(SecuritySettings {
        advanced_security: field!(advanced_security),
        secret_scanning: field!(secret_scanning),
        secret_scanning_push_protection: field!(secret_scanning_push_protection),
        dependabot_security_updates: field!(dependabot_security_updates),
        secret_scanning_validity_checks: field!(secret_scanning_validity_checks),
    })
}

/// Merge one collection section by item identity.
fn merge_section<T, K>(
    provenance: &mut Provenance,
    section: &str,
    base: &Layer<'_>,
    child: &Layer<'_>,
    project: impl Fn(&Settings) -> Option<&Prunable<T>>,
    key: impl Fn(&T) -> K,
) -> Option<Prunable<T>>
where
    T: Clone,
    K: PartialEq,
{
    let base_section = project(base.settings);
    let child_section = project(child.settings);

    match (base_section, child_section) {
        (None, None) => None,

        // Only one document declares it: take it as written, and point every
        // item back at that document.
        (Some(section_value), None) => {
            let items = section_value.items().to_vec();
            record_items(provenance, section, base, 0..items.len());
            // `prune` does not inherit, so a base that prunes does not make the
            // child prune.
            Some(reshape(items, false))
        }
        (None, Some(section_value)) => {
            let items = section_value.items().to_vec();
            record_items(provenance, section, child, 0..items.len());
            Some(section_value.clone())
        }

        (Some(base_value), Some(child_value)) => {
            // The section key itself belongs to the child, which is the document
            // that decided the section is managed here.
            provenance.record(section, child.id, section);

            let base_items = base_value.items();
            let child_items = child_value.items();

            // Base order, with each child item replacing the base item of the
            // same identity in place, and child-only items appended. Base order
            // because it is stable under child edits, so adding a label to the
            // child does not renumber every inherited one.
            let mut merged: Vec<T> = Vec::with_capacity(base_items.len() + child_items.len());
            let mut taken = vec![false; child_items.len()];

            for item in base_items {
                let replacement = child_items
                    .iter()
                    .enumerate()
                    .find(|(position, candidate)| !taken[*position] && key(candidate) == key(item));

                match replacement {
                    Some((position, candidate)) => {
                        taken[position] = true;
                        record_item(provenance, section, child, merged.len(), position);
                        merged.push(candidate.clone());
                    }
                    None => {
                        record_item(provenance, section, base, merged.len(), merged.len());
                        merged.push(item.clone());
                    }
                }
            }

            // Untaken child items keep their own order. Note this never
            // de-duplicates *within* a document: two identical keys in the child
            // stay two items, so the duplicate check still fires instead of the
            // merge quietly swallowing the mistake.
            for (position, item) in child_items.iter().enumerate() {
                if !taken[position] {
                    record_item(provenance, section, child, merged.len(), position);
                    merged.push(item.clone());
                }
            }

            Some(reshape(merged, child_value.prune()))
        }
    }
}

/// Put a merged collection back into the terser of the two equivalent forms.
///
/// `List(v)` and `Managed { prune: false, items: Some(v) }` are observationally
/// identical — `items`, `prune` and `is_empty` are the only accessors — so the
/// object form is worth keeping only when it carries a `prune` that matters.
fn reshape<T>(items: Vec<T>, prune: bool) -> Prunable<T> {
    if prune {
        Prunable::Managed {
            prune: true,
            items: Some(items),
        }
    } else {
        Prunable::List(items)
    }
}

/// Record where one merged item was written.
fn record_item(
    provenance: &mut Provenance,
    section: &str,
    layer: &Layer<'_>,
    merged_position: usize,
    source_position: usize,
) {
    provenance.record(
        format!("{section}.{merged_position}"),
        layer.id,
        format!(
            "{}.{source_position}",
            physical_section(section, layer.spans)
        ),
    );
}

/// Record a contiguous run of items that kept their positions.
fn record_items(
    provenance: &mut Provenance,
    section: &str,
    layer: &Layer<'_>,
    positions: std::ops::Range<usize>,
) {
    let physical = physical_section(section, layer.spans);
    provenance.record(section, layer.id, section);
    for position in positions {
        provenance.record(
            format!("{section}.{position}"),
            layer.id,
            format!("{physical}.{position}"),
        );
    }
}

/// Where a section's items physically live in a given document.
///
/// The parsed `Prunable` has forgotten which of its two forms it was written
/// in, so the document has to be asked.
fn physical_section(section: &str, spans: &SpanIndex) -> String {
    let nested = format!("{section}.items");
    if spans.contains(&nested) {
        nested
    } else {
        section.to_string()
    }
}

#[cfg(test)]
mod tests;
