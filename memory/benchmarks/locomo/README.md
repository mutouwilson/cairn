# LoCoMo Benchmark Harness for Cairn

This is a **scaffold** for running Cairn (personal-memory product, Tauri 2 + Rust +
SQLite) against the LoCoMo benchmark. It is intentionally minimal: ingest a
conversation, query at each Q&A turn, score with token-F1, dump JSONL.

## What is LoCoMo?

**LoCoMo** — *"Evaluating Very Long-Term Conversational Memory of LLM Agents"*
(Maharana et al., 2024). It's the standard benchmark for memory systems: ~10
multi-session dialogues between two personas, each with ~600 question/answer
pairs spanning five categories:

- **single-hop** — a fact directly stated in one turn
- **multi-hop** — requires combining facts from multiple turns/sessions
- **temporal** — "when did X happen", relative ordering, durations
- **open-domain** — needs world knowledge in addition to dialogue context
- **adversarial** — the answer cannot be inferred; correct response is "I don't know"

Paper: <https://arxiv.org/abs/2402.17753>
Likely dataset slug: `snap-stanford/locomo10` on HuggingFace
(<https://huggingface.co/datasets/snap-stanford/locomo10>). **Please verify the
exact dataset name before running** — it may have moved or be gated, and the
LoCoMo project page lists a couple of variants.

## Methodology

```
for each conversation in dataset:
    # Phase 1 — ingest
    for each session in conversation:
        for each turn in session:
            POST /api/capture { text: turn.text, source: "locomo:<conv>:<sess>", metadata: {...} }
        sleep(--sleep-after-capture)   # let the LLM entity extractor catch up

    # Phase 2 — query
    for each qa in conversation.qa:
        GET /api/search?q=<question>&limit=20
        score retrieved entities + notes against golden answer
        write one JSONL line

print per-category F1 + overall
```

Scoring is **SQuAD-style token F1** (precision/recall over normalized tokens)
plus exact-match, computed against the union of retrieved entity names and
note texts. Whatever answer text appears in *any* retrieved item wins — we are
benchmarking the retriever, not a generator on top of it.

## Run

```bash
# 1. start Cairn somewhere — it should expose http://127.0.0.1:7717
# 2. install deps
pip install -r requirements.txt

# 3. smoke test with the bundled fixture
python run.py \
    --conversation fixtures/sample.json \
    --base-url http://127.0.0.1:7717 \
    --output results/sample.jsonl \
    --limit 10

# 4. for the real thing, download the HF dataset first
bash download_dataset.sh
python run.py --conversation data/locomo10.json --base-url http://127.0.0.1:7717
```

Useful flags:

| flag | default | what |
| --- | --- | --- |
| `--conversation` | _required_ | path to a JSON file in LoCoMo shape |
| `--base-url` | `http://127.0.0.1:7717` | Cairn HTTP endpoint |
| `--limit` | none | cap Q&A pairs (for smoke runs) |
| `--output` | `results/run.jsonl` | per-question JSONL output |
| `--ingest-only` | off | only run Phase 1 |
| `--query-only` | off | only run Phase 2 (assume Phase 1 already done) |
| `--sleep-after-capture` | `2.0` | seconds to wait between sessions so the async entity extractor has time |

## What we are NOT yet doing

- **LLM-as-judge.** LoCoMo's reference evaluation uses an LLM to compare a
  generated answer against the gold answer (paraphrase tolerance, semantic
  equivalence). We only do exact-match + token-F1 over retrieved text. A stub
  exists in `scoring.py` (`llm_judge`) that raises `NotImplementedError`.
- **Database reset between runs.** You must manually wipe Cairn's SQLite db
  between conversations or the corpus will accumulate. We may add an
  `/api/reset` endpoint later; for now it's on the operator.
- **Multiple conversations in one process.** `run.py` takes one
  `--conversation` per invocation. Loop in shell if you need to fan out.
- **Generator-side benchmarking.** No OpenAI/Anthropic clients are imported.
  We're measuring retrieval quality only.

## Schema assumptions

The bundled `fixtures/sample.json` uses these keys:

```json
{
  "conversation_id": "...",
  "speakers": ["Alice", "Bob"],
  "sessions": [
    {
      "session_id": "...",
      "timestamp": "ISO 8601",
      "turns": [
        {"turn_id": "...", "speaker": "Alice", "text": "...", "timestamp": "..."}
      ]
    }
  ],
  "qa": [
    {
      "question_id": "...",
      "category": "single-hop | multi-hop | temporal | open-domain | adversarial",
      "question": "...",
      "answer": "..."
    }
  ]
}
```

The real LoCoMo release uses slightly different field names (e.g.
`dialog`/`speaker_a`/`speaker_b`, evidence pointers, etc.). When you swap in
the real data, adjust the loader in `run.py::load_conversation` — it's
intentionally a single function.
