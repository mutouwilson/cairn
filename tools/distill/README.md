# `distill/` — On-device extraction model toolkit

End-to-end pipeline that distills a Claude Opus-quality entity-extraction model
into a 1.5B Qwen 2.5 LoRA suitable for on-device inference via MLX (Apple
Silicon) or llama.cpp (everywhere).

This produces the model that backs `extract/local.rs` in the main app, so the
contract is the same: input is a free-form note, output is the
`save_extraction` tool call payload that matches
`memory/src-tauri/src/extract/prompt.rs::EXTRACTION_SCHEMA`.

## Pipeline

```
1. synth.py  ──┐
   teacher = Claude Opus or any Vercel-Gateway model
   →           ├─► data/{train,test}.jsonl  (note → extraction pairs)
2. train.py  ──┤
   base   = Qwen/Qwen2.5-1.5B-Instruct
   LoRA   = rank 32, alpha 64
   →           ├─► runs/<tag>/adapter/      (LoRA weights)
3. eval.py   ──┤
   metrics: per-type F1, exact-JSON-match, mean latency
   →           └─► runs/<tag>/eval.json
4. export.py
   merges LoRA → base, quantizes for MLX 4-bit
   →           models/qwen-1.5b-extract-mlx/
5. serve via `mlx_lm.server` or `llama-server`
   wire into the app via CAIRN_EXTRACTION_PROVIDER=local
```

## Hardware

| Stage  | Minimum                              | Recommended                          |
| ------ | ------------------------------------ | ------------------------------------ |
| synth  | any CPU + network                    | parallelise N=16 against gateway     |
| train  | NVIDIA A10 24 GB / 4× T4 32 GB / M2 Max 32 GB unified | A100 80 GB                           |
| eval   | inference-only, fits on a laptop GPU | dedicated GPU for batch eval         |
| export | Mac with MLX installed               | M2/M3 Max                            |
| serve  | M-series Mac (MLX) or any GPU (gguf) | M2 Pro+ for 50–100 tok/s             |

LoRA training on a single A100 takes ~45 minutes for 20 000 examples × 3 epochs;
on an M2 Max 32 GB it's ~3 hours.

## Install

```bash
cd tools/distill
python3.11 -m venv .venv && source .venv/bin/activate
pip install -e .
```

(Use `python3.11` specifically — PyTorch + bitsandbytes + MLX prebuilts are
not yet uniformly available for 3.13 as of 2026-05.)

## End-to-end run (gateway teacher, MLX export)

```bash
export AI_GATEWAY_API_KEY=...

# 20k training pairs + 1k test pairs (≈ 30 min via gateway, parallel=16)
distill-synth --output data/train.jsonl --n 20000 --parallel 16
distill-synth --output data/test.jsonl  --n 1000  --parallel 8  --seed 42

# LoRA fine-tune (single GPU)
distill-train \
  --train data/train.jsonl --test data/test.jsonl \
  --base Qwen/Qwen2.5-1.5B-Instruct \
  --output runs/v1 --epochs 3 --batch-size 8 --lr 2e-4

# Eval
distill-eval --model runs/v1 --data data/test.jsonl --report runs/v1/eval.json

# Merge LoRA + 4-bit quantise for MLX
distill-export --run runs/v1 --output models/qwen-1.5b-extract-mlx --quant 4

# Serve
pip install mlx-lm
mlx_lm.server --model models/qwen-1.5b-extract-mlx --port 8080
```

Then in `memory/.env`:

```
CAIRN_EXTRACTION_PROVIDER=local
CAIRN_EXTRACTION_MODEL=qwen-1.5b-extract-mlx
CAIRN_LOCAL_ENDPOINT=http://localhost:8080/v1
```

## Quality targets (gates before shipping a new run)

| Metric                                          | Floor   | Target  |
| ----------------------------------------------- | ------- | ------- |
| Per-type F1 (Person/Preference/Event)           | ≥ 0.78  | ≥ 0.85  |
| Per-type F1 (Goal/Belief)                       | ≥ 0.70  | ≥ 0.80  |
| Exact JSON-schema validity rate                 | ≥ 0.985 | ≥ 0.998 |
| P50 latency M2 Max, batch=1, max-tokens=512     | < 200ms | < 100ms |
| P95 latency                                     | < 500ms | < 250ms |

Runs that don't clear the floors stay in `runs/` and don't get tagged.

## Layout

```
tools/distill/
├── README.md                    this file
├── pyproject.toml               distill-* console scripts
├── src/distill/
│   ├── __init__.py
│   ├── schema.py                JSON Schema + Pydantic mirror of EXTRACTION_SCHEMA
│   ├── synth.py                 generate (note, extraction) pairs via teacher
│   ├── train.py                 LoRA fine-tune
│   ├── eval.py                  metrics + report
│   ├── export.py                LoRA merge + MLX / GGUF quantise
│   ├── prompts.py               system + few-shot, kept in sync with the Rust prompt
│   └── seeds.py                 seed concepts for synth diversity
├── data/                        gitignored — generated jsonl + checkpoints
├── runs/                        gitignored — training artefacts
└── models/                      gitignored — exported quantised models
```

## Why distill instead of training from scratch

| Approach                       | Cost     | Quality | Notes                                             |
| ------------------------------ | -------- | ------- | ------------------------------------------------- |
| Prompt Claude Haiku every note | $0.001   | best    | flat per-note cost, requires network              |
| Prompt local Qwen 1.5B raw     | $0       | poor    | unreliable JSON, no chain-of-thought              |
| **Distil from Opus → Qwen**    | one-off  | strong  | one synth pass amortised across all future notes  |
| Train from scratch             | huge     | unknown | needs labelled data we don't have                 |

The third row is the only one that lets the product run offline at zero
per-note cost while keeping >80 % of Claude Haiku's extraction quality.

## Notes on reproducibility

- `synth` is deterministic given `--seed`; the same seed against the same teacher model produces byte-identical JSONL.
- Trained adapter weights + the exact training command line are written into `runs/<tag>/manifest.json`.
- Eval results are appended to `runs/<tag>/eval.json` with a timestamp so we keep history of how a run performed under different evaluators.

## Known limits (filed, not punted)

- LoRA on a 1.5B base saturates around 35 k examples; pushing past that needs full fine-tune or QLoRA on a bigger base (3B / 7B).
- `bitsandbytes` 4-bit inference isn't supported on Apple Silicon → use MLX path on Macs and bitsandbytes on Linux/CUDA only.
- The MLX server doesn't yet honour `tool_choice="required"`; we lean on Qwen's JSON-mode (`response_format`) plus an instructions block. Re-evaluate when MLX adds native tool-choice support.
