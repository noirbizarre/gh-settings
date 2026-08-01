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
# yaml-language-server: $schema=https://gh-settings.dev/schema/v1/settings.json
```


## `autolinks`

Autolink references.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. |


### `autolinks.items[]`

A single autolink reference.

| Field | Type | Required | Description |
|---|---|---|---|
| `is_alphanumeric` | boolean | no | Whether the reference is alphanumeric rather than purely numeric. |
| `key_prefix` | string | yes | The prefix that triggers the link, for example `OPS-`. |
| `url_template` | string | yes | Target URL, containing the `<num>` placeholder. |


## `labels`

Issue and pull request labels.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. |


### `labels.items[]`

A single label.

| Field | Type | Required | Description |
|---|---|---|---|
| `color` | string | no | Six hexadecimal digits, with or without a leading `#`. |
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
| `merge_commit_message` | string | no | Default commit message for merge commits. |
| `merge_commit_title` | string | no | Default commit title for merge commits. |
| `private` | boolean | no | Whether the repository is private. |
| `security` | object | no | Security and analysis features. |
| `squash_merge_commit_message` | string | no | Default commit message for squash merges. |
| `squash_merge_commit_title` | string | no | Default commit title for squash merges. |
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

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of object | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. |


### `rulesets.items[]`

A repository ruleset.

| Field | Type | Required | Description |
|---|---|---|---|
| `bypass_actors` | list of object | no | Who may bypass it. |
| `conditions` | object | no | Which refs it applies to. |
| `enforcement` | string | no | How strictly it is applied. |
| `name` | string | yes | Ruleset name. |
| `rules` | list of object | no | The rules themselves. |
| `target` | string | no | What the ruleset applies to. |


## `topics`

Repository topics.

Also accepted under `repository.topics` for `safe-settings`
compatibility; declaring both is an error.

| Field | Type | Required | Description |
|---|---|---|---|
| `items` | list of string | no | The declared items. |
| `prune` | boolean | no | Delete items that exist on GitHub but are absent here. |


## `version`

Schema major version this file targets.

Type: integer

