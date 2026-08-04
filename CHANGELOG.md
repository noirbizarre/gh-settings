# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0](https://github.com/noirbizarre/gh-settings/compare/0.1.0..0.2.0) - 2026-08-04

### 💫 Features

- **action** Add a composite action - ([1bdacb7](https://github.com/noirbizarre/gh-settings/commit/1bdacb79996d9f242a3ea7981094a940a3ef6ddf))
- **config** Resolve and merge an inherited configuration - ([b667076](https://github.com/noirbizarre/gh-settings/commit/b667076f80b2d4edf5e304df43fb6ee1426361f8))
- **config** Add the extends key, its reference syntax and its validation - ([3aef957](https://github.com/noirbizarre/gh-settings/commit/3aef9575a715d5e2a6240872e620249d64e2ee77))
- **config** Merge an inherited configuration with the one extending it - ([c6fba48](https://github.com/noirbizarre/gh-settings/commit/c6fba4880101438cfed06fb3cbe37e3e41cfd6ff))
- **doctor** Say when a token cannot read an inherited configuration - ([9fab2c7](https://github.com/noirbizarre/gh-settings/commit/9fab2c7786ce6f64e1cd67b0a660b09d0b871bb2))
- **engine** Record inherited configurations in a saved plan - ([2f4be39](https://github.com/noirbizarre/gh-settings/commit/2f4be39c83666a776263e0263732fd111fd760b2))
- **github** Read a file verbatim, and load a base configuration with it - ([3c8dfce](https://github.com/noirbizarre/gh-settings/commit/3c8dfce100b2af82f91441ae32fae14ade662fd9))
- **json** Name the document each finding's offset belongs to - ([d5db3e1](https://github.com/noirbizarre/gh-settings/commit/d5db3e16b19d0e339c1ce2aed2d2553398541643))
- **sync** Refuse to start when a change is certain to be rejected - ([75d2564](https://github.com/noirbizarre/gh-settings/commit/75d256484374f86b67055a4e1c12cc1b4268c566))
- **sync** Name the missing permission when a write is refused - ([b2e966a](https://github.com/noirbizarre/gh-settings/commit/b2e966a9dc0fd560e7694226e7e2eef3ae7e6f78))

### 🐛 Bug Fixes

- **cli** Make `--format json` actually machine-readable - ([a0f4e3a](https://github.com/noirbizarre/gh-settings/commit/a0f4e3ac1c84d9fb2faf042ed400a30f0c9ca09b))
- **rulesets** Underline the rule the user wrote, not the one that sorted there - ([1e62c2e](https://github.com/noirbizarre/gh-settings/commit/1e62c2e89fe0466a8243d0a7a8aa3b76e9b4e14a))
- **topics** Stop panicking on a configuration that only uses repository.topics - ([7fe71f0](https://github.com/noirbizarre/gh-settings/commit/7fe71f07a6dd4f0db7b13546e387d2d61ab31c56))
- **validate** Point at the item, not the whole section, in the object form - ([633b0fb](https://github.com/noirbizarre/gh-settings/commit/633b0fbe15cec3a1c72be817407d41d5731484fe))

### 🔨 Refactor

- **config** Record where each path lives instead of probing for it - ([8293ae4](https://github.com/noirbizarre/gh-settings/commit/8293ae49e7e4a9289b764e76f9ff92e4b43faebe))
- **config** Carry every contributing document on Config - ([9be2719](https://github.com/noirbizarre/gh-settings/commit/9be2719077cf98769e7f9b56af28e4e8635eb46e))
- **config** Tag spans with their document and split exact from resolve - ([31c88b7](https://github.com/noirbizarre/gh-settings/commit/31c88b7fd70d48d16763b82290e74ef1b2e96bef))
- **config** Introduce Sources and SourceId - ([14e6d28](https://github.com/noirbizarre/gh-settings/commit/14e6d287f9274ffef280142eeb5dae3d4c677a5b))
- **diagnostics** Render each finding against its own document - ([862a7c8](https://github.com/noirbizarre/gh-settings/commit/862a7c814027f605a2ed743fb9d70fa8a13bfebe))
- **github** Use the Resolver instead of duplicating it - ([54e4655](https://github.com/noirbizarre/gh-settings/commit/54e4655062f90d55ac80838a51a28ea6b3d3f366))
- **resources** Make Requirement::verdict the one place a token is judged - ([ad88b68](https://github.com/noirbizarre/gh-settings/commit/ad88b68a9ac77efab3a37a435ddcfaaec2ebcc07))

### 📚 Documentation

- Record diagnostic provenance (ADR-016) - ([8961bb6](https://github.com/noirbizarre/gh-settings/commit/8961bb6a88b4a16bef2ffa0d1f5858024fbe6175))
- Describe the pre-flight check and tick off what shipped - ([47f1a43](https://github.com/noirbizarre/gh-settings/commit/47f1a43cd7bf7b734a6e8e795c324ec4fdd71ee1))

### 🧪 Tests

- **live** Run the real thing against a real repository - ([679b826](https://github.com/noirbizarre/gh-settings/commit/679b8266c01a21fa1066aacd52599a1b7349fa50))
- **rulesets** Cover the apply path through the stub - ([05ea09a](https://github.com/noirbizarre/gh-settings/commit/05ea09aed379f9cdeff54ec1a52be89c37905b30))
- **sync** Exercise --continue-on-error through the binary - ([885f29b](https://github.com/noirbizarre/gh-settings/commit/885f29b22d98e8b909e8f78ef657e869901a3a9d))
- Snapshot the output of plan, doctor and export - ([e1f0fdc](https://github.com/noirbizarre/gh-settings/commit/e1f0fdc19f4afe51b839a23d195ea55c239e5058))

### 🔧 CI

- **release** Pass the release tag through env: rather than interpolating it - ([ce071ab](https://github.com/noirbizarre/gh-settings/commit/ce071abaf3ec17e96e14a5d48193613bd67f6d83))

## 0.1.0 - 2026-08-03

### 💫 Features

- **docs** Generate the permission and CLI references from the code - ([2b50807](https://github.com/noirbizarre/gh-settings/commit/2b5080758b7eafe574e78adeb9b56e21c9e88e6e))
- **export** Annotate with `# $schema:` instead of the yaml-language-server modeline - ([9b03354](https://github.com/noirbizarre/gh-settings/commit/9b03354c1d3269ffd9ae3ee08373626d814d839f))
- Declarative repository settings with plan, sync and export - ([1778559](https://github.com/noirbizarre/gh-settings/commit/177855901b9f6e7a624a439b806315ecb0eadad7))

### 🐛 Bug Fixes

- **github** Report the real HTTP status for paginated requests - ([155f7fa](https://github.com/noirbizarre/gh-settings/commit/155f7faee9af8deb1684cb236f82dda3215e6c7e))
- **rulesets** Ignore server-defaulted rule parameters when diffing - ([259f8cf](https://github.com/noirbizarre/gh-settings/commit/259f8cf97a115306eed2302ccfa078e2b4a41e24))
- **tests** Redact the config path wherever the OS puts temporary files - ([28ed341](https://github.com/noirbizarre/gh-settings/commit/28ed34165175a5ea800cb0a3cd732d3a07a596aa))
- **tests** Make the gh stub portable across GNU and BSD userlands - ([6e6ec29](https://github.com/noirbizarre/gh-settings/commit/6e6ec2962f36acfc21be14a6a0deba374a48105e))

### 🔨 Refactor

- Adopt gh-ship's crate conventions and publish the schema on Pages - ([3731c93](https://github.com/noirbizarre/gh-settings/commit/3731c932cea04a043cd1b1240c8752d94c80313f))

### 📚 Documentation

- **roadmap** Record the live test suite, the action and inheritance - ([afe25fc](https://github.com/noirbizarre/gh-settings/commit/afe25fcfb674d66af907095c0d0de57e5adbb0d1))
- **roadmap** Track supported settings and follow-ups in the repository - ([bc6b7e1](https://github.com/noirbizarre/gh-settings/commit/bc6b7e15b723e0956803e8ffccd2fad3ab9a6635))
- Record the gh-ship release decision and adopt its docs setup - ([09a1ffd](https://github.com/noirbizarre/gh-settings/commit/09a1ffda058d6ebaa076c60be13c0f7530fa8930))
- Architecture decisions, authentication guide and configuration reference - ([98ac714](https://github.com/noirbizarre/gh-settings/commit/98ac71428143fff374623464aed4f06ed761da63))

### 🏗️ Build

- **mise** Update the mise.lock file - ([2e37c55](https://github.com/noirbizarre/gh-settings/commit/2e37c55c35862f2dfdfed706511af3de663206fc))

### 🔧 CI

- **codecov** Wait for every matrix leg before judging coverage - ([d140e8f](https://github.com/noirbizarre/gh-settings/commit/d140e8fdbb52b258b473bbfc986e1c7b3bad4ad0))
- **release** Build for windows-arm64 and verify asset names - ([ad4010e](https://github.com/noirbizarre/gh-settings/commit/ad4010e7fcdc9b7b8b84e8ee0237afe888205757))
- Release with gh-ship, and align CI with its workflows - ([0f85fc9](https://github.com/noirbizarre/gh-settings/commit/0f85fc984c9c8ef7bb23103010aac3be1e0d9bf5))

### 🧹 Chores

- Align the toolchain and lint stack with gh-ship - ([17b6109](https://github.com/noirbizarre/gh-settings/commit/17b610981d809a6a1d4d06fc39cbcbc28b4da38e))
- Set up the project toolchain and CI - ([f89f30f](https://github.com/noirbizarre/gh-settings/commit/f89f30f918b11cdb8719ba537ea2e517baabf9aa))

## ❤️ New Contributors

* @noirbizarre made their first contribution in [#2](https://github.com/noirbizarre/gh-settings/pull/2)
