# Authentication

`gh-settings` never asks you to create, install or operate a GitHub App, and
there is no central service. It uses whatever credential the GitHub CLI is
holding.

That does **not** mean any credential will do. This page is the definitive
answer to *"why did that return 403?"*.

Run `gh settings doctor` at any time to see what your current credential can and
cannot manage.

---

## Quick answer

| Where | Credential | Works? |
|---|---|---|
| Your machine | `gh auth login` | ✅ everything |
| GitHub Actions | `secrets.GITHUB_TOKEN` | ⚠️ **labels only** |
| GitHub Actions | a PAT in a secret | ✅ everything |
| GitHub Actions | a GitHub App installation token | ✅ everything (optional) |

---

## Why `secrets.GITHUB_TOKEN` is not enough

A workflow's `permissions:` block can request exactly these permissions:

```
actions, attestations, checks, contents, deployments, discussions, id-token,
issues, models, packages, pull-requests, repository-projects, security-events,
statuses, pages
```

**There is no `administration` key.** It cannot be requested at any value.

Repository metadata, topics, autolinks and rulesets all require
`Administration: write`. They are therefore *structurally* unavailable to
`GITHUB_TOKEN` — this is not a permission you forgot to enable, it cannot be
granted at all.

Labels are the exception: they fall under `Issues: write`, which `GITHUB_TOKEN`
can hold.

### The working Actions setup

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

      - run: gh extension install noirbizarre/gh-settings

      - run: gh settings sync --yes
        env:
          # A PAT or GitHub App token — NOT secrets.GITHUB_TOKEN.
          GH_TOKEN: ${{ secrets.GH_SETTINGS_TOKEN }}
```

### The labels-only setup

If labels are all you need, the built-in token is enough and you can skip
managing a secret entirely:

```yaml
jobs:
  labels:
    runs-on: ubuntu-latest
    permissions:
      issues: write
    steps:
      - uses: actions/checkout@v5
      - run: gh extension install noirbizarre/gh-settings
      - run: gh settings sync --yes --only labels
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### Using a GitHub App anyway

A GitHub App installation token works and avoids a long-lived PAT. It is an
option, never a requirement:

```yaml
      - uses: actions/create-github-app-token@v2
        id: token
        with:
          app-id: ${{ vars.APP_ID }}
          private-key: ${{ secrets.APP_PRIVATE_KEY }}

      - run: gh settings sync --yes
        env:
          GH_TOKEN: ${{ steps.token.outputs.token }}
```

The App needs the repository permissions listed below.

---

## Fine-grained personal access tokens

| Permission | Level | Needed for |
|---|---|---|
| **Metadata** | Read | Mandatory baseline for every fine-grained token |
| **Administration** | Read & write | `repository`, `topics`, `autolinks`, `rulesets` |
| **Issues** | Read & write | `labels` |
| *Organization → Members* | Read | Resolving `bypass_actors: [{ team: … }]` (organisation repositories only) |

Fine-grained tokens do not report their permissions through the API. `doctor`
will therefore say **unknown** rather than guessing, and fall back to probing
whether the token has admin rights on the repository.

## Classic personal access tokens

| Scope | Needed for |
|---|---|
| `repo` | Everything |
| `read:org` | Resolving team slugs in ruleset `bypass_actors` |
| `admin:org` | Organisation-level rulesets and teams (not yet supported) |

`workflow` and `delete_repo` are **never** required.

Classic tokens *do* report their scopes, so `doctor` can be exact about them.

---

## Per-resource requirements

These are declared in the code, on each resource, and this table is generated
from that same declaration — so it cannot drift from what the tool actually
enforces.

| Resource | Fine-grained | Classic | Works with `GITHUB_TOKEN` |
|---|---|---|---|
| `repository` | Metadata: read, Administration: write | `repo` | ✘ |
| `topics` | Metadata: read, Administration: write | `repo` | ✘ |
| `labels` | Metadata: read, Issues: write | `repo` | ✔ |
| `autolinks` | Metadata: read, Administration: write | `repo` | ✘ |
| `rulesets` | Metadata: read, Administration: write | `repo` | ✘ |

---

## GitHub Enterprise Server

Authentication, host selection and pagination are all inherited from the GitHub
CLI, so `gh auth login --hostname github.example.com` is all that is required.

---

## Troubleshooting

**`HTTP 403: Resource not accessible by integration`**
You are almost certainly using `secrets.GITHUB_TOKEN`. See above.

**`HTTP 403` with a personal access token**
Your token is missing `repo` (classic) or `Administration: write`
(fine-grained). Run `gh settings doctor`.

**`HTTP 404` on a repository you can see in the browser**
A fine-grained token only covers repositories it was explicitly granted. Check
the token's repository access list.

**`no team named X could be found`**
Ruleset bypass actors need `read:org` (classic) or organisation `Members: read`
(fine-grained) to resolve a team slug to an identifier.
