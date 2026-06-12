# Contributing to Cairn

Thanks for your interest! Cairn is in **alpha** — the source is public ([FSL-1.1](./LICENSE), Fair Source) while the core is still moving fast. **Issues and bug reports are very welcome.** For code contributions, please open an issue to discuss before sending a PR — the architecture shifts quickly at this stage and we'd hate for your work to go to waste.

Sections marked **(post-public)** describe the full contributor flow as it stabilizes.

## Quick links

- 🔧 [DEVELOPMENT.md](./DEVELOPMENT.md) — setup, commands, MCP integration
- 🏛 [ARCHITECTURE.md](./ARCHITECTURE.md) — system design and module map
- 🔒 [SECURITY.md](./SECURITY.md) — security policy
- 📜 [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) — community standards

## What we accept

| Type | Status (alpha) | Status (post-public) |
|---|---|---|
| Bug reports | ✅ open | ✅ open |
| Feature requests | ✅ open | ✅ open |
| Documentation fixes (typos, broken links) | ✅ PR welcome | ✅ PR welcome |
| Small bug fixes (< 50 lines) | discuss in issue first | ✅ PR welcome |
| Larger features / refactors | ❌ ask first | discuss in issue first |
| New modules in `src-tauri/src/` | ❌ ask first | discuss in issue first |
| Performance regressions / SLO violations | ✅ PR welcome | ✅ PR welcome |

## How to file a bug

1. Search [existing issues](https://github.com/mutouwilson/cairn/issues) first.
2. Open a new issue with the **Bug report** template.
3. Include reproduction steps, your OS + Cairn version, and the last ~50 lines of `cairn.log`.

## How to propose a feature

1. Open an issue using the **Feature request** template.
2. Describe the problem first, then the proposed solution.
3. If it touches the audit chain, the permission model or storage layout, flag this — those are load-bearing surfaces and need an explicit ADR (Architecture Decision Record) before code.

## Submitting a PR

1. Fork the repo and create a topic branch (`feat/your-thing`, `fix/issue-123`).
2. Make sure CI is green locally:
   ```bash
   cd memory
   pnpm install
   pnpm exec tsc --noEmit && pnpm lint
   ( cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test )
   ```
3. Follow **Conventional Commits** for commit messages (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`, `perf:`). A pre-commit hook (husky + commitlint) will enforce this.
4. Update [ARCHITECTURE.md](./ARCHITECTURE.md) and inline doc-comments if you change a module's responsibilities.
5. If you touch the database, add an `sqlx migrate` file — don't edit existing migrations.
6. **Audit chain rule**: every code path that reads or writes user memory MUST record an `AuditLogger::log(...)` entry. No exceptions.
7. Open the PR using the template; link the issue with `Closes #N`.

## Code style

- **Rust**: `rustfmt` defaults. Clippy on `--all-targets` should be clean (we'll re-enable `-D warnings` once the existing backlog is cleared).
- **TypeScript**: Next.js + `next/core-web-vitals` + `next/typescript` (see `memory/.eslintrc.json`).
- **No comments that explain what the code does** — name things well. Save comments for *why*: invariants, gotchas, links to issues/RFCs.
- **No new dependencies without a note in the PR description**: why we need it, alternatives considered, supply-chain notes.

## Tests

- Rust: unit tests next to the module (`#[cfg(test)] mod tests { ... }`).
- Frontend: TBD (likely Vitest + Testing Library). For now, manual repros in PR descriptions.
- Performance SLOs (cold start, idle RSS, capture latency) are tracked separately — if your change might affect them, run the benchmarks in `memory/benchmarks/` before and after.

## Security disclosures

**Do not file public issues for security vulnerabilities.** See [SECURITY.md](./SECURITY.md) for the disclosure flow.

## Licensing of contributions

By submitting a PR you agree your contribution is licensed under the same [FSL-1.1-ALv2 license](./LICENSE) that covers the rest of the repo (inbound = outbound; each release converts to Apache-2.0 two years after publication). No CLA required.

## Code of conduct

Participation is governed by [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md). Be the kind of person you'd want to collaborate with at 2am over a tricky bug.

## **(Post-public)** Maintainer review checklist

When you receive a PR, before merging confirm:

- [ ] CI is green
- [ ] Conventional commit prefix is correct
- [ ] No new `unsafe` without justification
- [ ] No new dependency without justification
- [ ] If touches `audit/` or `mcp/`, has an extra reviewer
- [ ] If touches storage, has a migration
- [ ] CHANGELOG entry under `## Unreleased`
- [ ] Squash-merge with the PR title as the commit message
