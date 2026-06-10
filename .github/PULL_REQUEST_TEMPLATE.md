<!--
Thanks for your PR! Please fill in the sections below. Sections marked
"(optional)" can be skipped for trivial changes (typos, doc fixes).
-->

## Summary

<!-- One paragraph: what does this change and why. -->

## Related issues

<!-- e.g. Closes #123, Refs #456 -->

## Type of change

<!-- Tick all that apply. -->

- [ ] `feat:` New user-visible capability
- [ ] `fix:` Bug fix
- [ ] `refactor:` Behaviour-preserving code restructure
- [ ] `perf:` Performance improvement
- [ ] `docs:` Documentation only
- [ ] `test:` Tests only
- [ ] `chore:` Tooling / CI / housekeeping
- [ ] `build:` Build system / dependencies

## Load-bearing surfaces touched

<!-- Pick all that apply. Any "yes" means an extra reviewer is required. -->

- [ ] Audit chain (Merkle + Ed25519)
- [ ] Permission matrix
- [ ] Storage / migrations
- [ ] MCP server / protocol
- [ ] Cryptography / keystore
- [ ] None of the above

## Checklist

- [ ] CI is green locally (`pnpm typecheck && pnpm lint`, `cargo fmt --check && cargo clippy --all-targets && cargo test`).
- [ ] Conventional Commits style on every commit (the commit-msg hook enforces this).
- [ ] If I touched any read/write of user memory, I added an `audit::record(...)` entry.
- [ ] If I touched the database schema, I added a new `sqlx migrate` file (didn't edit existing).
- [ ] If I changed module responsibilities, I updated [ARCHITECTURE.md](../ARCHITECTURE.md).
- [ ] If I added a new dependency, I justified it in the description (alternatives considered, supply-chain notes).
- [ ] If user-facing behaviour changed, I added an entry under `## Unreleased` in [CHANGELOG.md](../CHANGELOG.md).

## Screenshots / demo (optional)

<!-- For UI changes, drag-drop a screenshot or a 30s screencast here. -->

## Anything else reviewers should know? (optional)

<!-- Trade-offs, follow-ups, known limitations. -->
