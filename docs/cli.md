# CLI reference

<!-- generated: do not edit below -->

<!-- Generated from the command definitions by `gh-settings internal cli`. Edit `src/cli/`, then run `mise run docs:reference`. -->

Manage GitHub repository settings declaratively from `.github/settings.yml`.

Requires the GitHub CLI for authentication. Note that most settings need a personal access token or GitHub App token: the Actions GITHUB_TOKEN cannot manage repository settings. Run `gh settings doctor` to check.

## Global options

Accepted by every command.

| Option | Description |
|---|---|
| `-R, --repo <OWNER/REPO>` | Repository to act on, as `owner/repo`. Inferred from the git remote when omitted. |
| `-c, --config <PATH>` | Path to the configuration file. Defaults to `.github/settings.yml`, searched for upwards from the current directory. Env: `GH_SETTINGS_CONFIG`. |
| `--only <RESOURCE>` | Limit the run to specific resources. Repeat or comma-separate, e.g. `--only labels,topics`. |
| `--format <FORMAT>` | Output format. One of: `text`, `json`. Defaults to `text`. |
| `-v, --verbose` | Show field-level detail. |
| `--color <WHEN>` | Colourise output. Detected from the terminal by default; `NO_COLOR` is honoured. One of: `auto`, `always`, `never`. |
| `--debug` | Increase log verbosity. Repeat for more. |

## Commands

### `gh settings validate`  <small>(alias: `check`)</small>

Check the configuration file. Contacts GitHub only to read an `extends` base

| Option | Description |
|---|---|
| `--strict` | Treat warnings as errors. |

### `gh settings plan`

Show the changes required to reach the desired state

| Option | Description |
|---|---|
| `--out <PATH>` | Write the plan to a file for `sync --plan` to apply later. |
| `--prune` | Delete items present on GitHub but absent from the configuration. |
| `--no-prune` | Never delete anything, overriding the configuration. |

### `gh settings sync`  <small>(alias: `apply`)</small>

Apply the configuration to the repository

| Option | Description |
|---|---|
| `-y, --yes` | Apply without asking for confirmation. |
| `--plan <PATH>` | Apply a plan previously written by `plan --out`. |
| `--prune` | Delete items present on GitHub but absent from the configuration. |
| `--no-prune` | Never delete anything, overriding the configuration. |
| `--continue-on-error` | Keep going after a failure. |
| `--dry-run` | Show what would happen without changing anything. |

### `gh settings export`

Generate a configuration file from the repository's current state

| Option | Description |
|---|---|
| `--stdout` | Write to standard output instead of a file. |
| `-f, --force` | Overwrite an existing configuration file. |
| `-o, --out <PATH>` | Path to write to. Defaults to `.github/settings.yml`. |

### `gh settings doctor`

Check that the environment can actually manage these settings

| Option | Description |
|---|---|
| `--strict` | Also fail when a capability cannot be determined, not just when it is certainly impossible. |

### `gh settings schema`

Print the JSON Schema for the configuration file

| Option | Description |
|---|---|
| `-o, --output <PATH>` | Write to a file instead of standard output. |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success; nothing to do |
| `1` | Failure |
| `2` | `plan` found pending changes |

The distinct code for pending changes lets CI detect drift without treating it as a build failure.

<!-- /generated -->
