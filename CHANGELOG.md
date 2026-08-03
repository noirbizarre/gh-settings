# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

* @noirbizarre made their first contribution
