# Security Policy

Cairn handles personal memory data, signing keys, and cryptographic audit chains. We take security seriously.

## Reporting a vulnerability

**Do not file a public issue.** Send a private report instead:

- Email: **security@cairn.local** (TODO: replace with real address before public launch)
- Or use GitHub's [private security advisory](https://github.com/mutouwilson/cairn/security/advisories/new) flow once the repo is public.

We aim to:

- Acknowledge your report within **3 business days**.
- Provide a triage outcome within **7 business days**.
- Ship a fix within **30 days** for high-severity issues.

Please include:

- The version / commit hash you tested.
- Reproduction steps.
- Impact assessment (what an attacker could do).
- Suggested mitigation, if any.

## Scope

In scope:

- The Cairn desktop app (`memory/`) — Tauri shell, Next.js UI, Rust core.
- The `cairn-mcp` standalone MCP server.
- The audit chain integrity (Merkle + Ed25519).
- The permission matrix and IPC surface.
- Storage encryption (when built with `--features encrypted`).
- Dev-tool sync read/write paths (V6).

Out of scope:

- Third-party MCP hosts (Claude Desktop, Cursor, etc.) — please report to those vendors.
- Issues that require physical access to an unlocked machine.
- Vulnerabilities in upstream crates / npm packages — please report upstream; we'll bump our deps once a fix is available.
- Self-XSS where the attacker is the user themselves.

## Disclosure policy

We follow **coordinated disclosure**:

1. You report privately.
2. We confirm + agree on a fix timeline.
3. We ship a fix.
4. You and we agree on a public disclosure date (typically 30-90 days after fix ships).
5. We publish a CVE (when appropriate) and credit you in the changelog and security advisory.

## Supported versions

During alpha, only the `main` branch tip and the latest tagged pre-release are supported. Once we reach v0.1 stable, the latest minor will receive security fixes.

## Hall of fame

Reporters who have responsibly disclosed will be listed here (with permission).
