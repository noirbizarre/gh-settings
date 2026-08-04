//! Ruleset model, normalisation and API translation.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::Finding;
use crate::github::{GitHubClient, Resolver, Result as GitHubResult};
use crate::resources::{FieldDiff, ValidateCtx};

/// What a ruleset applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Branches.
    #[default]
    Branch,
    /// Tags.
    Tag,
    /// Pushes, including to repositories without branches yet.
    Push,
}

impl Target {
    /// The API spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Tag => "tag",
            Self::Push => "push",
        }
    }
}

/// Parse the API spelling of a target.
pub fn parse_target(value: &str) -> Option<Target> {
    match value {
        "branch" => Some(Target::Branch),
        "tag" => Some(Target::Tag),
        "push" => Some(Target::Push),
        _ => None,
    }
}

/// How strictly a ruleset is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    /// Not enforced.
    Disabled,
    /// Enforced.
    #[default]
    Active,
    /// Evaluated and reported, but not enforced. Requires GitHub Enterprise.
    Evaluate,
}

impl Enforcement {
    /// The API spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Active => "active",
            Self::Evaluate => "evaluate",
        }
    }
}

/// Parse the API spelling of an enforcement level.
pub fn parse_enforcement(value: &str) -> Option<Enforcement> {
    match value {
        "disabled" => Some(Enforcement::Disabled),
        "active" => Some(Enforcement::Active),
        "evaluate" => Some(Enforcement::Evaluate),
        _ => None,
    }
}

/// When a bypass actor may bypass the ruleset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BypassMode {
    /// Always.
    #[default]
    Always,
    /// Only for pull requests.
    PullRequest,
}

impl BypassMode {
    /// The API spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::PullRequest => "pull_request",
        }
    }
}

/// Who may bypass a ruleset.
///
/// Declared by slug rather than id: numeric ids are neither stable across
/// organisations nor meaningful to a human reading the file, which would make an
/// exported configuration unusable anywhere but its origin. Resolution happens in
/// [`Resource::prepare`](crate::resources::Resource::prepare).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BypassActor {
    /// Bypass through an organisation team, by slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,

    /// Bypass through a GitHub App, by slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,

    /// Bypass for organisation administrators.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub organization_admin: bool,

    /// Raw actor identifier.
    ///
    /// An escape hatch for actor kinds this build does not model by name, such as
    /// repository roles, whose identifiers vary between organisations and so
    /// cannot be safely mapped from a name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<u64>,

    /// Raw actor type, used together with `actor_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_type: Option<String>,

    /// When the bypass applies.
    #[serde(default)]
    pub bypass_mode: BypassMode,
}

impl BypassActor {
    /// A team bypass.
    pub fn team(slug: impl Into<String>) -> Self {
        Self {
            team: Some(slug.into()),
            ..Self::empty()
        }
    }

    /// An app bypass.
    pub fn app(slug: impl Into<String>) -> Self {
        Self {
            app: Some(slug.into()),
            ..Self::empty()
        }
    }

    /// An organisation administrator bypass.
    pub fn organization_admin() -> Self {
        Self {
            organization_admin: true,
            ..Self::empty()
        }
    }

    /// An empty actor, for use with struct update syntax.
    fn empty() -> Self {
        Self {
            team: None,
            app: None,
            organization_admin: false,
            actor_id: None,
            actor_type: None,
            bypass_mode: BypassMode::Always,
        }
    }

    /// Whether this actor still needs a slug lookup.
    pub fn needs_resolution(&self) -> bool {
        self.actor_id.is_none() && (self.team.is_some() || self.app.is_some())
    }

    /// Resolve the slug to an identifier, caching lookups for the run.
    pub async fn resolve(
        &mut self,
        client: &dyn GitHubClient,
        owner: &str,
        resolver: &Resolver,
    ) -> GitHubResult<()> {
        if !self.needs_resolution() {
            // `organization_admin` has a fixed identifier and needs no lookup.
            if self.organization_admin && self.actor_id.is_none() {
                self.actor_id = Some(1);
                self.actor_type = Some("OrganizationAdmin".into());
            }
            return Ok(());
        }

        if let Some(slug) = self.team.clone() {
            self.actor_id = Some(resolver.team(client, owner, &slug).await?);
            self.actor_type = Some("Team".into());
            return Ok(());
        }

        if let Some(slug) = self.app.clone() {
            self.actor_id = Some(resolver.app(client, &slug).await?);
            self.actor_type = Some("Integration".into());
        }

        Ok(())
    }

    /// Build the API representation.
    pub fn to_api(&self) -> Value {
        json!({
            "actor_id": self.actor_id,
            "actor_type": self.actor_type.clone().unwrap_or_else(|| "Team".into()),
            "bypass_mode": self.bypass_mode.as_str(),
        })
    }

    /// Read an actor back from the API.
    pub fn from_api(value: &Value) -> Option<Self> {
        let actor_type = value.get("actor_type").and_then(Value::as_str)?;
        let actor_id = value.get("actor_id").and_then(Value::as_u64);
        let bypass_mode = match value.get("bypass_mode").and_then(Value::as_str) {
            Some("pull_request") => BypassMode::PullRequest,
            _ => BypassMode::Always,
        };

        Some(Self {
            team: None,
            app: None,
            organization_admin: actor_type == "OrganizationAdmin",
            actor_id,
            actor_type: Some(actor_type.to_string()),
            bypass_mode,
        })
    }

    /// A normalised copy.
    ///
    /// Slugs are deliberately *kept*: they are what validation reports on and
    /// what [`Self::resolve`] needs. Equality between a configured
    /// `{ team: eng }` and the `{ actor_id: 42, actor_type: Team }` the API
    /// returns is established by [`Self::comparable`], not by dropping fields.
    pub fn normalized(&self) -> Self {
        Self {
            team: self.team.as_deref().map(str::trim).map(str::to_string),
            app: self.app.as_deref().map(str::trim).map(str::to_string),
            organization_admin: self.organization_admin,
            actor_id: self.actor_id,
            actor_type: self.actor_type.clone(),
            bypass_mode: self.bypass_mode,
        }
    }

    /// The identity the API actually cares about.
    ///
    /// Comparing on this rather than on the whole struct is what lets a
    /// human-friendly `{ team: eng }` match the numeric form GitHub returns,
    /// without having to discard the slug that made it readable.
    pub fn comparable(&self) -> (Option<u64>, String, &'static str) {
        (
            self.actor_id,
            self.actor_type.clone().unwrap_or_default(),
            self.bypass_mode.as_str(),
        )
    }

    /// A stable sort key, so actor order never causes a spurious diff.
    pub fn sort_key(&self) -> (String, u64, &'static str) {
        (
            self.actor_type.clone().unwrap_or_default(),
            self.actor_id.unwrap_or(0),
            self.bypass_mode.as_str(),
        )
    }
}

/// Which refs a ruleset applies to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    /// Ref name matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<RefNameCondition>,
}

impl Conditions {
    /// Build the API representation.
    pub fn to_api(&self) -> Value {
        let mut object = Map::new();
        if let Some(ref_name) = &self.ref_name {
            object.insert(
                "ref_name".into(),
                json!({
                    "include": ref_name.include,
                    "exclude": ref_name.exclude,
                }),
            );
        }
        Value::Object(object)
    }

    /// Read conditions back from the API.
    pub fn from_api(value: &Value) -> Option<Self> {
        let ref_name = value.get("ref_name").map(|ref_name| RefNameCondition {
            include: string_list(ref_name.get("include")),
            exclude: string_list(ref_name.get("exclude")),
        });
        Some(Self { ref_name })
    }

    /// A normalised copy with deterministic ordering.
    pub fn normalized(&self) -> Self {
        Self {
            ref_name: self.ref_name.as_ref().map(|ref_name| {
                let mut include = ref_name.include.clone();
                let mut exclude = ref_name.exclude.clone();
                include.sort();
                include.dedup();
                exclude.sort();
                exclude.dedup();
                RefNameCondition { include, exclude }
            }),
        }
    }

    /// Whether nothing is declared.
    pub fn is_empty(&self) -> bool {
        self.ref_name
            .as_ref()
            .is_none_or(|ref_name| ref_name.include.is_empty() && ref_name.exclude.is_empty())
    }
}

/// Ref name matching.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefNameCondition {
    /// Patterns to include.
    ///
    /// Supports the special values `~DEFAULT_BRANCH` and `~ALL`.
    #[serde(default)]
    pub include: Vec<String>,

    /// Patterns to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A single rule within a ruleset.
///
/// Modelled as `{ type, parameters }` rather than a closed enum of every known
/// rule so that a rule type this build predates round-trips untouched instead of
/// being silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The rule type, for example `pull_request` or `required_status_checks`.
    #[serde(rename = "type")]
    pub rule_type: String,

    /// Rule-specific parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

impl Rule {
    /// A parameterless rule.
    pub fn new(rule_type: impl Into<String>) -> Self {
        Self {
            rule_type: rule_type.into(),
            parameters: None,
        }
    }

    /// A rule with parameters.
    pub fn with(rule_type: impl Into<String>, parameters: Value) -> Self {
        Self {
            rule_type: rule_type.into(),
            parameters: Some(parameters),
        }
    }

    /// Whether this build recognises the rule type.
    pub fn is_known(&self) -> bool {
        KNOWN_RULE_TYPES.contains(&self.rule_type.as_str())
    }

    /// Marker used in documentation and diagnostics for unrecognised rules.
    pub fn is_unknown(&self) -> bool {
        !self.is_known()
    }

    /// Build the API representation.
    pub fn to_api(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".into(), json!(self.rule_type));
        if let Some(parameters) = &self.parameters
            && !is_empty_object(parameters)
        {
            object.insert("parameters".into(), parameters.clone());
        }
        Value::Object(object)
    }

    /// Read a rule back from the API.
    pub fn from_api(value: &Value) -> Option<Self> {
        let rule_type = value.get("type").and_then(Value::as_str)?;
        let parameters = value
            .get("parameters")
            .filter(|parameters| !is_empty_object(parameters))
            .cloned();
        Some(Self {
            rule_type: rule_type.to_string(),
            parameters,
        })
    }

    /// A normalised copy.
    ///
    /// An empty `parameters` object and an absent one are the same thing to the
    /// API, so they must be the same thing to the diff.
    pub fn normalized(&self) -> Self {
        Self {
            rule_type: self.rule_type.clone(),
            parameters: self
                .parameters
                .clone()
                .filter(|parameters| !is_empty_object(parameters)),
        }
    }
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty) || value.is_null()
}

/// Rule types this build knows about.
///
/// Used only to warn about probable typos: unknown types are still preserved.
pub const KNOWN_RULE_TYPES: &[&str] = &[
    "creation",
    "update",
    "deletion",
    "required_linear_history",
    "required_deployments",
    "required_signatures",
    "pull_request",
    "required_status_checks",
    "non_fast_forward",
    "commit_message_pattern",
    "commit_author_email_pattern",
    "committer_email_pattern",
    "branch_name_pattern",
    "tag_name_pattern",
    "workflows",
    "code_scanning",
    "merge_queue",
    "file_path_restriction",
    "max_file_path_length",
    "file_extension_restriction",
    "max_file_size",
];

/// A repository ruleset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ruleset {
    /// Ruleset name.
    ///
    /// This is the identity used for matching. Server ids never appear in the
    /// configuration because they are not portable between repositories.
    pub name: String,

    /// What the ruleset applies to.
    #[serde(default)]
    pub target: Target,

    /// How strictly it is applied.
    #[serde(default)]
    pub enforcement: Enforcement,

    /// Who may bypass it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bypass_actors: Vec<BypassActor>,

    /// Which refs it applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<Conditions>,

    /// The rules themselves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
}

impl Ruleset {
    /// Build a ruleset.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            target: Target::default(),
            enforcement: Enforcement::default(),
            bypass_actors: Vec::new(),
            conditions: None,
            rules: Vec::new(),
        }
    }

    /// Attach rules.
    pub fn with_rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = rules;
        self
    }

    /// Set the enforcement level.
    pub fn with_enforcement(mut self, enforcement: Enforcement) -> Self {
        self.enforcement = enforcement;
        self
    }

    /// A normalised copy, safe to compare against a normalised counterpart.
    ///
    /// Rules and bypass actors are canonically ordered because the API returns
    /// them in an arbitrary order; without this the plan would report changes on
    /// every run.
    pub fn normalized(&self) -> Self {
        let mut rules: Vec<Rule> = self.rules.iter().map(Rule::normalized).collect();
        rules.sort_by(|a, b| a.rule_type.cmp(&b.rule_type));

        let mut bypass_actors: Vec<BypassActor> = self
            .bypass_actors
            .iter()
            .map(BypassActor::normalized)
            .collect();
        bypass_actors.sort_by_key(BypassActor::sort_key);

        Self {
            name: self.name.trim().to_string(),
            target: self.target,
            enforcement: self.enforcement,
            bypass_actors,
            conditions: self
                .conditions
                .as_ref()
                .map(Conditions::normalized)
                .filter(|conditions| !conditions.is_empty()),
            rules,
        }
    }

    /// Build the API request body.
    pub fn as_body(&self) -> Value {
        super::ruleset_body(self)
    }

    /// Compare against the current state.
    pub fn diff_against(&self, current: &Self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();

        if self.target != current.target {
            fields.push(FieldDiff::changed(
                "target",
                current.target.as_str(),
                self.target.as_str(),
            ));
        }

        if self.enforcement != current.enforcement {
            fields.push(FieldDiff::changed(
                "enforcement",
                current.enforcement.as_str(),
                self.enforcement.as_str(),
            ));
        }

        if self.conditions != current.conditions {
            fields.push(FieldDiff::changed(
                "conditions",
                render_conditions(current.conditions.as_ref()),
                render_conditions(self.conditions.as_ref()),
            ));
        }

        let desired_actors: Vec<_> = self
            .bypass_actors
            .iter()
            .map(BypassActor::comparable)
            .collect();
        let current_actors: Vec<_> = current
            .bypass_actors
            .iter()
            .map(BypassActor::comparable)
            .collect();
        if desired_actors != current_actors {
            fields.push(FieldDiff::changed(
                "bypass_actors",
                current.bypass_actors.len().to_string(),
                self.bypass_actors.len().to_string(),
            ));
        }

        // Report rules individually: "rules changed" is useless in a plan.
        let current_types: Vec<&str> = current
            .rules
            .iter()
            .map(|rule| rule.rule_type.as_str())
            .collect();
        let desired_types: Vec<&str> = self
            .rules
            .iter()
            .map(|rule| rule.rule_type.as_str())
            .collect();

        for rule in &self.rules {
            match current
                .rules
                .iter()
                .find(|candidate| candidate.rule_type == rule.rule_type)
            {
                None => fields.push(FieldDiff::added(
                    format!("rule {}", rule.rule_type),
                    render_parameters(rule.parameters.as_ref()),
                )),
                Some(existing)
                    if parameters_differ(
                        rule.parameters.as_ref(),
                        existing.parameters.as_ref(),
                    ) =>
                {
                    fields.push(FieldDiff::changed(
                        format!("rule {}", rule.rule_type),
                        render_parameters(existing.parameters.as_ref()),
                        render_parameters(rule.parameters.as_ref()),
                    ));
                }
                Some(_) => {}
            }
        }

        for rule in &current.rules {
            if !desired_types.contains(&rule.rule_type.as_str()) {
                fields.push(FieldDiff::removed(
                    format!("rule {}", rule.rule_type),
                    render_parameters(rule.parameters.as_ref()),
                ));
            }
        }

        let _ = current_types;
        fields
    }
}

/// Whether a rule's declared parameters differ from what GitHub reports.
///
/// GitHub fills in defaults the user never wrote: creating a `pull_request`
/// rule with five parameters returns seven, having added `required_reviewers`
/// and `allowed_merge_methods`. Comparing the objects wholesale therefore
/// reports a change on every run, for ever — the permanent diff ADR-002 exists
/// to prevent.
///
/// So only the keys the configuration actually declares are compared. A
/// parameter the user did not write is unmanaged, exactly as an omitted field
/// is everywhere else in this tool: we neither diff it nor reset it.
fn parameters_differ(desired: Option<&Value>, current: Option<&Value>) -> bool {
    match (desired, current) {
        // Nothing declared is nothing managed, whatever the server defaulted.
        (None, _) => false,
        (Some(_), None) => true,
        (Some(desired), Some(current)) => match (desired.as_object(), current.as_object()) {
            (Some(desired), Some(current)) => desired
                .iter()
                .any(|(key, value)| current.get(key) != Some(value)),
            // Not both objects: fall back to whole-value comparison.
            _ => desired != current,
        },
    }
}

fn render_conditions(conditions: Option<&Conditions>) -> String {
    match conditions {
        Some(conditions) => serde_json::to_string(&conditions.to_api()).unwrap_or_default(),
        None => "(none)".to_string(),
    }
}

fn render_parameters(parameters: Option<&Value>) -> String {
    match parameters {
        Some(parameters) => serde_json::to_string(parameters).unwrap_or_default(),
        None => "(no parameters)".to_string(),
    }
}

/// Validate the desired rulesets.
pub fn validate(rulesets: &[Ruleset], ctx: &ValidateCtx<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let base = ctx.items_path("rulesets");

    for (position, ruleset) in rulesets.iter().enumerate() {
        let path = format!("{base}.{position}");

        if ruleset.name.trim().is_empty() {
            findings.push(
                Finding::error(
                    "gh_settings::rulesets::empty_name",
                    "ruleset name cannot be empty",
                )
                .at(ctx.span(&format!("{path}.name"))),
            );
        }

        if let Some(previous) = seen.insert(&ruleset.name, position) {
            findings.push(
                Finding::error(
                    "gh_settings::rulesets::duplicate",
                    format!("ruleset `{}` is declared more than once", ruleset.name),
                )
                .at(ctx.span(&format!("{path}.name")))
                .labelled(format!("already declared at rulesets.{previous}")),
            );
        }

        if ruleset.rules.is_empty() {
            findings.push(
                Finding::warning(
                    "gh_settings::rulesets::no_rules",
                    format!("ruleset `{}` declares no rules", ruleset.name),
                )
                .at(ctx.key_span(&format!("{path}.rules")))
                .help("a ruleset without rules has no effect"),
            );
        }

        let mut rule_types: HashMap<&str, usize> = HashMap::new();
        for (rule_position, rule) in ruleset.rules.iter().enumerate() {
            if let Some(previous) = rule_types.insert(&rule.rule_type, rule_position) {
                findings.push(
                    Finding::error(
                        "gh_settings::rulesets::duplicate_rule",
                        format!(
                            "rule `{}` is declared more than once in ruleset `{}`",
                            rule.rule_type, ruleset.name
                        ),
                    )
                    .at(ctx.span(&format!("{path}.rules.{rule_position}.type")))
                    .labelled(format!("already declared at rules.{previous}")),
                );
            }

            if rule.is_unknown() {
                // A warning, never an error: GitHub ships new rule types
                // continuously and refusing them would make the tool a blocker.
                findings.push(
                    Finding::warning(
                        "gh_settings::rulesets::unknown_rule",
                        format!("`{}` is not a rule type this build knows", rule.rule_type),
                    )
                    .at(ctx.span(&format!("{path}.rules.{rule_position}.type")))
                    .suggest(&rule.rule_type, KNOWN_RULE_TYPES)
                    .labelled("unrecognised rule type"),
                );
            }
        }

        // Tag rulesets cannot carry branch-only rules; GitHub rejects them with
        // an opaque 422.
        if ruleset.target == Target::Tag {
            for (rule_position, rule) in ruleset.rules.iter().enumerate() {
                if matches!(
                    rule.rule_type.as_str(),
                    "pull_request" | "required_status_checks" | "merge_queue"
                ) {
                    findings.push(
                        Finding::error(
                            "gh_settings::rulesets::rule_not_valid_for_target",
                            format!("rule `{}` cannot be used on a tag ruleset", rule.rule_type),
                        )
                        .at(ctx.span(&format!("{path}.rules.{rule_position}.type")))
                        .labelled("branch-only rule"),
                    );
                }
            }
        }

        for (actor_position, actor) in ruleset.bypass_actors.iter().enumerate() {
            let declared = [
                actor.team.is_some(),
                actor.app.is_some(),
                actor.organization_admin,
                actor.actor_id.is_some(),
            ]
            .iter()
            .filter(|declared| **declared)
            .count();

            if declared == 0 {
                findings.push(
                    Finding::error(
                        "gh_settings::rulesets::empty_bypass_actor",
                        "bypass actor declares no target",
                    )
                    .at(ctx.span(&format!("{path}.bypass_actors.{actor_position}")))
                    .help("set one of `team`, `app`, `organization_admin` or `actor_id`"),
                );
            } else if declared > 1 {
                findings.push(
                    Finding::error(
                        "gh_settings::rulesets::ambiguous_bypass_actor",
                        "bypass actor declares more than one target",
                    )
                    .at(ctx.span(&format!("{path}.bypass_actors.{actor_position}")))
                    .help(
                        "declare exactly one of `team`, `app`, `organization_admin` or `actor_id`",
                    ),
                );
            }

            if actor.actor_id.is_some() && actor.actor_type.is_none() {
                findings.push(
                    Finding::error(
                        "gh_settings::rulesets::missing_actor_type",
                        "`actor_id` requires `actor_type`",
                    )
                    .at(ctx.span(&format!("{path}.bypass_actors.{actor_position}.actor_id"))),
                );
            }
        }
    }

    findings
}
