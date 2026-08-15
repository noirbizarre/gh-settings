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
| GitHub Actions | `secrets.GITHUB_TOKEN` | ⚠️ **labels and Pages only** |
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

Repository metadata, topics, autolinks, rulesets, environments and Actions
general settings all require `Administration: write`; Actions variables require
`Variables: write`, and the ones nested under an environment require
`Environments: write`. None of those keys exists in the list above, so they are
*structurally* unavailable to `GITHUB_TOKEN` — this is not a permission you
forgot to enable, it cannot be granted at all.

Labels and Pages are the exceptions. Labels fall under `Issues: write`, and
`pages` is in the list above — both are permissions `GITHUB_TOKEN` can hold.

Pages is a genuine oddity, and the table below shows it: a *fine-grained token*
needs `Administration: write` as well as `Pages: write`, which GitHub states
outright in the `X-Accepted-GitHub-Permissions` header for those endpoints. The
Actions token is a separate permission system, where `pages: write` alone is
enough — it is what `actions/configure-pages` uses. The two are not
contradictory; they are different credentials answering to different rules.

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

### The labels-and-Pages setup

If labels and Pages are all you need, the built-in token is enough and you can
skip managing a secret entirely:

```yaml
jobs:
  settings:
    runs-on: ubuntu-latest
    permissions:
      issues: write
      pages: write
    steps:
      - uses: actions/checkout@v5
      - run: gh extension install noirbizarre/gh-settings
      - run: gh settings sync --yes --only labels,pages
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

Request only the permissions you actually use: a job that manages labels alone
needs `issues: write` and nothing more.

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
| **Administration** | Read & write | `repository`, `topics`, `autolinks`, `rulesets`, `environments`, `actions`, and `pages` |
| **Issues** | Read & write | `labels` — or `Pull requests`, either is accepted |
| **Pages** | Read & write | `pages`, alongside `Administration: write` |
| **Variables** | Read & write | `variables` at repository scope |
| **Environments** | Read & write | `variables` nested under an environment; reading them back on `export` |
| **Actions** | Read | Listing the repository's environments, which both `environments` and `variables` start from |
| *Actions policies* | Read & write | `actions` — see the note below |
| **Contents** | Read | Reading a base named in `extends:` — **on that repository**, not this one |
| *Organization → Members* | Read | Resolving `bypass_actors: [{ team: … }]` and environment reviewers named by team (organisation repositories only) |

These categories do **not** nest ([ADR-020](adr/020-permission-categories-do-not-nest.md)).
`Administration: write` lets you create an environment but not *list* the
environments — that read is `Actions: read` — and it does not cover an
environment's variables either. A token granted only what it seems to need will
fail on the read before it ever attempts the write.

Every mapping in this table was confirmed against the
`X-Accepted-GitHub-Permissions` response header, which GitHub sends to
fine-grained tokens to say what an endpoint actually requires. The live suite
re-checks them, so they cannot quietly drift.

*Actions policies* is the exception, and it is italicised above for that reason.
It is the name GitHub's documentation gives for the endpoints behind
`actions` — artifact and log retention, fork pull request approval — and it
appears in none of GitHub's published permission tables. It is **not** the same
thing as the `Actions` permission listed above; ADR-020 applies. Until the live
suite settles it, `doctor` reports **unknown** for it rather than claiming
either way. In practice a classic token with `repo`, or a fine-grained token
with `Administration: write`, has managed these settings in testing.

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

Each resource declares its own requirements in code. The table below is
generated from those declarations by `gh settings internal requirements`, and CI
fails if the committed copy is stale — so it cannot drift from what the tool
actually enforces.

<!-- generated: do not edit below -->

| Resource | Fine-grained | Classic | Works with `GITHUB_TOKEN` |
|---|---|---|---|
| `repository` | Metadata: read, Administration: write | `repo` | ✘ |
| `topics` | Metadata: read, Administration: write | `repo` | ✘ |
| `labels` | Metadata: read, Issues: write | `repo` | ✔ |
| `autolinks` | Metadata: read, Administration: write | `repo` | ✘ |
| `rulesets` | Metadata: read, Administration: write | `repo` | ✘ |
| `environments` | Metadata: read, Actions: read, Environments: read, Administration: write | `repo` | ✘ |
| `actions` | Metadata: read, Administration: write, Actions policies: write † | `repo` | ✘ |
| `variables` | Metadata: read, Actions: read, Variables: write, Environments: write | `repo` | ✘ |
| `pages` | Metadata: read, Pages: write, Administration: write | `repo` | ✔ |
| `extends` | Contents: read | `repo` | ✘ |

† GitHub's own reference does not settle this mapping — it is either absent or ambiguous there, so this is our best understanding and the minimal claim, not a guarantee. `gh settings doctor` will tell you what your token can actually do.

`repository`, `topics`, `autolinks`, `rulesets`, `environments` and `actions` require Administration: write, which cannot be granted to GITHUB_TOKEN — the workflow `permissions:` block has no key that grants it. Use a personal access token or a GitHub App token.

`variables` requires Variables: write, which cannot be granted to GITHUB_TOKEN — the workflow `permissions:` block has no key that grants it. Use a personal access token or a GitHub App token.

`extends` is not a resource — it is read while loading the configuration — and it requires Contents: read on the *other* repository, which GITHUB_TOKEN does not have.

<!-- /generated -->

---

## Inheriting from another repository

`extends: acme/.github@v1` reads a configuration from a different repository,
which needs **`Contents: read` on that repository** — not on the one being
configured.

The Actions `GITHUB_TOKEN` cannot do this. It is scoped to the repository
running the workflow, so it cannot read a base held anywhere else, and no
`permissions:` block changes that. This is the same dead end as
`Administration: write`, and `gh settings doctor` reports it.

| Credential | Can inherit? |
|---|---|
| `gh auth login` on your machine | ✅ |
| Classic token with `repo` | ✅ |
| Fine-grained token | ✅ if the base repository is in its access list |
| GitHub App installation token | ✅ if the app is installed on the base repository |
| Actions `secrets.GITHUB_TOKEN` | ✖ never |

A configuration that does not use `extends:` still needs no credentials at all
to `validate`.

---

## What `sync` checks before it writes

`sync` consults the table above before making its first request, and refuses to
start when a change is *certain* to be rejected — naming the permission rather
than letting you discover it through a failed write.

It only refuses when it can prove the problem:

| Credential | Behaviour |
|---|---|
| Classic token missing `repo` | **Refused.** Scopes are advertised, so this is a fact. |
| Actions `GITHUB_TOKEN` on an `Administration: write` resource | **Refused.** No `permissions:` block can grant it. |
| Fine-grained or App token | **Allowed to proceed**, unless the repository read shows it has no admin rights. |
| Credential that could not be introspected | **Allowed to proceed.** |

That last row is deliberate. A token we cannot understand is not a token we know
to be insufficient, and there is no flag to overrule a refusal — so when in
doubt `sync` lets GitHub answer, and you get GitHub's error rather than our
guess about it.

When a write *is* refused, the failure names the permission that was missing,
for that specific resource.

---

## GitHub Enterprise Server

Authentication, host selection and pagination are all inherited from the GitHub
CLI, so `gh auth login --hostname github.example.com` is all that is required.

---

## Troubleshooting

**`HTTP 403: Resource not accessible by integration`**
You are almost certainly using `secrets.GITHUB_TOKEN`. See above.

**`Refusing to start: this token cannot make some of these changes`**
The pre-flight check proved the write would fail, so nothing was attempted. The
message names the resource and the missing permission; grant it and re-run.

**`HTTP 403` with a personal access token**
Your token is missing `repo` (classic) or `Administration: write`
(fine-grained). The error names the permission for the resource that failed.

**`HTTP 403` or `404` while *reading* environments or variables**
Not the write permission — the read. Listing environments is `Actions: read`,
and an environment's variables are `Environments`, neither of which
`Administration: write` includes. See the table above.

**`HTTP 404` on a repository you can see in the browser**
A fine-grained token only covers repositories it was explicitly granted. Check
the token's repository access list.

**`could not read the base configuration`**
The repository being configured is fine — the one that cannot be read is the
one named in `extends:`. Check the reference, the ref, and that your token can
read that repository. Inside Actions, `secrets.GITHUB_TOKEN` cannot read
another repository at all.

**`no team named X could be found`**
Ruleset bypass actors need `read:org` (classic) or organisation `Members: read`
(fine-grained) to resolve a team slug to an identifier.
