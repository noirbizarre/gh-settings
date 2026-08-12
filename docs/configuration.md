# Configuration reference

<!--
  Generated from the JSON Schema by scripts/gen-reference.py.
  Do not edit by hand: edit the Rust types and run `mise run docs:reference`.
-->

Every section is optional. **An absent section is unmanaged**: nothing is read,
diffed or written for it. That is what makes adoption incremental — you can
manage labels alone and nothing else will move.

Add this line to get completion and validation in your editor:

```yaml
# $schema: https://noirbizarre.github.io/gh-settings/schema/v1/settings.json
```


## `autolinks`

Autolink references.

May also be written as a bare list, which is the same as giving `items` with `prune: false`.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. Defaults to `false`. |


### `autolinks.items[]`

A single autolink reference.

| Field | Type | Required | Description |
|---|---|---|---|
| `is_alphanumeric` | boolean | no | Whether the reference is alphanumeric rather than purely numeric. |
| `key_prefix` | string | yes | The prefix that triggers the link, for example `OPS-`. |
| `url_template` | string | yes | Target URL, containing the `<num>` placeholder. |


## `extends`

Inherit this configuration from another repository.

Written as `owner/repo[/path/to/file]@ref`. The path is optional and
defaults to `.github/settings.yml`, so:

- `acme/.github@v1` reads `.github/settings.yml` from `acme/.github`
- `acme/.github/config/base.yml@v1` reads that file instead

The ref is required, so a shared base cannot move underneath a plan that
was reviewed against it.

Anything the local file declares wins. Collections are merged by item
identity — a label of the same name replaces the inherited one outright —
and `prune` is never inherited, so editing a shared file cannot start
deleting things in the repositories that extend it.

A base configuration may not itself use `extends`.

Type: string


## `labels`

Issue and pull request labels.

May also be written as a bare list, which is the same as giving `items` with `prune: false`.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. Defaults to `false`. |


### `labels.items[]`

A single label.

| Field | Type | Required | Description |
|---|---|---|---|
| `color` | string | no | Six hexadecimal digits, with or without a leading `#`. Defaults to `ededed`. |
| `description` | string | no | Optional short description, at most 100 characters. |
| `name` | string | yes | Label name. |
| `new_name` | string | no | Rename this label to the given name. |


## `repository`

Repository metadata: description, homepage, features, merge and security
settings.

| Field | Type | Required | Description |
|---|---|---|---|
| `allow_auto_merge` | boolean | no | Whether auto-merge is available on pull requests. |
| `allow_merge_commit` | boolean | no | Whether merge commits are allowed. |
| `allow_rebase_merge` | boolean | no | Whether rebase merging is allowed. |
| `allow_squash_merge` | boolean | no | Whether squash merging is allowed. |
| `allow_update_branch` | boolean | no | Whether updating a pull request branch is allowed. |
| `anonymous_access_enabled` | boolean | no | Whether anonymous Git read access is enabled (GitHub Enterprise only). |
| `archived` | boolean | no | Whether the repository is archived. |
| `default_branch` | string | no | The default branch. |
| `delete_branch_on_merge` | boolean | no | Whether head branches are deleted automatically after merge. |
| `description` | string | no | Short description shown under the repository name. |
| `has_discussions` | boolean | no | Whether the discussions tab is enabled. |
| `has_issues` | boolean | no | Whether the issue tracker is enabled. |
| `has_projects` | boolean | no | Whether repository projects are enabled. |
| `has_wiki` | boolean | no | Whether the wiki is enabled. |
| `homepage` | string | no | Project website shown next to the description. |
| `is_template` | boolean | no | Whether the repository is a template. |
| `merge_commit_message` | `PR_BODY` \| `PR_TITLE` \| `BLANK` | no | Default commit message for merge commits. |
| `merge_commit_title` | `PR_TITLE` \| `MERGE_MESSAGE` | no | Default commit title for merge commits. |
| `private` | boolean | no | Whether the repository is private. |
| `security` | object | no | Security and analysis features. |
| `squash_merge_commit_message` | `PR_BODY` \| `COMMIT_MESSAGES` \| `BLANK` | no | Default commit message for squash merges. |
| `squash_merge_commit_title` | `PR_TITLE` \| `COMMIT_OR_PR_TITLE` | no | Default commit title for squash merges. |
| `topics` | list of string | no | Topics. |


### `repository.security`

Security and analysis features.

| Field | Type | Required | Description |
|---|---|---|---|
| `advanced_security` | boolean | no | Dependency graph advanced security (private repositories). |
| `dependabot_security_updates` | boolean | no | Automatic Dependabot security fixes. |
| `secret_scanning` | boolean | no | Secret scanning. |
| `secret_scanning_push_protection` | boolean | no | Secret scanning push protection. |
| `secret_scanning_validity_checks` | boolean | no | Secret scanning validity checks. |


## `rulesets`

Repository rulesets.

May also be written as a bare list, which is the same as giving `items` with `prune: false`.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. Defaults to `false`. |


### `rulesets.items[]`

A repository ruleset.

| Field | Type | Required | Description |
|---|---|---|---|
| `bypass_actors` | list of object | no | Who may bypass it. |
| `conditions` | object | no | Which refs it applies to. |
| `enforcement` | `disabled` \| `active` \| `evaluate` | no | How strictly it is applied. Defaults to `active`. |
| `name` | string | yes | Ruleset name. |
| `rules` | list of object | no | The rules themselves. |
| `target` | `branch` \| `tag` \| `push` | no | What the ruleset applies to. Defaults to `branch`. |


## `topics`

Repository topics.

Also accepted under `repository.topics` for `safe-settings`
compatibility; declaring both is an error.

May also be written as a bare list, which is the same as giving `items` with `prune: false`.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of string | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. Defaults to `false`. |


## `version`

Schema major version this file targets.

Type: integer
