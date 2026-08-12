# Roadmap

Where `gh-settings` is, where it is going, and what it will deliberately never
do. Decisions already settled live in the [architecture decision
records](adr/README.md); this page tracks what is left.

The guiding ambition: *if it is under the repository Settings page, it should
eventually be manageable here.*

---

## Supported settings

| Resource | Status | Notes |
|---|---|---|
| Repository metadata | ✅ | description, homepage, features, merge strategies, commit title/message, default branch |
| Repository security | ✅ | secret scanning, push protection, Dependabot updates, advanced security |
| Topics | ✅ | |
| Labels | ✅ | including renames, which preserve issue assignments |
| Autolinks | ✅ | changes are delete-and-recreate; GitHub has no update endpoint |
| Rulesets | ✅ | unknown rule types round-trip untouched rather than being dropped |
| Inheritance (`extends:`) | ✅ | single level, ref pinned, `prune` never inherited |
| Environments | ✅ | protection rules, reviewers and deployment branch policies |
| Actions variables | ✅ | repository and environment scope; values are readable, so they diff |
| Custom properties | planned | |
| Webhooks | planned | |
| Pages | planned | |
| Collaborators & teams | planned | |
| Branch protection (legacy) | planned | rulesets are the modern equivalent and already supported |
| Repository interactions | planned | |
| Secrets | ✖ never | [ADR-009](adr/009-secrets-out-of-scope.md) — values are write-only, so they cannot be diffed, exported, or made idempotent |

Every section of the configuration file is optional, and **an absent section is
unmanaged**. Adoption is incremental: you can manage labels alone and nothing
else will move.

---

## Next

Roughly in priority order. Each entry says *why*, so it can be dropped when the
reason stops applying.

### Verification

- [x] **A live test suite** against a real throwaway repository, `#[ignore]`d
      and gated on `GH_SETTINGS_TEST_REPO`. Nine tests covering each resource's
      create → update → prune cycle, re-planning after every mutation. Runs
      nightly and on demand; refuses to start against a repository that already
      has managed configuration. Run it yourself with
      `GH_SETTINGS_TEST_REPO=you/sandbox mise run test:live` — the sandbox must
      be **public**, since a private repository on the free plan answers
      `403 Upgrade to GitHub Pro` for rulesets.
- [x] **Ruleset apply-path tests through the stub.** Create, update, delete and
      prune, asserting the request log — including that an update addresses the
      server id rather than the name, which nothing else checked, and that
      server-only fields never travel back.
- [x] **Repository security PATCH test** — covered by the live suite.
- [x] **Generic idempotency contract** — the live suite re-plans after every
      mutation and asserts the plan is empty, for every resource. Checked
      against reality rather than against our own fixtures, which is the only
      version of this assertion that would have caught the ruleset permanent
      diff.
- [x] **Snapshots for `plan`, `doctor` and `export`.** Including `plan
      --verbose`, which is the reason there is no separate `diff` command, and
      `doctor`'s "unknown" verdict. The shared `assert_cli_snapshot!` applies the
      stabilising filters, because forgetting them fails only on someone else's
      machine.
- [x] **`--continue-on-error` integration test.** Exercised through the binary:
      every change attempted, partial success preserved, and every failure
      reported in the JSON output rather than just the first.

### Behaviour

Two span bugs were found while preparing this and fixed first: ruleset findings
underlined whichever rule sorted into the position rather than the one the user
wrote, and a configuration using only `repository.topics` panicked a debug build.

`Requirement::verdict` in `src/resources/requirement.rs` is the single place a
credential is judged. `doctor` renders it and `sync` refuses on it, so the two
cannot disagree about what a token can do.

- [x] **`sync` pre-flight permission check.** Refuses before the first request
      when a change is certain to be rejected, naming the permission. Stays
      conservative: only `Capability::Impossible` blocks, and an
      unintrospectable token proceeds so the real error can speak. There is no
      flag to overrule a refusal, which is why the bar for making one is proof.
- [x] **Attach the requirement table to a `403`.** Per failing resource, so a
      failed label write no longer points at `Administration: write`. The
      GITHUB_TOKEN note appears only inside Actions, where it is the answer
      rather than a false lead.
- [ ] **Help with ruleset rule parameters.** GitHub requires *all* parameters of
      a rule, not the subset you want to change: a `pull_request` rule missing
      one field is rejected with `Invalid property /rules/1: data matches no
      possible input`, which names neither the rule nor the field. Validation
      could name the rule from the index, and list the parameters a known rule
      type expects.

### Features

- [x] **A composite action** — `uses: noirbizarre/gh-settings@main` until the
      first release carries it, `@v1` after. Maps exit
      code 2 to a `changed` output rather than a failed job, writes the plan to
      the job summary, and annotates a 403 with the token explanation. See
      [GitHub Actions](actions.md).

- [x] **Inheritance (`extends:`)** — share labels, autolinks and rulesets across
      repositories. See [ADR-017](adr/017-inheritance.md).

      Collections replace the whole item by identity rather than merging field
      by field: `Label::color` and `Ruleset::target`/`enforcement` are not
      `Option`, so a parsed document cannot distinguish an omitted field from
      one written to its default, and a field-wise merge would repaint an
      inherited label grey the moment a child named it.

      `prune` never inherits. The ref is required. A base may not itself
      extend. Reading a base needs `Contents: read` on the other repository,
      which the Actions `GITHUB_TOKEN` does not have — `doctor` says so.

      Still open: making those three fields `Option` would allow field-wise
      merging, and is the way to revisit the trade-off above.

### Documentation

- [ ] Installation page with per-platform notes and upgrading.
- [ ] Quick start: export → validate → plan → sync on an existing repository.
- [ ] Migration guide from `safe-settings` — what is read, what has to be
      removed before the file will parse (org-level `suborgs`/`overrides`,
      `branches`, `collaborators`), and above all that **pruning is off by
      default**, so the first run is non-destructive. The README and the index
      both say so; what is missing is the step-by-step.
- [ ] FAQ: why not safe-settings, why no secrets, why a `403`, why a PAT in CI.

### Infrastructure

- [x] Harden `${{ inputs.tag }}` interpolation in `publish-release.yaml` by
      passing through `env:`. Only reachable by someone who already has write
      access, so defence in depth rather than a hole.

---

## Not planned, and why

These are settled. Reopening one means superseding its record, not re-arguing
it.

| | Decision |
|---|---|
| **Secrets** | [ADR-009](adr/009-secrets-out-of-scope.md) — write-only values cannot be diffed, exported or made idempotent. Use `gh secret set`. |
| **Org-level `suborgs` / `overrides`** | [ADR-006](adr/006-safe-settings-compatibility.md) — `safe-settings` compatibility is one-way and per-repository. Sharing configuration between repositories now exists as [`extends:`](adr/017-inheritance.md), on its own merits rather than as compatibility; `suborgs` and `overrides` themselves remain out of scope. |
| **A `diff` command** | `plan --verbose` already shows field-level before/after; a second command for the same output is surface without substance. |
| **A typed enum of ruleset rules** | GitHub ships new rule types faster than any client tracks them. The untyped passthrough is deliberate: an unrecognised rule round-trips untouched rather than being silently dropped on the next sync. |
| **GraphQL** | [ADR-004](adr/004-rest-first.md) — autolinks, topics, rulesets, environments and custom properties have no GraphQL mutations at all. |
| **Publishing to crates.io** | [ADR-014](adr/014-releases.md) — `gh-settings` is distributed as a GitHub CLI extension; a second install path is a second thing to keep working. |

---

## Manual verification checklist

Automated tests assert the *shape* of the requests we send. They cannot assert
that GitHub *accepts* them, so these paths need a human and a throwaway
repository. Several are destructive.

- [x] Add a topic
- [x] `export`
- [x] `plan`
- [x] `sync`
- [x] `export` → `plan` reports zero changes
- [x] Sync a **ruleset** with rules and `conditions.ref_name` — found a `422`;
      GitHub requires *every* parameter of a `pull_request` rule, not a subset
- [x] Update that ruleset (change a rule parameter) and re-sync — exercises
      `PUT /rulesets/{id}` and canonical rule ordering
- [ ] A ruleset with `bypass_actors: [{ team: … }]` **on an organisation
      repository** — slug-to-id resolution; impossible to test on a personal repo
- [x] `repository.security.secret_scanning: true`
- [x] Change an autolink's `url_template` and re-sync — the recreate path
- [x] `sync --prune` after removing a label — the destructive path and its prompt
- [x] Run any `sync` **twice**; the second must report "up to date" — this is
      what caught the ruleset permanent diff
- [ ] `doctor` inside GitHub Actions with `secrets.GITHUB_TOKEN` — should report
      labels-only and say why
- [ ] `gh extension install noirbizarre/gh-settings` on a clean machine

A `422` means a payload-shape bug, and GitHub will not say which field.
