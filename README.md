# gh-settings

> Declarative GitHub repository settings for the GitHub CLI.

Make repository configuration behave like infrastructure as code. One
`.github/settings.yml` describes the desired state; `gh settings` computes the
difference and applies it.

No GitHub App. No central service. No webhook. Just the GitHub CLI you already
have.

---

## ✨ What it does

```console
$ gh settings plan

Plan for noirbizarre/gh-settings

Repository
  ~ update repository description

Topics
  + add topic rust
  - remove topic archived

Labels
  + create label enhancement
  ~ update label bug

Autolinks
  ~ recreate autolink OPS- (no update endpoint)

2 to create, 2 to update, 1 to recreate, 1 to delete.
! this plan deletes existing configuration.
```

```console
$ gh settings sync --yes
✔ update repository description
✔ add topic rust
✔ remove topic archived
✔ create label enhancement
✔ update label bug
✔ recreate autolink OPS- (no update endpoint)

✔ applied 6 changes.
```

Run it twice and the second run reports nothing to do. That is the point.

---

## 🚀 Why not safe-settings?

[`safe-settings`](https://github.com/github/safe-settings) is the incumbent, and
it is a fine tool — if you can run a GitHub App.

|  | safe-settings | gh-settings |
|---|---|---|
| GitHub App required | yes | no |
| Central service | yes | no |
| Runs locally | no | yes |
| Preview before applying | no | `gh settings plan` |
| Deletes things you did not list | by default | only when you ask |
| Formal, versioned schema | no | yes |
| Editor completion | no | yes |

`gh-settings` uses the same spelling as `safe-settings` for the sections it
supports, so those need no rewriting. Sections it does not support yet — like
`branches` or `collaborators` — have to be removed first: unknown keys are a
parse error, which is what lets a typo be caught and suggested against. See
[ADR-006](docs/adr/006-safe-settings-compatibility.md) for exactly how far
compatibility goes.

---

## 📦 Installation

```sh
gh extension install noirbizarre/gh-settings
```

That is the only supported install path, deliberately — see
[ADR-014](docs/adr/014-releases.md). Building from source with `cargo build`
produces a standalone `gh-settings` binary rather than a `gh` subcommand, so the
`gh settings …` examples below would not apply to it.

---

## 🧪 Usage

```sh
# Generate a configuration file from a repository you already have
gh settings export

# Check it, without touching the network
gh settings validate

# See what would change
gh settings plan

# Apply it
gh settings sync

# Find out what your token can actually manage
gh settings doctor
```

The repository is inferred from your git remote, exactly as `gh` does. Override
it with `-R owner/repo`.

### Useful flags

| Flag | Effect |
|---|---|
| `--only labels,topics` | Restrict the run to specific resources |
| `--prune` / `--no-prune` | Force deletion of unmanaged items on or off |
| `--dry-run` | Show what `sync` would do, change nothing |
| `--format json` | Machine-readable output |
| `--verbose` | Field-level detail in the plan |
| `--plan plan.json` | Apply a plan saved by `plan --out` |

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success, nothing to do |
| `1` | Failure |
| `2` | `plan` found pending changes |

The distinct code for pending changes lets CI detect drift without treating it
as a build failure.

---

## 🛠 Configuration

```yaml
# $schema: https://noirbizarre.github.io/gh-settings/schema/v1/settings.json
version: 1

repository:
  description: Declarative GitHub repository settings
  homepage: https://noirbizarre.github.io/gh-settings
  has_issues: true
  has_wiki: false
  allow_squash_merge: true
  allow_merge_commit: false
  delete_branch_on_merge: true
  security:
    secret_scanning: true
    secret_scanning_push_protection: true

topics:
  - rust
  - github-cli
  - gh-extension

labels:
  prune: true
  items:
    - name: bug
      color: d73a4a
      description: Something isn't working
    - name: enhancement
      color: a2eeef

autolinks:
  - key_prefix: OPS-
    url_template: https://jira.company.com/browse/<num>
    is_alphanumeric: false

rulesets:
  - name: main-protection
    target: branch
    enforcement: active
    conditions:
      ref_name:
        include: ["~DEFAULT_BRANCH"]
    bypass_actors:
      - team: engineering
        bypass_mode: pull_request
    rules:
      - type: pull_request
        parameters:
          required_approving_review_count: 1
      - type: non_fast_forward
```

Every section is **optional**, and an absent section is **unmanaged** — nothing
is read, diffed or written for it. You can start by managing labels alone and
nothing else will move.

### Nothing is deleted unless you ask

By default the configuration is *additive*. A label that exists on GitHub but is
absent from your file is left alone.

To make a section authoritative, opt in:

```yaml
labels:
  prune: true
  items:
    - name: bug
      color: d73a4a
```

Deletions always appear in the plan as `- delete` lines, and `sync` asks before
performing them. `--no-prune` overrides the file, so you can always force
safety. See [ADR-005](docs/adr/005-prune-opt-in.md).

### Editor support

Add the schema annotation and get completion, validation and hover
documentation:

```yaml
# $schema: https://noirbizarre.github.io/gh-settings/schema/v1/settings.json
```

`gh settings export` writes it for you.

### Sharing configuration across repositories

```yaml
version: 1
extends: acme/.github@v1     # reads .github/settings.yml from acme/.github at v1

labels:
  - name: bug
    color: ff0000           # replaces the inherited `bug` outright
```

Anything the local file declares wins. Collections merge by item identity — a
label of the same name replaces the inherited one **as a whole**, so change one
field and you restate the item.

The ref is required, so a shared file cannot move underneath a plan you already
reviewed. A base may not itself use `extends:`.

**`prune` is never inherited.** Editing a shared file cannot start deleting
things in the repositories that extend it. Note that `sync --prune` on the
command line *does* apply to inherited items — that is a local, explicit
instruction.

Reading a base needs `Contents: read` on **that** repository, which the Actions
`GITHUB_TOKEN` does not have. See
[authentication](docs/authentication.md#inheriting-from-another-repository).

---

## 🔑 Authentication

**Important:** `secrets.GITHUB_TOKEN` **cannot** manage repository settings.

A workflow's `permissions:` block has no `administration` key, so repository
metadata, topics, autolinks and rulesets cannot be granted to it — this is not a
permission you forgot to enable, it cannot be requested at all. Labels are the
exception, since they live under `Issues: write`.

Use a personal access token or a GitHub App installation token:

```yaml
- uses: actions/checkout@v5
- run: gh extension install noirbizarre/gh-settings
- run: gh settings sync --yes
  env:
    GH_TOKEN: ${{ secrets.GH_SETTINGS_TOKEN }}   # NOT secrets.GITHUB_TOKEN
```

In a workflow, use the action rather than wiring up the CLI by hand:

```yaml
- uses: actions/checkout@v5
- uses: noirbizarre/gh-settings@main
  with:
    token: ${{ secrets.GH_SETTINGS_TOKEN }}   # NOT secrets.GITHUB_TOKEN
```

`command: plan` reports drift through a `changed` output instead of failing the
job, and writes the plan to the job summary. See
[docs/actions.md](docs/actions.md).

Run `gh settings doctor` to see what your current credential can manage:

```console
$ gh settings doctor
Environment
  ✔ gh CLI           gh version 2.62.0
  ✔ Authentication   github.com as noirbizarre
  ✔ Token type       classic personal access token
    Scopes           repo, read:org

Resources
  ✔ repository
  ✔ topics
  ✔ labels
  ✔ autolinks
  ✔ rulesets
```

Full details, including the exact scopes each resource needs, are in
[docs/authentication.md](docs/authentication.md).

`sync` checks this before its first request and refuses to start when a change
is *certain* to be rejected, naming the permission instead of letting you find
out through a failed write. It refuses only when it can prove the problem — a
credential it cannot introspect is allowed through, so you get GitHub's error
rather than a guess.

---

## 🧩 Supported settings

| Resource | Status |
|---|---|
| Repository metadata, merge & security settings | ✅ |
| Topics | ✅ |
| Labels (including renames) | ✅ |
| Autolinks | ✅ |
| Rulesets | ✅ |
| Inheritance from a shared repository (`extends:`) | ✅ |

Custom properties, environments, variables, webhooks, Pages and collaborators
are planned; secrets are [deliberately out of
scope](docs/adr/009-secrets-out-of-scope.md).

The guiding ambition: *if it is under the repository Settings page, it should
eventually be manageable here.*

See the **[roadmap](docs/roadmap.md)** for the full picture, including what will
never be supported and why.

---

## 🧠 Design

`gh-settings` is built around a synchronisation engine. Every GitHub feature is
an independent **resource** implementing one trait:

```
load desired state → validate → read current state → diff → plan → apply
```

Adding support for a new setting means writing one module and adding one line to
the registry. The engine never changes.

Decisions, and what they cost, are recorded as
[Architecture Decision Records](docs/adr/). The ones worth reading first:

* [ADR-001](docs/adr/001-resource-abstraction.md) — the resource abstraction
* [ADR-003](docs/adr/003-gh-api-transport.md) — why we shell out to `gh api`
* [ADR-005](docs/adr/005-prune-opt-in.md) — why deletion is opt-in
* [ADR-015](docs/adr/015-token-requirements.md) — how permissions are tracked

---

## 🤝 Contributing

```sh
mise run           # fmt, lint, lint:actions, build, test
mise run test      # cargo nextest
mise run cover     # coverage
mise run snapshots # review insta snapshots
mise run schema    # regenerate the JSON Schema
mise run docs:reference # regenerate the configuration reference
mise run docs      # serve the documentation locally
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

Releases are orchestrated by [gh-ship](https://github.com/noirbizarre/gh-ship):
pushing to `main` maintains a Release PR carrying the version bump and
changelog, and merging it tags, drafts and publishes. See
[ADR-014](docs/adr/014-releases.md).

---

## 📄 License

MIT
