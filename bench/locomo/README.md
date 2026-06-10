# LoCoMo · Long Conversation Memory Benchmark

A reproducible harness for evaluating Cairn against the [LoCoMo](https://arxiv.org/abs/2402.17753) long-term-memory benchmark (SNAP Research, ACL 2024).

> **Why this exists.** MemoryLake and several other AI-memory startups publish LoCoMo scores ("global #1 at 94.03%") but ship only result JSON and plotting code, not the evaluation harness itself. There is no way to reproduce their numbers from outside their lab.
>
> We do the opposite: this directory holds the **complete pipeline** — fetch dataset → ingest into Cairn → query via MCP → score. Run it yourself, get exactly our numbers. If we publish a score, the harness that produced it lives here.

## At a glance

| Item | Value |
|---|---|
| Dataset | 10 conversations · ~600 turns each · ~16K tokens each |
| Questions | 1,540 across 4 task types |
| Task types | `single_hop` · `multi_hop` · `temporal` · `open_domain` |
| Metric | **F1** (token-stemmed precision/recall) · GPT-4-as-judge validation |
| Reference scores | MemoryLake (self-reported): 94.03 · EverMemOS: 92.32 · full-context baseline: 91.21 |

## How it works

```
┌──────────────────────────────────────────────────────────────────────┐
│                         locomo-harness                               │
└──────────────────────────────────────────────────────────────────────┘
            │
            │  1. fetch dataset
            ▼
┌──────────────────────┐
│ snap-research/locomo │  → data/locomo10.json
└──────────────────────┘
            │
            │  2. for each conversation:
            ▼
┌──────────────────────────────────┐
│ cairn (clean DB)                 │
│   capture_note(turn)             │  ← ingest every turn via HTTP /capture
│   capture_note(turn)             │
│   …                              │
└──────────────────────────────────┘
            │
            │  3. for each question on that conversation:
            ▼
┌──────────────────────────────────┐
│ cairn search_memory(question)    │  ← retrieve top-k entities/notes
└──────────────────────────────────┘
            │
            │  4. answer = LLM(retrieved_context + question)
            ▼
┌──────────────────────────────────┐
│ score F1(answer, ground_truth)   │
└──────────────────────────────────┘
            │
            ▼
       results.json
```

Cairn itself is a memory **store**, not a QA system, so step 4 plugs an LLM
on top of Cairn's retrieved context. We report the LLM used so the score
is comparable.

## Quick start

```bash
# 1. clone, install deps
cd bench/locomo
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

# 2. pull the dataset
./run.sh fetch

# 3. start a fresh cairn-mcp + HTTP API on the side
cd ../../memory && pnpm build && pnpm tauri:dev &
cd ../bench/locomo

# 4. run the benchmark (uses CAIRN_API_URL + LLM_API_KEY from env)
./run.sh evaluate --conversations all --tasks all --answerer claude-haiku-4-5

# 5. inspect
cat results/latest/results.json | jq '.summary'
```

Numbers land in `results/<timestamp>/`:

- `results.json` — per-question, with retrieved context, predicted answer, ground truth, scores
- `summary.json` — aggregate F1 per task type and overall
- `traces/` — per-conversation Cairn audit-chain export, so each run is reproducible

## Configuration

Environment variables (or `.env` in this directory):

```env
CAIRN_API_URL=http://localhost:8787   # cairn-mcp HTTP surface
CAIRN_AGENT_ID=locomo-harness         # used in audit chain entries

# Answerer LLM — anything with a chat-completions API.
LLM_PROVIDER=anthropic                # anthropic | openai | gateway
LLM_MODEL=claude-haiku-4-5-20251001
LLM_API_KEY=sk-ant-…

# Judge LLM — used for free-form `open_domain` answers where F1 alone is noisy.
JUDGE_PROVIDER=anthropic
JUDGE_MODEL=claude-sonnet-4-7
JUDGE_API_KEY=sk-ant-…                # may differ from LLM_API_KEY
```

## What we score and how

- **F1 (primary).** Token-stem overlap between predicted answer and gold,
  micro-averaged per task type and overall. This matches the LoCoMo paper.
- **LLM-judge concordance (secondary).** For `open_domain` we additionally
  ask a judge LLM "is this answer correct given the gold and the
  conversation?" The judge's accept-rate against F1 ≥ 0.5 should track
  the LoCoMo paper's reported correlation.

We **do not** post-train Cairn against this benchmark. Each evaluation is
zero-shot from Cairn's normal capture/retrieval path. If the score
improves, it's because the retrieval got better in general, not because
we tuned to LoCoMo specifically.

## Reproducibility checklist

- [ ] **Dataset hash** pinned in `data/locomo10.sha256`.
- [ ] **Cairn commit hash** recorded in every `results.json`.
- [ ] **Answerer model + temperature** recorded.
- [ ] **Judge model** recorded.
- [ ] **Cairn audit chain export** stored per run.
- [ ] Re-running the harness on the same Cairn build + same LLM + same
      seed produces F1 within ±0.5 pp.

## Implementation status

| Component | Status |
|---|---|
| Harness orchestrator (`harness.py`) | 🚧 skeleton — fill in conversation loop |
| Dataset fetcher (`run.sh fetch`) | 🚧 skeleton |
| Cairn HTTP client wrapper | 🚧 skeleton |
| F1 scorer (token-stemmed) | 🚧 skeleton |
| Judge LLM client | 🚧 skeleton |
| Cairn audit export per run | ⏳ planned |
| GitHub Action that runs benchmark on tagged releases | ⏳ planned |

Tracked in [issue #TBD](https://github.com/mutouwilson/cairn/issues).

## Why our numbers may differ from MemoryLake's

| Source of variance | Likely magnitude |
|---|---|
| Answerer LLM choice (Claude Haiku vs Sonnet vs GPT-4o) | ±2–5 pp |
| Judge LLM strictness on `open_domain` | ±1–3 pp |
| Cairn retrieval `top_k` and re-rank weights | ±2–4 pp |
| Token-stem implementation (NLTK vs HuggingFace) | ±0.5 pp |
| Dataset version (LoCoMo released a v1.1 with cleaned ground truths) | ±1 pp |

We commit ours and document each. If anyone can show that **with the
same answerer + judge + dataset version** we score worse than another
system, we'd like to know — that's a real bug.

## References

- Maharana et al., *"Evaluating Very Long-Term Conversational Memory of LLM Agents"*, [arXiv:2402.17753](https://arxiv.org/abs/2402.17753) (ACL 2024).
- Dataset: <https://github.com/snap-research/locomo>
- MemoryLake's published results JSON: <https://github.com/memorylake-ai/memorylake-locomo-benchmark>
