"""Evaluate a trained extraction model on the held-out test set.

Metrics:

* **Per-type F1**: micro-averaged over entities of each type. Entity match =
  same (type, name) modulo trivial whitespace/case.
* **Relation F1**: same as entity F1 but on relation triples.
* **JSON validity rate**: fraction of model outputs that parse and validate
  against the extraction schema.
* **Latency**: end-to-end mean / P50 / P95 seconds per record.

Writes a structured report to ``<run-dir>/eval.json`` and prints a summary
table to stderr.
"""

from __future__ import annotations

import json
import logging
import statistics
import time
from collections import defaultdict
from pathlib import Path
from typing import Any

import click
import orjson
import torch
from peft import PeftModel
from rich.console import Console
from rich.table import Table
from tqdm.auto import tqdm
from transformers import AutoModelForCausalLM, AutoTokenizer

from .prompts import SERVING_SYSTEM
from .schema import ENTITY_TYPES, validate_payload

log = logging.getLogger(__name__)
console = Console(stderr=True)

TOOL_CALL_OPEN = "<tool_call>"
TOOL_CALL_CLOSE = "</tool_call>"


def _normalise_name(s: str) -> str:
    return s.strip().casefold()


def _entity_pairs(extraction: dict[str, Any]) -> set[tuple[str, str]]:
    return {(e["type"], _normalise_name(e["name"])) for e in extraction.get("entities", [])}


def _relation_triples(extraction: dict[str, Any]) -> set[tuple[str, str, str]]:
    return {
        (_normalise_name(r["from"]), _normalise_name(r["to"]), r["type"])
        for r in extraction.get("relations", [])
    }


def _f1(tp: int, fp: int, fn: int) -> tuple[float, float, float]:
    precision = tp / (tp + fp) if (tp + fp) else 0.0
    recall = tp / (tp + fn) if (tp + fn) else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) else 0.0
    return precision, recall, f1


def _parse_tool_call(text: str) -> dict[str, Any] | None:
    if TOOL_CALL_OPEN in text and TOOL_CALL_CLOSE in text:
        inner = text.split(TOOL_CALL_OPEN, 1)[1].split(TOOL_CALL_CLOSE, 1)[0].strip()
    else:
        inner = text.strip()
    try:
        obj = orjson.loads(inner)
    except orjson.JSONDecodeError:
        return None
    if isinstance(obj, dict) and "arguments" in obj:
        args = obj["arguments"]
        if isinstance(args, str):
            try:
                return orjson.loads(args)
            except orjson.JSONDecodeError:
                return None
        if isinstance(args, dict):
            return args
    if isinstance(obj, dict) and "entities" in obj:
        return obj
    return None


def load_model(model_dir: Path, base_model: str | None) -> tuple[Any, Any]:
    tokenizer = AutoTokenizer.from_pretrained(model_dir / "adapter", trust_remote_code=True)
    base_name = base_model or _read_base_model(model_dir)
    base = AutoModelForCausalLM.from_pretrained(
        base_name,
        trust_remote_code=True,
        torch_dtype=torch.bfloat16 if torch.cuda.is_available() else torch.float32,
    )
    model = PeftModel.from_pretrained(base, model_dir / "adapter")
    model.eval()
    if torch.cuda.is_available():
        model = model.cuda()
    return model, tokenizer


def _read_base_model(model_dir: Path) -> str:
    manifest = model_dir / "manifest.json"
    if manifest.exists():
        return json.loads(manifest.read_text())["base_model"]
    raise click.ClickException(
        f"missing {manifest}; pass --base to specify the base model explicitly"
    )


def _generate(model, tokenizer, note: str, max_new: int = 1024) -> tuple[str, float]:
    messages = [
        {"role": "system", "content": SERVING_SYSTEM},
        {"role": "user", "content": note},
    ]
    inputs = tokenizer.apply_chat_template(
        messages, return_tensors="pt", add_generation_prompt=True
    )
    if torch.cuda.is_available():
        inputs = inputs.cuda()
    started = time.perf_counter()
    with torch.no_grad():
        out = model.generate(
            inputs,
            max_new_tokens=max_new,
            do_sample=False,
            pad_token_id=tokenizer.pad_token_id or tokenizer.eos_token_id,
        )
    elapsed = time.perf_counter() - started
    text = tokenizer.decode(out[0][inputs.shape[1] :], skip_special_tokens=False)
    return text, elapsed


@click.command()
@click.option("--model", "model_dir", type=click.Path(file_okay=False, exists=True, path_type=Path), required=True)
@click.option("--data", "data_path", type=click.Path(dir_okay=False, exists=True, path_type=Path), required=True)
@click.option("--report", "report_path", type=click.Path(dir_okay=False, path_type=Path), default=None)
@click.option("--max-rows", type=int, default=None, help="cap eval set size for fast smoke tests")
@click.option("--base", "base_model", default=None, help="override base model if manifest is missing")
@click.option("--log-level", default="INFO")
def main(
    model_dir: Path,
    data_path: Path,
    report_path: Path | None,
    max_rows: int | None,
    base_model: str | None,
    log_level: str,
) -> None:
    logging.basicConfig(level=log_level.upper(), format="%(asctime)s %(levelname)s %(message)s")
    if report_path is None:
        report_path = model_dir / "eval.json"

    rows: list[dict[str, Any]] = []
    with data_path.open("rb") as f:
        for line in f:
            if not line.strip():
                continue
            rows.append(orjson.loads(line))
            if max_rows and len(rows) >= max_rows:
                break

    model, tokenizer = load_model(model_dir, base_model)

    tp_by_type: dict[str, int] = defaultdict(int)
    fp_by_type: dict[str, int] = defaultdict(int)
    fn_by_type: dict[str, int] = defaultdict(int)

    rel_tp = rel_fp = rel_fn = 0
    valid = 0
    latencies: list[float] = []

    for r in tqdm(rows, desc="eval"):
        text, elapsed = _generate(model, tokenizer, r["note"])
        latencies.append(elapsed)
        pred = _parse_tool_call(text)
        if pred is None or validate_payload(pred):
            for e in r["extraction"]["entities"]:
                fn_by_type[e["type"]] += 1
            rel_fn += len(r["extraction"]["relations"])
            continue
        valid += 1

        gold_e = _entity_pairs(r["extraction"])
        pred_e = _entity_pairs(pred)
        for t in ENTITY_TYPES:
            gold_t = {p for p in gold_e if p[0] == t}
            pred_t = {p for p in pred_e if p[0] == t}
            tp_by_type[t] += len(gold_t & pred_t)
            fp_by_type[t] += len(pred_t - gold_t)
            fn_by_type[t] += len(gold_t - pred_t)

        gold_r = _relation_triples(r["extraction"])
        pred_r = _relation_triples(pred)
        rel_tp += len(gold_r & pred_r)
        rel_fp += len(pred_r - gold_r)
        rel_fn += len(gold_r - pred_r)

    report: dict[str, Any] = {
        "rows": len(rows),
        "valid_json_rate": valid / max(1, len(rows)),
        "entity_f1": {},
        "relation_f1": _f1(rel_tp, rel_fp, rel_fn)[2],
        "latency_seconds": {
            "mean": statistics.mean(latencies) if latencies else 0.0,
            "p50": statistics.median(latencies) if latencies else 0.0,
            "p95": statistics.quantiles(latencies, n=20)[-1] if len(latencies) >= 20 else 0.0,
        },
    }

    table = Table(title=f"Eval @ {model_dir}")
    table.add_column("Type")
    table.add_column("P", justify="right")
    table.add_column("R", justify="right")
    table.add_column("F1", justify="right")
    table.add_column("n", justify="right")
    for t in ENTITY_TYPES:
        p, r_, f1 = _f1(tp_by_type[t], fp_by_type[t], fn_by_type[t])
        n = tp_by_type[t] + fn_by_type[t]
        report["entity_f1"][t] = {"precision": p, "recall": r_, "f1": f1, "n": n}
        table.add_row(t, f"{p:.3f}", f"{r_:.3f}", f"{f1:.3f}", str(n))
    rp, rr, rf = _f1(rel_tp, rel_fp, rel_fn)
    table.add_row("Relations", f"{rp:.3f}", f"{rr:.3f}", f"{rf:.3f}", str(rel_tp + rel_fn))
    console.print(table)
    console.print(f"valid-json-rate = {report['valid_json_rate']:.3f}")
    console.print(
        f"latency (s)  mean={report['latency_seconds']['mean']:.3f}  "
        f"p50={report['latency_seconds']['p50']:.3f}  "
        f"p95={report['latency_seconds']['p95']:.3f}"
    )

    report_path.write_text(json.dumps(report, indent=2))
    console.print(f"[green]report → {report_path}[/green]")


if __name__ == "__main__":
    main()
