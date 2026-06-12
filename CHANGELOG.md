# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-12

First public release on this repository (supersedes the retired alpha pre-releases).

### Added

- Initial repo import: Tauri 2 + Next.js 15 + Rust core with audit chain (SHA-256 hash chain + Ed25519), MCP stdio server, permission matrix, Ebbinghaus-aware retrieval.
- `ARCHITECTURE.md` and `DEVELOPMENT.md` covering system design and contributor setup.
- GitHub Actions CI: frontend `typecheck + lint`, rust `fmt + clippy + check + test`.
- Apache-2.0 LICENSE, CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md.
- Issue templates (bug report, feature request) and pull-request template.
- Dependabot for npm, cargo, and GitHub Actions.
- `.editorconfig`, Husky + commitlint pre-commit hooks (Conventional Commits enforced).
- Release workflow draft (tagged builds for macOS, Linux, Windows).

### Changed

- Relicensed from Apache-2.0 to **FSL-1.1-ALv2** (Functional Source License — Fair Source). Source stays public and auditable; non-competing use, modification, and self-hosting are free; building a competing commercial product/service is restricted. Each release auto-converts to Apache-2.0 two years after publication. Commercial license available on request.
- Renamed env vars and module identifiers from legacy `NEXTAGENT_*` / `nextagent` to `CAIRN_*` / `cairn`.
- Removed empty placeholder `tools/browser-ext/` at repo root (the real implementation lives in `memory/tools/browser-ext/`).
- Renamed UI design source `untitled.pen` → `cairn.pen`.

### Removed

- Strategy / research / competitive-analysis notes moved out of this repo into a separate private archive.

[Unreleased]: https://github.com/mutouwilson/cairn/compare/v0.1.0...main
[0.1.0]: https://github.com/mutouwilson/cairn/releases/tag/v0.1.0
