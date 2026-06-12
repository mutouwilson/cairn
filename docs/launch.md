# Launch kit

Copy + plan for the first public push. The goal is **one high-signal launch
event** that lands in front of the right people → the first real wave of stars.
Stars are a result, not the goal — don't buy or swap them.

## Pre-flight checklist

- [x] Record `docs/demo.gif` (storyboard below) and swap it into the README hero
      — done 2026-06-12 (9.9 MB, 1445×930).
- [ ] Pick a launch day: **Tue–Thu, ~8–10am US Pacific**.
- [ ] Be free for ~3 hours after posting to reply to *every* comment. Engagement
      in the first hour decides whether HN/Reddit ranks you.
- [ ] Make sure the Releases page is clean (latest alpha pinned, clear install
      notes for the unsigned `.app` / `.msi` / `.AppImage`).

## demo.gif storyboard (8–12s silent loop)

1. **(0–2s)** Type in the ChatGPT composer → the Cairn pill appears: *"2 matches"*.
2. **(2–4s)** Press Enter → the memories drop in as a `<context>` block above the prompt.
3. **(4–7s)** Cut to the Cairn app → that same note, already structured into typed
   entities + relations.
4. **(7–10s)** Audit view → the signed chain, rows lighting up *"verified"*.

Keep it silent, add short captions, target < 4 MB so GitHub inlines it.

---

## Show HN

**Title:**
```
Show HN: Cairn – a local-first memory OS your AI agents can read
```

**Body:**
> Every AI tool I use has its own memory, or none. ChatGPT forgets, Claude
> forgets, Cursor re-learns my stack every session — and the "memory" features
> that do exist live in someone else's cloud.
>
> Cairn is my attempt at a single, local memory layer. You capture notes (or
> highlight text anywhere); on-device AI structures them into typed
> entities/relations/themes; then any MCP-capable agent reads exactly what you
> authorize. A browser extension also surfaces relevant memories inline across
> ~10 AI chat sites and injects them with one keypress.
>
> The part I cared most about is trust. It's local-first (a SQLite file on your
> machine, no account), and every read/write/extract/grant is hashed into a
> Merkle chain and Ed25519-signed, so you can verify offline that nothing was
> altered behind your back.
>
> Stack: Tauri 2 + Rust + Next.js, SQLite/FTS5, MCP over stdio. Desktop builds
> for macOS/Windows/Linux + a Chrome extension. Source-available under FSL-1.1
> (each release converts to Apache-2.0 after two years).
>
> It's early (alpha). I'd love feedback on the model — especially the
> "verify, don't trust" audit chain, and whether the per-agent × per-type
> permission matrix fits how you'd actually share memory with an agent.
>
> Repo: https://github.com/mutouwilson/cairn

---

## Reddit

Primary: **r/LocalLLaMA**. Also (spaced out over a day, not the same minute):
r/selfhosted, r/ClaudeAI, r/ChatGPT.

**Title:**
```
I built a local-first memory layer any MCP agent (Claude, Cursor…) can read — structured, auditable, fully yours
```

**Body:**
> I wanted one memory that every AI tool shares, that lives on my machine, and
> that I can actually audit — so I built Cairn.
>
> - **100% local** — SQLite file on your disk, no account, no telemetry, works offline.
> - **One memory, every agent** — exposed over MCP (Claude Desktop, Cursor, Codex,
>   Cline…), plus a browser extension that surfaces + injects memories across ~10
>   AI chat sites.
> - **Structured, not flat** — on-device AI extracts typed entities, relations and
>   themes; retrieval is BM25 + importance + recency decay, not a dumb vector blob.
> - **Verify, don't trust** — every access is Merkle-chained + Ed25519-signed; you
>   can replay and verify the whole history offline.
> - **You control sharing** — per-agent × per-entity-type permission matrix.
>
> Source-available (FSL-1.1 → Apache-2.0 after 2 years). Desktop builds + Chrome
> extension in Releases. Still alpha — feedback very welcome, especially from
> people running local models + MCP.
>
> Repo: https://github.com/mutouwilson/cairn

(For r/selfhosted, lead with the local/self-host/no-cloud angle. For r/ClaudeAI,
lead with the MCP + Claude Desktop integration.)

---

## X / Twitter thread

1/ Your AI tools each keep their own memory — or none. ChatGPT forgets, Cursor
re-learns your stack every session, and "memory" usually means *their* cloud.
I built Cairn: one **local** memory every agent can read. 🧵

2/ [demo gif]
Type in ChatGPT → relevant memories surface → one keypress injects them.
The same memory is readable by Claude, Cursor, Codex — anything that speaks MCP.

3/ How it works: you jot a note → on-device AI structures it into typed
entities/relations/themes → it's exposed to agents over MCP. Your data stays in
a SQLite file on your machine. No account, no cloud.

4/ The part I care about most: trust.
Every read/write is hashed into a Merkle chain + Ed25519-signed. You can verify,
offline, that nothing was changed behind your back. Verify, don't trust.

5/ Local-first · source-available (Fair Source) · works across 10+ AI chat sites.
Desktop apps for macOS/Windows/Linux + a Chrome extension. Still alpha — kicking
the tires + feedback both very welcome 👇
https://github.com/mutouwilson/cairn

---

## Distribution checklist (after the gif is in)

- [ ] Show HN (the anchor — everything else can point back to it)
- [ ] r/LocalLLaMA → later r/selfhosted, r/ClaudeAI, r/ChatGPT
- [ ] X thread; @ a few MCP / Anthropic-adjacent accounts who cover the ecosystem
- [ ] PRs adding Cairn to: awesome-mcp, awesome-local-first, awesome-selfhosted
- [ ] Share in MCP / Anthropic community Discords
- [ ] Cross-post the "why I built it" story (separate blog/thread) a few days later
      to catch the second wave

## Don'ts

- Don't buy/swap stars (GitHub bans + the signal is worthless).
- Don't blast the identical link to 6 subreddits in the same minute — it reads as
  spam and gets you removed.
- Don't optimize for the number. 100 stars from people who installed it >>> 1000
  drive-by stars. Stars are cold-start social proof, not the product.
