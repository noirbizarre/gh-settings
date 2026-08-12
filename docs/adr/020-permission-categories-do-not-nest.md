# 20. Fine-grained permission categories do not nest

## Status

Accepted. Extends [015](015-token-requirements.md).

## Context

`Requirement::ADMINISTRATION` was shared by every resource that writes through
an admin-only endpoint, on the assumption that one resource maps to one
permission. Checking each endpoint we call against GitHub's reference showed
that assumption is false, and that two declarations were wrong because of it:

* `GET /repos/{owner}/{repo}/environments` — the read that every environment
  plan starts with — is **Actions: read**. `Administration: write` does not
  include it. A token granted exactly what the old table demanded could create
  an environment but not list one.
* `/repos/{owner}/{repo}/environments/{name}/variables` is **Environments**,
  not **Variables**. The scope decides the permission, not the payload; we had
  declared both scopes as `Variables: write`.

The mistake in both cases was the same: assuming a permission that grants the
expensive operation also grants the cheap one, or that a resource named after a
thing is governed by the permission named after that thing.

## Decision

A `Requirement` lists **every** permission the resource's endpoints need,
including read permissions in categories it does not write, and including the
same category at two levels where that is what the reference says. Four entries
for one resource is normal, not a smell.

Requirements are derived per *endpoint*, from GitHub's published table, not per
resource by analogy with a resource that looks similar.

Where the reference does not settle a mapping — several endpoints are listed
under two permissions with a marker that means either "both" or "either" — the
minimal sufficient claim is declared and marked `Confidence::Unverified`, which
carries a footnote into the docs and an admission into `doctor`. `pages` is
currently the only one.

## Consequences

* The published permission table is longer and less tidy. It is also correct,
  and the tidiness was costing users a 403 on a read.
* `environments` needs its own `Requirement` rather than sharing
  `ADMINISTRATION`, and any future resource touching environments will too.
* Two resources reading the same endpoint must each declare the permission it
  needs. `environments` and `variables` both list environments, so both carry
  `Actions: read`; neither can rely on the other having asked.
* The `github_token_note` strings are load-bearing: the docs generator groups
  resources by exact string equality to build one sentence instead of five.
  `ENVIRONMENTS` repeats `ADMINISTRATION`'s note verbatim for that reason.
* Settling an ambiguous mapping needs a **fine-grained** token and the
  `X-Accepted-GitHub-Permissions` response header. A classic token gets
  `X-Accepted-OAuth-Scopes` instead and cannot answer the question.
* That header's syntax is the opposite of the obvious reading, and we got it
  wrong once: a **comma** joins permissions that are *all* required, and a
  **semicolon** separates alternative sets. `pages=write,administration=write`
  means both; `issues=read; pull_requests=read` means either. Assuming the
  intuitive reading turns every finding inside out.
* The header also uses wire names, not the names in the token UI: repository
  variables are `actions_variables` there and "Variables" everywhere a human
  looks.
* That check is a live test rather than a chore someone remembers:
  `live_declared_permissions_match_what_github_accepts` asks GitHub what each
  endpoint accepts and fails if a declaration does not cover it. It skips on a
  classic token, so it cannot be the only guard — the unit tests in
  `src/resources/requirement.rs` still pin the mappings offline.
* Its first run paid for itself: it found that the Pages writes need
  `Administration: write` as well as `Pages: write`, which no amount of reading
  the published table would have settled.
* A declared requirement and `github_token_capable` can legitimately disagree.
  Pages needs `Administration: write` as a fine-grained token yet remains
  manageable with the Actions token, because that is a different permission
  system with a `pages` key of its own. When in doubt the flag stays `true`: a
  false refusal cannot be overruled, while a false permission is merely a 403
  from GitHub with an explanation attached.
