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

3 to create, 2 to update, 1 to delete.
```

```console
$ gh settings sync --yes
✔ update repository description
✔ add topic rust
✔ create label enhancement
✔ applied 3 changes.
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

`gh-settings` reads existing `safe-settings` files for the sections it supports,
so migrating is not a rewrite. See [ADR-006](docs/adr/006-safe-settings-compatibility.md)
for exactly how far that goes.

---

## 📦 Installation

```sh
gh extension install noirbizarre/gh-settings
```

Or build from source:

```sh
cargo install --git https://github.com/noirbizarre/gh-settings
```

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

---

## 🧩 Supported settings

| Resource | Status |
|---|---|
| Repository metadata, merge & security settings | ✅ |
| Topics | ✅ |
| Labels (including renames) | ✅ |
| Autolinks | ✅ |
| Rulesets | ✅ |
| Custom properties | planned |
| Environments & variables | planned |
| Webhooks, Pages, collaborators | planned |
| Secrets | [out of scope](docs/adr/009-secrets-out-of-scope.md) |

The guiding ambition: *if it is under the repository Settings page, it should
eventually be manageable here.*

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
