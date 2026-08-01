//! The change model.
//!
//! [`Change`] is the single currency shared by the diff engine, the renderers, the
//! JSON plan artifact and the apply path. Resources produce them; nothing else
//! needs to understand what a label or a ruleset actually is.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resources::ResourceId;

/// The kind of operation a change performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// The item does not exist and will be created.
    Create,
    /// The item exists and differs.
    Update,
    /// The item exists, is unmanaged, and pruning is enabled.
    Delete,
    /// Some GitHub objects cannot be updated in place — notably autolinks, which
    /// have no update endpoint. Modelled explicitly rather than pretending an
    /// update happened, because it is destructive and the plan must say so.
    Recreate,
}

impl Op {
    /// The sigil used in plan output.
    pub fn sigil(&self) -> char {
        match self {
            Self::Create => '+',
            Self::Update | Self::Recreate => '~',
            Self::Delete => '-',
        }
    }

    /// Verb used in summaries.
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Recreate => "recreate",
        }
    }

    /// Whether the operation destroys existing state.
    ///
    /// Drives the confirmation prompt and the `--allow-delete`-style guardrails.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete | Self::Recreate)
    }
}

/// A single field-level difference, shown in verbose plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Field name, as it appears in the configuration.
    pub field: String,
    /// Value before the change, `None` when the field is being added.
    pub before: Option<String>,
    /// Value after the change, `None` when the field is being removed.
    pub after: Option<String>,
}

impl FieldDiff {
    /// A changed field.
    pub fn changed(
        field: impl Into<String>,
        before: impl Into<String>,
        after: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            before: Some(before.into()),
            after: Some(after.into()),
        }
    }

    /// An added field.
    pub fn added(field: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            before: None,
            after: Some(after.into()),
        }
    }

    /// A removed field.
    pub fn removed(field: impl Into<String>, before: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            before: Some(before.into()),
            after: None,
        }
    }
}

/// One unit of work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// Which resource produced this change.
    pub resource: ResourceId,
    /// The operation.
    pub op: Op,
    /// Identity of the affected item: a label name, an autolink prefix, a ruleset
    /// name, or a field name for singleton resources such as `repository`.
    pub key: String,
    /// One-line human summary.
    pub summary: String,
    /// Field-level detail, for verbose output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldDiff>,
    /// Resource-owned data needed to perform the change.
    ///
    /// Opaque to everything but the originating resource. It is serialisable so
    /// that `plan --out` can be replayed by `sync --plan`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

impl Change {
    /// Build a change.
    pub fn new(resource: ResourceId, op: Op, key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            resource,
            op,
            summary: format!("{} {} {}", op.verb(), resource.as_str(), key),
            key,
            fields: Vec::new(),
            payload: Value::Null,
        }
    }

    /// Override the generated summary.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// Attach field-level detail.
    pub fn fields(mut self, fields: Vec<FieldDiff>) -> Self {
        self.fields = fields;
        self
    }

    /// Attach the data needed to apply the change.
    pub fn payload(mut self, payload: impl Serialize) -> Self {
        self.payload = serde_json::to_value(payload).unwrap_or(Value::Null);
        self
    }

    /// Whether this change destroys existing state.
    pub fn is_destructive(&self) -> bool {
        self.op.is_destructive()
    }

    /// Decode the payload into a resource's own type.
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }
}

/// Tally of a set of changes, used in summaries and for exit codes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Number of creations.
    pub create: usize,
    /// Number of in-place updates.
    pub update: usize,
    /// Number of deletions.
    pub delete: usize,
    /// Number of delete-and-recreate operations.
    pub recreate: usize,
}

impl Counts {
    /// Tally a slice of changes.
    pub fn of<'a>(changes: impl IntoIterator<Item = &'a Change>) -> Self {
        let mut counts = Self::default();
        for change in changes {
            match change.op {
                Op::Create => counts.create += 1,
                Op::Update => counts.update += 1,
                Op::Delete => counts.delete += 1,
                Op::Recreate => counts.recreate += 1,
            }
        }
        counts
    }

    /// Total number of changes.
    pub fn total(&self) -> usize {
        self.create + self.update + self.delete + self.recreate
    }

    /// Whether there is nothing to do.
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Whether any change destroys existing state.
    pub fn has_destructive(&self) -> bool {
        self.delete + self.recreate > 0
    }
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, other: Self) {
        self.create += other.create;
        self.update += other.update;
        self.delete += other.delete;
        self.recreate += other.recreate;
    }
}

impl serde::Serialize for ResourceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ResourceId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown resource `{value}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn recreate_is_destructive_because_it_deletes_first() {
        assert!(Op::Recreate.is_destructive());
        assert!(Op::Delete.is_destructive());
        assert!(!Op::Create.is_destructive());
        assert!(!Op::Update.is_destructive());
    }

    #[test]
    fn recreate_reads_as_an_update_in_the_margin() {
        // The sigil communicates intent, `is_destructive` communicates risk.
        assert_eq!(Op::Recreate.sigil(), '~');
        assert_eq!(Op::Delete.sigil(), '-');
    }

    #[test]
    fn generates_a_default_summary() {
        let change = Change::new(ResourceId::Labels, Op::Create, "bug");
        assert_eq!(change.summary, "create labels bug");
    }

    #[test]
    fn payloads_round_trip() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Payload {
            name: String,
        }
        let payload = Payload { name: "bug".into() };
        let change = Change::new(ResourceId::Labels, Op::Create, "bug").payload(&payload);
        assert_eq!(change.decode::<Payload>().unwrap(), payload);
    }

    #[test]
    fn counts_tally_by_operation() {
        let changes = vec![
            Change::new(ResourceId::Labels, Op::Create, "a"),
            Change::new(ResourceId::Labels, Op::Create, "b"),
            Change::new(ResourceId::Labels, Op::Delete, "c"),
        ];
        let counts = Counts::of(&changes);
        assert_eq!(counts.create, 2);
        assert_eq!(counts.delete, 1);
        assert_eq!(counts.total(), 3);
        assert!(counts.has_destructive());
    }

    #[test]
    fn resource_ids_serialize_as_their_public_strings() {
        let json = serde_json::to_string(&ResourceId::Autolinks).unwrap();
        assert_eq!(json, "\"autolinks\"");
        assert_eq!(
            serde_json::from_str::<ResourceId>(&json).unwrap(),
            ResourceId::Autolinks
        );
    }
}
