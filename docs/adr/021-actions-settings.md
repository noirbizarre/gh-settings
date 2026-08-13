# 21. Actions general settings are one section over seven endpoints

## Status

Accepted.

## Context

GitHub's *Settings → Actions → General* page is one screen. The REST API exposes
it as seven sibling endpoints under `/repos/{owner}/{repo}/actions/permissions`:
the policy itself, the allowed-actions list, workflow token defaults, artifact
and log retention, fork pull request approval, the outside access level, and
fork pull request behaviour on private repositories. Each has its own `GET` and
its own `PUT`, and each `PUT` rejects a body carrying another's fields.

Three of them do not always exist. `/access` and
`/fork-pr-workflows-private-repos` are private-repository features and answer
`404` on a public one; artifact retention can be locked by an enterprise owner
and answer `403`.

Two questions followed: where the configuration for this belongs, and what to do
about a setting the configuration declares but the API will not talk about.

## Decision

**One top-level `actions:` section, one resource, seven changes.**

The configuration follows the screen, not the API. Field names are the API's
own, so there is no mapping table to keep in step, and the only nesting is where
the API itself groups — `selected_actions` and
`fork_pr_workflows_private_repos`. The resource does the fanning out, and a
change is keyed by the endpoint suffix it will be sent to, which is all `apply`
needs to know where a body goes.

Rejected: **nesting it under `repository:`**. It reads well — the web UI puts
both under Settings — but `repository:` is one `PATCH` and this is seven `PUT`s,
so the section would stop meaning "the fields of one request". ADR-018 permits a
resource to span sections; it does not ask two unrelated request shapes to share
one.

Rejected: **one section per endpoint** (`actions_permissions:`,
`actions_workflow:`, …). Honest about the API and unusable as a description of
intent. Nobody thinks of retention and fork approval as different subsystems.

**A declared setting we could not read still produces a change.**

Reporting "up to date" for a group whose `GET` returned `404` would claim a
convergence that has not happened. Emitting the change means `apply` sends the
`PUT` and GitHub's own `404` reaches the user, which says far more than our
silence would. The alternative — dropping the change quietly — is the failure
mode this codebase exists to avoid: a setting accepted by the schema, published
in the docs, and then never applied.

A `403` is deliberately *not* absorbed the same way. An enterprise lock and a
token missing a permission are indistinguishable from here, and swallowing the
second would turn a credential problem into a silent no-op.

## Consequences

* A change body is never mixed, so the API cannot reject one for containing a
  field that belongs elsewhere.
* Running `plan` against a public repository with `access_level:` declared shows
  a change that will always be there. That is correct — the setting cannot be
  applied — and the failure at `sync` names the reason.
* `PUT /actions/permissions` requires `enabled`, and
  `/fork-pr-workflows-private-repos` requires
  `run_workflows_from_fork_pull_requests`. When the configuration leaves the
  required field unmanaged, it is filled from the current state: preserving it,
  rather than defaulting it and silently turning Actions off. When the current
  state is unknown too, the body goes out without it, because inventing a value
  for a required field we cannot read is the guess this codebase does not make.
* `maximum_allowed_days` is reported by the retention endpoint and is neither
  diffed nor exported. It describes the plan, not the repository, and is not a
  body parameter.
* The permission mapping could not be settled from GitHub's published tables.
  See [ADR-020](020-permission-categories-do-not-nest.md) and the `unverified`
  entry in `Requirement::ACTIONS`; the live suite probes all seven endpoints.
* Resetting a sandbox now has to reset Actions settings explicitly. They have no
  `prune` — every one has a value, never an existence — so neither the
  configuration format nor the live pre-flight can see them
  ([ADR-019](019-live-test-sandboxes.md)).
