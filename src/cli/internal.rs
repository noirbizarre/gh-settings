//! `gh-settings internal` — documentation generators.
//!
//! Hidden from `--help` on purpose. These are build-time tools, not product
//! surface: they exist so that documentation which describes the code is
//! *produced by* the code, and therefore cannot drift from it.
//!
//! Two things are generated:
//!
//! * the per-resource permission table, from [`Resource::requirement`], which
//!   backs the claim ADR-015 makes about a single source of truth;
//! * the CLI reference, from clap's own command tree, so a flag that exists in
//!   the binary and a flag that appears in the docs are the same list.
//!
//! Both are committed and checked in CI, exactly as the JSON Schema is: a stale
//! copy fails a pull request rather than quietly misleading a reader.
//!
//! [`Resource::requirement`]: crate::resources::Resource::requirement

use std::fmt::Write as _;

use clap::{Command as ClapCommand, CommandFactory};
use miette::Result;

use crate::cli::exit;
use crate::engine::Registry;
use crate::resources::Requirement;
use crate::resources::requirement::Confidence;

/// Arguments for `internal`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which document to generate.
    #[command(subcommand)]
    pub document: Document,
}

/// The generated documents.
#[derive(Debug, clap::Subcommand)]
pub enum Document {
    /// Emit the per-resource permission table.
    Requirements,
    /// Emit the CLI reference.
    Cli,
}

/// Run the command.
pub fn run(args: &Args) -> Result<i32> {
    let rendered = match args.document {
        Document::Requirements => requirements(),
        Document::Cli => cli_reference(),
    };
    print!("{rendered}");
    Ok(exit::SUCCESS)
}

/// Marker used by the tooling to locate a generated region inside a file that
/// is otherwise hand-written.
pub const BEGIN: &str = "<!-- generated: do not edit below -->";
/// Closing marker for a generated region.
pub const END: &str = "<!-- /generated -->";

/// Render the permission table.
///
/// Walks the real registry, so a resource that is added without declaring a
/// requirement cannot be omitted from the published table. `extends` is appended
/// by hand because it belongs to no resource — it is needed while *loading* the
/// configuration — and being outside the registry is exactly how it came to be
/// documented in prose that nothing checks.
pub fn requirements() -> String {
    let registry = Registry::default();
    let mut out = String::new();

    let rows: Vec<(String, &'static Requirement)> = registry
        .all()
        .map(|resource| (format!("`{}`", resource.id()), resource.requirement()))
        .chain([("`extends`".to_string(), &Requirement::CONTENTS)])
        .collect();

    let _ = writeln!(out, "{BEGIN}");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Resource | Fine-grained | Classic | Works with `GITHUB_TOKEN` |"
    );
    let _ = writeln!(out, "|---|---|---|---|");

    let mut any_unverified = false;

    for (label, requirement) in &rows {
        let fine_grained = requirement
            .fine_grained
            .iter()
            .map(|permission| {
                // An unverified mapping is marked in the published table rather
                // than presented as fact. Reporting honest uncertainty is the
                // rule ADR-015 sets for `doctor`, and it applies just as much
                // to the documentation.
                let marker = if permission.confidence == Confidence::Unverified {
                    any_unverified = true;
                    " †"
                } else {
                    ""
                };
                format!("{}: {}{marker}", permission.name, permission.access.label())
            })
            .collect::<Vec<_>>()
            .join(", ");

        let classic = requirement
            .classic
            .iter()
            .map(|scope| format!("`{scope}`"))
            .collect::<Vec<_>>()
            .join(", ");

        let github_token = if requirement.github_token_capable {
            "✔".to_string()
        } else {
            "✘".to_string()
        };

        let _ = writeln!(
            out,
            "| {label} | {fine_grained} | {classic} | {github_token} |"
        );
    }

    if any_unverified {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "† This mapping is inferred rather than confirmed against GitHub's own \
             reference. It is our best understanding, not a guarantee; \
             `gh settings doctor` will tell you what your token can actually do."
        );
    }

    // The `GITHUB_TOKEN` column is the one people get wrong, so spell out why
    // rather than leaving a bare ✘ to be interpreted. Grouped by the reason:
    // `Administration: write` and `Variables: write` are both ungrantable, but
    // they are different permissions and saying otherwise sends people looking
    // in the wrong place.
    let mut blocked: Vec<(&'static str, Vec<String>)> = Vec::new();
    for resource in registry.all() {
        let requirement = resource.requirement();
        if requirement.github_token_capable {
            continue;
        }
        let Some(note) = requirement.github_token_note else {
            continue;
        };
        let label = format!("`{}`", resource.id());
        match blocked.iter_mut().find(|(reason, _)| *reason == note) {
            Some((_, labels)) => labels.push(label),
            None => blocked.push((note, vec![label])),
        }
    }

    let inheritance_note = Requirement::CONTENTS.github_token_note;

    for (note, labels) in &blocked {
        // The note is phrased for a single resource ("requires X"); a group of
        // them needs the plural verb.
        let (verb, reason) = match note.strip_prefix("requires ") {
            Some(reason) if labels.len() > 1 => ("require ", reason),
            _ => ("", *note),
        };
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{} {verb}{reason} — the workflow `permissions:` block has no key that \
             grants it. Use a personal access token or a GitHub App token.",
            join_with_and(labels)
        );
    }

    // A different dead end from the one above, and reached by a different route:
    // the permission exists, it is simply on a repository the workflow token was
    // never scoped to.
    if let Some(note) = inheritance_note {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "`extends` is not a resource — it is read while loading the configuration — \
             and it {note}."
        );
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "{END}");

    out
}

/// Join a list into readable English: `a`, `b` and `c`.
fn join_with_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Render the CLI reference from clap's command tree.
pub fn cli_reference() -> String {
    let command = crate::cli::Cli::command();
    let mut out = String::new();

    let _ = writeln!(out, "# CLI reference");
    let _ = writeln!(out);
    let _ = writeln!(out, "{BEGIN}");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "<!-- Generated from the command definitions by `gh-settings internal cli`. \
         Edit `src/cli/`, then run `mise run docs:reference`. -->"
    );
    let _ = writeln!(out);

    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        let _ = writeln!(out, "{}", reflow(&about.to_string()));
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Global options");
    let _ = writeln!(out);
    let _ = writeln!(out, "Accepted by every command.");
    let _ = writeln!(out);
    render_args(&mut out, &command, true);

    let _ = writeln!(out, "## Commands");
    let _ = writeln!(out);

    for subcommand in command.get_subcommands().filter(|sub| !sub.is_hide_set()) {
        render_subcommand(&mut out, subcommand);
    }

    let _ = writeln!(out, "## Exit codes");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Code | Meaning |");
    let _ = writeln!(out, "|---|---|");
    let _ = writeln!(out, "| `{}` | Success; nothing to do |", exit::SUCCESS);
    let _ = writeln!(out, "| `{}` | Failure |", exit::FAILURE);
    let _ = writeln!(
        out,
        "| `{}` | `plan` found pending changes |",
        exit::CHANGES_PENDING
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "The distinct code for pending changes lets CI detect drift without \
         treating it as a build failure."
    );

    let _ = writeln!(out);
    let _ = writeln!(out, "{END}");

    out
}

/// Render one subcommand section.
fn render_subcommand(out: &mut String, command: &ClapCommand) {
    let name = command.get_name();

    let aliases: Vec<&str> = command.get_visible_aliases().collect();
    let heading = if aliases.is_empty() {
        format!("### `gh settings {name}`")
    } else {
        format!(
            "### `gh settings {name}`  <small>(alias: {})</small>",
            aliases
                .iter()
                .map(|alias| format!("`{alias}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let _ = writeln!(out, "{heading}");
    let _ = writeln!(out);

    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        let _ = writeln!(out, "{}", reflow(&about.to_string()));
        let _ = writeln!(out);
    }

    render_args(out, command, false);
}

/// Render a command's own options as a table.
///
/// `global` selects which side of the split to emit: clap marks the shared
/// options as global, and they are documented once rather than repeated under
/// every command.
fn render_args(out: &mut String, command: &ClapCommand, global: bool) {
    let args: Vec<&clap::Arg> = command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set())
        .filter(|arg| arg.is_global_set() == global)
        .filter(|arg| !matches!(arg.get_id().as_str(), "help" | "version"))
        .collect();

    if args.is_empty() {
        return;
    }

    let _ = writeln!(out, "| Option | Description |");
    let _ = writeln!(out, "|---|---|");

    for arg in args {
        let mut flags = Vec::new();
        if let Some(short) = arg.get_short() {
            flags.push(format!("-{short}"));
        }
        if let Some(long) = arg.get_long() {
            flags.push(format!("--{long}"));
        }
        if flags.is_empty() {
            flags.push(arg.get_id().to_string());
        }

        // A switch takes no value. Rendering `--prune <PRUNE>` with
        // "One of: true, false" would document a flag that does not exist:
        // clap derives that value parser from the `bool` type, but the action
        // is `SetTrue`, so the binary rejects `--prune true`.
        let takes_value = !is_switch(arg);

        let value = takes_value
            .then(|| {
                arg.get_value_names()
                    .and_then(|names| names.first())
                    .map(|name| format!(" <{name}>"))
            })
            .flatten()
            .unwrap_or_default();

        let mut description = arg
            .get_long_help()
            .or_else(|| arg.get_help())
            .map(|help| one_line(&help.to_string()))
            .unwrap_or_default();
        end_sentence(&mut description);

        if takes_value && let Some(values) = possible_values(arg) {
            description.push_str(&format!(" One of: {values}."));
        }

        if takes_value
            && let Some(default) = arg
                .get_default_values()
                .first()
                .map(|value| value.to_string_lossy().into_owned())
                .filter(|value| !value.is_empty())
        {
            description.push_str(&format!(" Defaults to `{default}`."));
        }

        if let Some(env) = arg.get_env() {
            description.push_str(&format!(" Env: `{}`.", env.to_string_lossy()));
        }

        let _ = writeln!(
            out,
            "| `{}{value}` | {} |",
            flags.join(", "),
            description.trim()
        );
    }

    let _ = writeln!(out);
}

/// Whether an argument is a switch rather than something taking a value.
///
/// clap gives a `bool` field a `true`/`false` value parser even though the
/// action is `SetTrue` and the binary accepts no value, so the action is the
/// only reliable signal.
fn is_switch(arg: &clap::Arg) -> bool {
    matches!(
        arg.get_action(),
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse | clap::ArgAction::Count
    )
}

/// Terminate a help string so appended sentences read correctly.
fn end_sentence(text: &mut String) {
    if !text.is_empty() && !text.ends_with(['.', '!', '?', ':']) {
        text.push('.');
    }
}

/// The accepted values of a value-enum argument, if it has any.
fn possible_values(arg: &clap::Arg) -> Option<String> {
    let values = arg.get_possible_values();
    if values.is_empty() {
        return None;
    }
    Some(
        values
            .iter()
            .map(|value| format!("`{}`", value.get_name()))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Collapse a help string onto one line, for use inside a table cell.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// Normalise a multi-paragraph help string for prose.
///
/// clap hard-wraps long help; paragraphs are preserved but the wrapping inside
/// them is not, so the Markdown renderer decides line length.
fn reflow(text: &str) -> String {
    text.split("\n\n")
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourceId;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_requirements_table_covers_every_registered_resource() {
        // A resource added without a requirement must not silently vanish from
        // the published table; that is the whole point of generating it.
        let table = requirements();
        for id in ResourceId::ALL {
            assert!(
                table.contains(&format!("| `{id}` |")),
                "{id} is missing from the generated table"
            );
        }
    }

    #[test]
    fn the_requirements_table_reflects_the_declared_data() {
        let table = requirements();
        // labels live under Issues, everything else under Administration.
        assert!(table.contains("| `labels` | Metadata: read, Issues: write | `repo` | ✔ |"));
        assert!(
            table.contains("| `repository` | Metadata: read, Administration: write | `repo` | ✘ |")
        );
    }

    #[test]
    fn the_table_covers_the_permission_that_belongs_to_no_resource() {
        // `extends` is read while loading, so it is not in the registry. Being
        // outside the registry is how it came to be documented only in prose
        // that nothing checked, while the table above claimed to be exhaustive.
        let table = requirements();
        assert!(
            table.contains("| `extends` | Contents: read | `repo` | ✘ |"),
            "{table}"
        );
        assert!(
            table.contains("Contents: read on the *other* repository"),
            "{table}"
        );
    }

    #[test]
    fn the_table_explains_the_github_token_column() {
        // A bare ✘ invites the reader to guess. The commonest support question
        // is answered inline instead.
        let table = requirements();
        assert!(table.contains("cannot be granted"), "{table}");

        // Each ungrantable permission is named separately: `Administration` and
        // `Variables` are different permissions, and lumping them together
        // would send half the readers looking in the wrong place.
        assert!(table.contains("require Administration: write"), "{table}");
        assert!(table.contains("requires Variables: write"), "{table}");
    }

    #[test]
    fn generated_regions_are_delimited() {
        for rendered in [requirements(), cli_reference()] {
            assert!(rendered.contains(BEGIN));
            assert!(rendered.trim_end().ends_with(END));
        }
    }

    #[test]
    fn the_cli_reference_documents_every_visible_command() {
        let reference = cli_reference();
        for command in ["validate", "plan", "sync", "export", "doctor", "schema"] {
            assert!(
                reference.contains(&format!("### `gh settings {command}`")),
                "{command} is missing from the CLI reference"
            );
        }
    }

    #[test]
    fn the_cli_reference_hides_internal_commands() {
        // `internal` is a build-time tool, not product surface.
        assert!(!cli_reference().contains("### `gh settings internal`"));
    }

    #[test]
    fn the_cli_reference_records_aliases() {
        let reference = cli_reference();
        assert!(reference.contains("alias: `check`"), "{reference}");
        assert!(reference.contains("alias: `apply`"), "{reference}");
    }

    #[test]
    fn the_cli_reference_documents_flags_and_their_defaults() {
        let reference = cli_reference();
        assert!(reference.contains("--prune"));
        assert!(reference.contains("--no-prune"));
        assert!(reference.contains("--plan <PATH>"));
        // Global options are documented once, not repeated per command.
        assert!(reference.contains("-R, --repo <OWNER/REPO>"));
        assert!(reference.contains("GH_SETTINGS_CONFIG"));
    }

    #[test]
    fn switches_are_not_documented_as_taking_a_value() {
        // clap derives a true/false value parser from the `bool` type, but the
        // action is SetTrue: `--prune true` is rejected by the binary, so
        // documenting it that way would be actively wrong.
        let reference = cli_reference();
        assert!(reference.contains("`--prune`"), "{reference}");
        assert!(!reference.contains("--prune <PRUNE>"), "{reference}");
        assert!(
            !reference.contains("One of: `true`, `false`"),
            "{reference}"
        );
    }

    #[test]
    fn value_taking_options_keep_their_placeholder() {
        let reference = cli_reference();
        assert!(reference.contains("`--plan <PATH>`"));
        assert!(reference.contains("`--format <FORMAT>`"));
        assert!(reference.contains("One of: `text`, `json`"));
    }

    #[test]
    fn descriptions_are_punctuated_before_appended_sentences() {
        // Otherwise: "Output format One of: `text`, `json`."
        assert!(cli_reference().contains("Output format. One of:"));
    }

    #[test]
    fn the_cli_reference_records_the_exit_code_contract() {
        let reference = cli_reference();
        assert!(reference.contains("| `2` | `plan` found pending changes |"));
    }

    #[test]
    fn table_cells_never_contain_a_raw_pipe() {
        // A pipe inside a help string would silently break the Markdown table.
        for rendered in [requirements(), cli_reference()] {
            for line in rendered.lines().filter(|line| line.starts_with("| `")) {
                let escaped = line.replace("\\|", "");
                assert!(
                    escaped.matches('|').count() <= 5,
                    "unescaped pipe in table row: {line}"
                );
            }
        }
    }

    #[test]
    fn joins_lists_readably() {
        let one = vec!["`a`".to_string()];
        let two = vec!["`a`".to_string(), "`b`".to_string()];
        let three = vec!["`a`".to_string(), "`b`".to_string(), "`c`".to_string()];
        assert_eq!(join_with_and(&one), "`a`");
        assert_eq!(join_with_and(&two), "`a` and `b`");
        assert_eq!(join_with_and(&three), "`a`, `b` and `c`");
    }

    #[test]
    fn generation_is_deterministic() {
        // The output is committed and diffed in CI, so instability here would
        // produce spurious failures.
        assert_eq!(requirements(), requirements());
        assert_eq!(cli_reference(), cli_reference());
    }
}
