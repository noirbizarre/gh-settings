# gh-settings

Declarative GitHub repository settings for the GitHub CLI.

One `.github/settings.yml` describes the desired state; `gh settings` computes
the difference and applies it. No GitHub App, no central service, no webhook.

## Getting started

```sh
gh extension install noirbizarre/gh-settings

# Generate a file from a repository you already have
gh settings export

# See what would change
gh settings plan

# Apply it
gh settings sync
```

Run `sync` twice and the second run reports nothing to do.

## Read next

* [Authentication](authentication.md) — **start here if you got a `403`**
* [Configuration reference](configuration.md) — every field, generated from the schema
* [CLI reference](cli.md) — every command and flag, generated from the parser
* [GitHub Actions](actions.md) — running it in a workflow
* [Architecture decisions](adr/README.md) — why the tool is built the way it is

## Two things worth knowing up front

**Nothing is deleted unless you ask.** An item that exists on GitHub but is
absent from your file is left alone. Opt in per section with `prune: true`.

**`secrets.GITHUB_TOKEN` cannot manage repository settings.** A workflow's
`permissions:` block has no `administration` key, so it cannot be granted. Use a
personal access token or a GitHub App token. See
[Authentication](authentication.md).
