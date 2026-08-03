# GitHub Actions

The action installs the extension and runs it, so automating this does not mean
hand-writing `gh extension install` in every repository.

```yaml
- uses: noirbizarre/gh-settings@v1
  with:
    token: ${{ secrets.GH_SETTINGS_TOKEN }}
```

!!! warning "The default token is not enough"

    `token` defaults to the workflow's own `GITHUB_TOKEN`, which can manage
    **labels and nothing else**. Repository settings, topics, autolinks and
    rulesets need `Administration: write`, and the workflow `permissions:`
    block has no `administration` key — it cannot be granted at all.

    Use a personal access token or a GitHub App installation token. See
    [Authentication](authentication.md).

## Keeping settings applied

Sync whenever the configuration changes:

```yaml
name: Repository settings

on:
  push:
    branches: [main]
    paths: ['.github/settings.yml']

jobs:
  sync:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: noirbizarre/gh-settings@v1
        with:
          token: ${{ secrets.GH_SETTINGS_TOKEN }}
```

## Detecting drift without changing anything

`plan` exits 2 when the repository differs from the configuration. The action
turns that into the `changed` output rather than a failed job, so drift is
something you can branch on.

```yaml
      - uses: noirbizarre/gh-settings@v1
        id: settings
        with:
          command: plan
          token: ${{ secrets.GH_SETTINGS_TOKEN }}

      - if: steps.settings.outputs.changed == 'true'
        run: echo "The repository has drifted from .github/settings.yml"
```

The plan is also written to the job summary, so a reviewer sees what would
change without opening the logs.

## Checking a pull request

`validate` needs no network, no repository and no credentials, which makes it
safe on pull requests from forks:

```yaml
on: pull_request

jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: noirbizarre/gh-settings@v1
        with:
          command: validate
```

## Labels only, with no secret

If labels are all you need, the built-in token suffices and you can skip
managing a secret:

```yaml
    permissions:
      issues: write
    steps:
      - uses: actions/checkout@v5
      - uses: noirbizarre/gh-settings@v1
        with:
          only: labels
```

## Diagnosing a token

```yaml
      - uses: noirbizarre/gh-settings@v1
        with:
          command: doctor
          token: ${{ secrets.GH_SETTINGS_TOKEN }}
```

Prints what the credential can and cannot manage, and says so honestly when it
cannot tell — a fine-grained token does not report its scopes.

## Inputs

<!-- generated: do not edit below -->

| Input | Default | Description |
|---|---|---|
| `command` | `sync` | What to run: sync, plan, validate, export or doctor. `plan` reports drift without changing anything and sets `changed`. |
| `token` | `${{ github.token }}` | Token used to talk to GitHub. The default is the workflow's own GITHUB_TOKEN, which is enough for `validate` and for labels — and nothing else. Repository settings, topics, autolinks and rulesets need `Administration: write`, which the workflow `permissions:` block cannot grant at all, because it has no `administration` key. For those, supply a personal access token or a GitHub App installation token. Run this action with `command: doctor` to see what a token can manage. See https://noirbizarre.github.io/gh-settings/authentication/ |
| `repository` | `${{ github.repository }}` | Repository to act on, as owner/repo. |
| `config` |  | Path to the configuration file. Defaults to .github/settings.yml. |
| `only` |  | Limit the run to specific resources, comma separated (e.g. labels,topics). |
| `prune` |  | Delete items present on GitHub but absent from the configuration. Overrides the file in both directions; leave unset to honour it. |
| `dry_run` | `false` | Show what sync would do without changing anything. |
| `verbose` | `false` | Include field-level detail in the plan and the job summary. |
| `version` | `latest` | Version of the extension to install, e.g. `0.1.0`. Defaults to the latest release. Pin it for reproducible workflows. |
| `summary` | `true` | Write the plan to the job summary. |

<!-- /generated -->

## Outputs

| Output | Description |
|---|---|
| `changed` | `true` when the repository differs from the configuration. From exit code 2 for `plan`; from what was applied for `sync`. |
| `counts` | JSON object of create/update/delete/recreate counts. |
| `json` | The full JSON output of the command. |
| `success` | Whether every change applied cleanly. `sync` only. |

## Pinning

`version` defaults to the latest release. Pin it for reproducible workflows:

```yaml
      - uses: noirbizarre/gh-settings@v1
        with:
          version: "0.1.0"
```
