//! Label model and normalisation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{Finding, suggest};
use crate::resources::{FieldDiff, ValidateCtx};

/// A single label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Label {
    /// Label name.
    ///
    /// When `new_name` is set, this is the name the label currently has.
    pub name: String,

    /// Six hexadecimal digits, with or without a leading `#`.
    ///
    /// Normalised to lowercase without the `#`, which is how GitHub stores it.
    #[serde(default = "default_color")]
    pub color: String,

    /// Optional short description, at most 100 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Rename this label to the given name.
    ///
    /// Renaming preserves the label's assignments, which deleting and recreating
    /// would not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
}

/// GitHub's default label colour when none is given.
fn default_color() -> String {
    "ededed".to_string()
}

/// A label as the API reports it.
///
/// Deliberately a separate type from [`Label`]. The configuration type carries
/// `deny_unknown_fields` so that a typo in a user's file is caught, but the API
/// payload also contains `id`, `node_id`, `url` and `default`, which are not
/// configuration. Sharing one type would force a choice between rejecting valid
/// API responses and silently accepting typos.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelState {
    /// Label name.
    pub name: String,
    /// Six hexadecimal digits, without the leading `#`.
    #[serde(default)]
    pub color: String,
    /// Short description, `null` or `""` when unset.
    #[serde(default)]
    pub description: Option<String>,
}

impl LabelState {
    /// The comparable configuration form of this label.
    pub fn as_label(&self) -> Label {
        Label {
            name: self.name.clone(),
            color: self.color.clone(),
            description: self.description.clone(),
            new_name: None,
        }
        .normalized()
    }
}

impl Label {
    /// Build a label for tests and exports.
    pub fn new(name: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: color.into(),
            description: None,
            new_name: None,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Rename to the given name.
    pub fn renamed_to(mut self, new_name: impl Into<String>) -> Self {
        self.new_name = Some(new_name.into());
        self
    }

    /// A normalised copy, safe to compare against a normalised counterpart.
    ///
    /// * colours lose their `#` and are lowercased, matching GitHub's storage;
    /// * an empty description becomes `None`, because GitHub reports "no
    ///   description" as `""` while users write it as an omitted key.
    pub fn normalized(&self) -> Self {
        Self {
            name: self.name.trim().to_string(),
            color: normalize_color(&self.color),
            description: self
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .map(str::to_string),
            new_name: self
                .new_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        }
    }

    /// The name used to find this label on GitHub.
    pub fn lookup_name(&self) -> &str {
        &self.name
    }

    /// The label as it will exist after the change is applied.
    pub fn applied(&self) -> Self {
        Self {
            name: self.new_name.clone().unwrap_or_else(|| self.name.clone()),
            color: self.color.clone(),
            description: self.description.clone(),
            new_name: None,
        }
    }

    /// Field-level detail for a newly created label.
    pub fn as_fields(&self) -> Vec<FieldDiff> {
        let mut fields = vec![FieldDiff::added("color", &self.color)];
        if let Some(description) = &self.description {
            fields.push(FieldDiff::added("description", description));
        }
        fields
    }

    /// Compare against the current state, returning only real differences.
    ///
    /// A `description` that is absent from the configuration is left alone rather
    /// than cleared: omission means "unmanaged", not "empty".
    pub fn diff_against(&self, current: &Self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();

        if let Some(new_name) = &self.new_name
            && new_name != &current.name
        {
            fields.push(FieldDiff::changed("name", &current.name, new_name));
        }

        if self.color != current.color {
            fields.push(FieldDiff::changed("color", &current.color, &self.color));
        }

        match (&self.description, &current.description) {
            (Some(desired), Some(current)) if desired != current => {
                fields.push(FieldDiff::changed("description", current, desired));
            }
            (Some(desired), None) => fields.push(FieldDiff::added("description", desired)),
            // An omitted description is unmanaged: never clear what is there.
            (None, _) | (Some(_), Some(_)) => {}
        }

        fields
    }

    /// Request body for creating this label.
    pub fn as_create_body(&self) -> Value {
        let mut body = serde_json::Map::new();
        body.insert("name".into(), json!(self.name));
        body.insert("color".into(), json!(self.color));
        if let Some(description) = &self.description {
            body.insert("description".into(), json!(description));
        }
        Value::Object(body)
    }

    /// Request body for updating this label, given the name it currently has.
    pub fn as_update_body(&self, existing: &str) -> Value {
        let mut body = serde_json::Map::new();
        if self.name != existing {
            body.insert("new_name".into(), json!(self.name));
        }
        body.insert("color".into(), json!(self.color));
        if let Some(description) = &self.description {
            body.insert("description".into(), json!(description));
        }
        Value::Object(body)
    }
}

/// Normalise a colour to GitHub's storage form.
pub fn normalize_color(color: &str) -> String {
    color.trim().trim_start_matches('#').to_lowercase()
}

/// The key under which a label is matched.
///
/// GitHub treats label names case-insensitively for uniqueness — you cannot have
/// both `Bug` and `bug` — so matching must be case-insensitive too, otherwise a
/// case-only change would be planned as "create" and fail with a 422.
pub fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Whether a string is a valid six-digit hex colour.
fn is_valid_color(color: &str) -> bool {
    color.len() == 6 && color.chars().all(|character| character.is_ascii_hexdigit())
}

/// GitHub's own default labels, used to suggest corrections.
const WELL_KNOWN: &[&str] = &[
    "bug",
    "documentation",
    "duplicate",
    "enhancement",
    "good first issue",
    "help wanted",
    "invalid",
    "question",
    "wontfix",
];

/// Maximum length GitHub accepts for a label description.
const MAX_DESCRIPTION: usize = 100;

/// Validate the desired labels.
pub fn validate(labels: &[Label], ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let base = ctx.items_path("labels");

    for (position, label) in labels.iter().enumerate() {
        let path = format!("{base}.{position}");

        if label.name.trim().is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::labels::empty_name",
                    "label name cannot be empty",
                )
                .at(ctx.span(&format!("{path}.name")))
                .labelled("empty name"),
            );
        }

        if let Some(previous) = seen.insert(key(&label.name), position) {
            findings.push(
                Finding::error(
                    "gh_settings::labels::duplicate",
                    format!("label `{}` is declared more than once", label.name),
                )
                .at(ctx.span(&format!("{path}.name")))
                .labelled(format!("already declared at labels.{previous}"))
                .help("label names are case-insensitive on GitHub; remove the duplicate"),
            );
        }

        if !is_valid_color(&label.color) {
            findings.push(
                Finding::error(
                    "gh_settings::labels::invalid_color",
                    format!("`{}` is not a valid label colour", label.color),
                )
                .at(ctx.span(&format!("{path}.color")))
                .labelled("expected six hexadecimal digits")
                .help("colours look like `d73a4a`, optionally prefixed with `#`"),
            );
        }

        if let Some(description) = &label.description
            && description.chars().count() > MAX_DESCRIPTION
        {
            findings.push(
                Finding::error(
                    "gh_settings::labels::description_too_long",
                    format!(
                        "label description is {} characters, the maximum is {MAX_DESCRIPTION}",
                        description.chars().count()
                    ),
                )
                .at(ctx.span(&format!("{path}.description")))
                .labelled("too long"),
            );
        }

        if let Some(new_name) = &label.new_name {
            if new_name == &label.name {
                findings.push(
                    Finding::warning(
                        "gh_settings::labels::pointless_rename",
                        format!("label `{}` is renamed to itself", label.name),
                    )
                    .at(ctx.span(&format!("{path}.new_name")))
                    .help("remove `new_name`"),
                );
            }
            if new_name.trim().is_empty() {
                findings.push(
                    Finding::error(
                        "gh_settings::labels::empty_new_name",
                        "`new_name` cannot be empty",
                    )
                    .at(ctx.span(&format!("{path}.new_name"))),
                );
            }
        }
    }

    // Renaming A to B while also declaring B is contradictory: the two rules
    // disagree about what should exist afterwards.
    let declared: std::collections::HashSet<String> =
        labels.iter().map(|label| key(&label.name)).collect();
    for (position, label) in labels.iter().enumerate() {
        if let Some(new_name) = &label.new_name
            && declared.contains(&key(new_name))
            && key(new_name) != key(&label.name)
        {
            findings.push(
                Finding::error(
                    "gh_settings::labels::rename_collision",
                    format!(
                        "label `{}` is renamed to `{new_name}`, which is also declared separately",
                        label.name
                    ),
                )
                .at(ctx.span(&format!("{base}.{position}.new_name")))
                .labelled("collides with another entry")
                .help("keep either the rename or the standalone declaration, not both"),
            );
        }
    }

    findings
}

/// Suggest a well-known label name close to the given one.
pub fn suggest_name(name: &str) -> Option<String> {
    suggest(name, WELL_KNOWN)
}
