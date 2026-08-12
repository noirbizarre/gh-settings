//! The Actions variable model.
//!
//! Variables are the readable half of what ADR-009 declined: a secret's value
//! can never be read back, so it can neither be diffed nor exported, whereas a
//! variable's value comes straight out of the API. That difference is the whole
//! reason this resource exists and secrets do not.
//!
//! # Normalisation
//!
//! GitHub matches variable names case-insensitively and echoes them back
//! uppercased, so names are compared under [`key`]. Values are *not*
//! normalised: whitespace is meaningful inside a variable value, and trimming
//! one would silently change what a workflow sees.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Finding;
use crate::resources::{FieldDiff, ValidateCtx};

/// A single Actions variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    /// The variable name, for example `DEPLOY_URL`.
    ///
    /// GitHub uppercases names and matches them case-insensitively, so `api_url`
    /// and `API_URL` are the same variable. Names may contain only letters,
    /// digits and underscores, may not start with a digit, and may not start
    /// with `GITHUB_`.
    pub name: String,

    /// The value.
    ///
    /// Stored and compared verbatim: leading and trailing whitespace is
    /// significant, because a workflow will see exactly what is written here.
    pub value: String,
}

impl Variable {
    /// Build a variable.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// The comparable form: the name canonicalised, the value untouched.
    pub fn normalized(&self) -> Self {
        Self {
            name: key(&self.name),
            value: self.value.clone(),
        }
    }

    /// Fields reported when the variable is created.
    pub fn as_fields(&self) -> Vec<FieldDiff> {
        vec![FieldDiff::added("value", self.value.clone())]
    }

    /// Difference against what exists, or an empty vector when they agree.
    pub fn diff_against(&self, current: &Self) -> Vec<FieldDiff> {
        if self.value == current.value {
            Vec::new()
        } else {
            vec![FieldDiff::changed(
                "value",
                current.value.clone(),
                self.value.clone(),
            )]
        }
    }

    /// Body for both `POST` (create) and `PATCH` (update).
    ///
    /// The update endpoint accepts `name` as well, which is how a variable would
    /// be renamed; we never send a different one, because a rename is
    /// indistinguishable from a delete-and-create at the level of a declarative
    /// file — there is nothing in the configuration that says the two names are
    /// the same variable.
    pub fn as_body(&self) -> Value {
        json!({ "name": self.name, "value": self.value })
    }
}

/// A variable as the API returns it.
///
/// Separate from [`Variable`] because the API payload carries `created_at` and
/// `updated_at`, which the configuration type rejects via
/// `deny_unknown_fields` — the same split labels makes, for the same reason.
#[derive(Debug, Clone, Deserialize)]
pub struct VariableState {
    /// The variable name, as GitHub spells it.
    pub name: String,
    /// The value.
    pub value: String,
}

impl VariableState {
    /// The normalised configuration form.
    pub fn as_variable(&self) -> Variable {
        Variable {
            name: key(&self.name),
            value: self.value.clone(),
        }
    }
}

/// The envelope both variable endpoints return.
///
/// `{"total_count": n, "variables": [...]}` rather than a bare array, which is
/// why these endpoints are read with a single `per_page=100` request instead of
/// `--paginate`: `gh api --paginate` concatenates JSON documents and has no way
/// to merge two envelopes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VariablePage {
    /// The variables on the page.
    #[serde(default)]
    pub variables: Vec<VariableState>,
}

/// The key under which a variable is matched.
///
/// GitHub rejects `foo` when `FOO` exists, so matching must be
/// case-insensitive; uppercasing rather than lowercasing matches what the API
/// echoes back, which keeps an exported configuration stable.
pub fn key(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// Whether a name is one GitHub will accept.
fn is_valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Validate a list of variables declared at `path`, e.g. `variables` or
/// `environments.0.variables`.
pub fn validate(variables: &[Variable], path: &str, ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (position, variable) in variables.iter().enumerate() {
        let item = format!("{path}.{position}");

        if variable.name.trim().is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::variables::empty_name",
                    "variable name cannot be empty",
                )
                .at(ctx.span(&format!("{item}.name")))
                .labelled("empty name"),
            );
            continue;
        }

        if !is_valid_name(variable.name.trim()) {
            findings.push(
                Finding::error(
                    "gh_settings::variables::invalid_name",
                    format!("`{}` is not a valid variable name", variable.name),
                )
                .at(ctx.span(&format!("{item}.name")))
                .labelled("invalid name")
                .help(
                    "names may contain only letters, digits and underscores, \
                     and may not start with a digit",
                ),
            );
        }

        // GitHub reserves the prefix for the variables it injects itself and
        // answers a 409 rather than explaining, so catch it before the write.
        if variable
            .name
            .trim()
            .to_ascii_uppercase()
            .starts_with("GITHUB_")
        {
            findings.push(
                Finding::error(
                    "gh_settings::variables::reserved_name",
                    format!("`{}` uses the reserved `GITHUB_` prefix", variable.name),
                )
                .at(ctx.span(&format!("{item}.name")))
                .labelled("reserved prefix")
                .help("GitHub reserves `GITHUB_*` for the variables it sets itself"),
            );
        }

        if let Some(previous) = seen.insert(key(&variable.name), position) {
            findings.push(
                Finding::error(
                    "gh_settings::variables::duplicate",
                    format!("variable `{}` is declared more than once", variable.name),
                )
                .at(ctx.span(&format!("{item}.name")))
                .labelled(format!("already declared at {path}.{previous}"))
                .help("variable names are case-insensitive on GitHub; remove the duplicate"),
            );
        }
    }

    findings
}
