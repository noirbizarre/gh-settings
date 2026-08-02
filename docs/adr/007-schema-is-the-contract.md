# 7. The JSON Schema is the public contract, generated from Rust

## Status

Accepted.

## Context

The YAML file is what users write, commit and share. It is the real interface of
this project — more so than the CLI.

A hand-written schema inevitably drifts from the code. A schema that exists only
in the code cannot power editor completion.

## Decision

The schema is generated from the Rust types with `schemars`, exposed through
`gh settings schema`, and committed to the repository. CI diffs the committed
copy against freshly generated output, so any change to a configuration type
that is not reflected in the schema fails the build.

It is published at `https://noirbizarre.github.io/gh-settings/schema/v1/settings.json`, versioned
by major, and `export` writes the `# $schema: …` annotation
into every file it generates. That form is preferred over the longer
`# yaml-language-server: $schema=…` modeline because it is shorter and IntelliJ
recognises only the former.

Within a major version, changes are additive only.

## Consequences

* Schema and implementation cannot disagree.
* Users get completion, validation and hover documentation for free.
* Rust type documentation becomes user-facing documentation, which raises the
  bar for doc comments on configuration types.
* Renaming a configuration field is a breaking change requiring a major version.
