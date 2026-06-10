# Cairn vs LoCoMo: v0.1 baseline (with footguns)

_Run date: 2026-05-15. Numbers are not yet directly comparable to the LoCoMo paper — see Caveats._

## TL;DR

We ran [Cairn][cairn] — our personal memory OS for AI agents — against [LoCoMo][locomo], the long-conversation memory benchmark from Snap + Stanford. On all 10 conversations / 1985 Q&A pairs, Cairn's first-pass retrieval scored:

- **Overall token F1: 0.179** (EM 0.044)
- Single-hop: F1 **0.248**, EM 0.032
- Multi-hop: F1 **0.299**, EM 0.085
- Open-domain: F1 **0.158**, EM 0.052
- Temporal: F1 **0.061**, EM 0.009
- Adversarial: F1 **0.000**, EM 0.000

Two of those numbers are stories in themselves: **temporal = 0.06** (Cairn doesn't anchor entities to event time) and **adversarial = 0** (Cairn has no "I don't know" mode — it returns top-K unconditionally). The other three sit in a respectable middle.

## What Cairn is

Cairn is a local-first memory layer that sits between any LLM agent and the user. You drop notes / paste text / let agents save what they learn about you, and Cairn extracts structured entities (Person, Goal, Preference, Belief, …) with a typed graph behind them. Agents query it back through MCP / a local HTTP API / a browser extension.

Three things make Cairn different:

1. **Hybrid retrieval out of the box** — BM25 (trigram FTS5) + vector (cosine) + RRF fusion, with a 0.65 cosine distance threshold to suppress junk.
2. **User-level only** — Cairn flattens per-project memory into one user memory store (no per-project tagging). The premise is that *you* are the constant, your projects come and go.
3. **No lock-in** — every file imported from Claude Code / Cursor / etc. keeps its source path; managed-block export to `~/.claude/CLAUDE.md` is sha256-verified so manual edits won't get silently overwritten.

## What LoCoMo is

[LoCoMo][locomo] (Long Conversational Memory, [Maharana et al. 2024][paper]) is a benchmark of multi-session conversations between two personas with ~200 Q&A pairs per conversation, across 5 categories:

| category | what it tests | example |
|---|---|---|
| single-hop | direct fact lookup | "What breed of dog did Alice adopt?" |
| multi-hop | composing 2+ facts | "Where does Alice volunteer and what city did she move to?" |
| temporal | time reasoning | "When did Caroline go to the LGBTQ support group?" |
| open-domain | grounded world knowledge | "Did Bob's marathon training plan follow standard practice?" |
| adversarial | unanswerable | "What is Alice's brother's favorite color?" (Alice has no brother) |

Total: 10 conversations × ~580 turns × ~200 Q&A.

## Methodology

We ran every turn through `POST /api/capture` (the same endpoint the browser extension uses), which:
1. Inserts the raw text as a `note` row.
2. Schedules an async LLM extraction (gpt-4o-mini in this run) that turns the note into typed entities + relations.
3. Vector-embeds each entity name (1024-dim) for hybrid retrieval.

For each Q&A pair, we then `GET /api/search?q=<question>` and aggregate the top 20 entity names + note texts. We score the highest-scoring retrieved text against the gold answer with [SQuAD-style token F1][squad].

We did **not** use an LLM-as-judge — every score below is mechanical token overlap. That penalises us a lot on paraphrase ("7 May 2023" vs "May 7, 2023"); see _Caveats_.

Source code: [`benchmarks/locomo/`][source].

## Results

```
=== LoCoMo summary ===
category           n       EM       F1
----------------------------------------
adversarial      446    0.000    0.000
multi-hop        840    0.085    0.299
open-domain       96    0.052    0.158
single-hop       282    0.032    0.248
temporal         321    0.009    0.061
----------------------------------------
OVERALL         1985    0.044    0.179
```

Wall-clock budget: ~10 min ingest (5882 captures @ ~100ms each), 20 min drain for the async extractor queue, ~50 min query (2000 retrievals @ ~1.5s each). Total ≈ 90 min, ≈$1.2 in gpt-4o-mini extraction + embedding spend.

## What worked

**Multi-hop got the highest F1 (0.299)** — surprising at first, but multi-hop gold answers tend to be multi-token phrases ("Riverside Rescue shelter in Brooklyn; Hoboken"), and Cairn's top-K retrieval frequently returns enough of those tokens for partial credit. With LLM-judge scoring instead of token-overlap, this number would either go up (judge accepts paraphrases) or down (judge demands the composition be correct, not just the parts).

**Single-hop F1 0.248** — Cairn does surface the right entity for direct lookups (career path, art style, who-supports-whom), but EM is low because the entity NAME isn't the gold answer phrase verbatim. Example: gold "abstract art" vs Cairn entity "Caroline's painting" — both refer to the same memory but token overlap is partial.

**Adversarial 0.000** — read it as a known architectural gap, not a quality signal. See below.

## What didn't

1. **Temporal queries → 0.061.** Cairn knows when each turn was *captured* (turn timestamp lives in `notes.created_at`) and the extractor sometimes pulls a date into `properties.date` — but `/api/search` doesn't index or surface those dates as queryable fields. We bucket-rank by `(BM25 ⊕ vector) × importance × recency`, where recency is "when did the note enter Cairn," not "when did the described event happen." A question like _"When did Melanie go on a hike after the roadtrip?"_ returns hike-related entities but never the date itself. Fix: V4 (timeline retrieval, Allen-interval relations).

2. **Adversarial 0.000.** LoCoMo's adversarial category asks unanswerable questions ("What are Melanie's summer adoption plans?" when none were discussed) and the gold answer is `None`. Cairn's retrieval path returns top-K unconditionally — there's no "confidence too low to answer" gate. Every retrieved entity is therefore wrong, F1=0 against `None`. The fix is signal-side: either the agent calling Cairn checks `diagnostics.fused_hits` and `final_score` thresholds before answering, or Cairn exposes a "no good hit" flag. Roadmap V9.

3. **Cross-language / cross-conversation contamination.** We ran all 10 convs through one Cairn DB on the author's machine, which already contained months of his real personal memory in Chinese + English. We tagged each LoCoMo turn with `source=locomo:<conv-id>` but `/api/search` doesn't filter by source — so retrieval can (and did) return entities like _"Caroline的艺术创作"_ (real user's Chinese entity about a friend named Caroline) for LoCoMo questions about a fictional Caroline. Roadmap V11: `source LIKE …` filter on search; will let us re-run with clean isolation.

4. **Extraction coverage gaps.** Specifics described offhand in conversation get dropped. Gold "Figurines, shoes" became "pottery_bowls, pottery_project" — the extractor latched onto a more recent, higher-confidence pottery thread and missed earlier purchases. Single-shot extraction over isolated turns doesn't keep a running tally; a windowed re-extract pass (V5) would help.

## Caveats — don't compare these numbers to the paper yet

- **Token F1 ≠ LoCoMo's reference judge.** The paper uses LLM-as-judge with paraphrase tolerance ("7 May 2023" ≈ "May 7, 2023"). We use mechanical SQuAD-style token overlap. Our F1 is therefore a strict lower bound for the same retrieval — when we wire up an LLM judge (V1 W4), expect a substantial uplift across all categories.
- **No DB isolation.** See "contamination" above.
- **No source filter on search.** Same.
- **gpt-4o-mini extraction.** Cheaper than gpt-4o; the harder entities (figurines/shoes, abstract art, Rome-as-shared-city) are within reach of a stronger extractor.
- **One shared DB across 10 convs.** Adds cross-conv noise.

In short: this is the **v0.1 baseline of an unpolished system**. The blog post you'd publish next time, after fixing isolation + judge + temporal retrieval, will move these numbers — probably significantly.

## What's next (next time we publish)

| change | expected effect |
|---|---|
| Source filter on `/api/search` (V11) | Eliminates cross-conv noise; lifts single-hop + open-domain |
| LLM-as-judge scoring (V1 W4) | Paraphrase tolerance; lifts every category, especially temporal |
| Timeline retrieval (V4) | Pulls dates from entity properties + turn timestamps into ranking |
| Confidence threshold + refusal signal (V9) | Adversarial moves from 0 to meaningfully above zero |
| Re-extract pass over earlier turns (V5) | Fewer missed specifics; modest single-hop lift |

The harness (`benchmarks/locomo/`) and a cleanup script (`cleanup.sh`) are in the [Cairn repo][source]; you can reproduce the run end-to-end.

[cairn]: https://cairn.dev
[locomo]: https://snap-research.github.io/locomo
[paper]: https://arxiv.org/abs/2402.17753
[source]: https://github.com/cairn-dev/cairn/tree/main/benchmarks/locomo
[squad]: https://rajpurkar.github.io/SQuAD-explorer/
