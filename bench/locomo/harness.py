"""LoCoMo benchmark harness for Cairn.

Skeleton — the bones of the pipeline are here so reviewers can audit the
shape end-to-end. The actual conversation loop, retrieval call and LLM
answerer are TODOs marked inline.

Usage:
    python harness.py fetch                                  # pull dataset
    python harness.py evaluate \\
        --conversations all \\
        --tasks single_hop multi_hop temporal open_domain \\
        --answerer claude-haiku-4-5-20251001

See README.md for the full design and reproducibility rules.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Iterable

HERE = Path(__file__).resolve().parent
DATA_DIR = HERE / "data"
RESULTS_DIR = HERE / "results"

LOCOMO_REPO = "https://github.com/snap-research/locomo.git"
LOCOMO_DATA_FILE = "locomo10.json"  # canonical filename inside that repo


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------


@dataclass
class Conversation:
    """One LoCoMo conversation: a long sequence of turns + a list of Qs/As."""

    id: str
    turns: list[dict]
    questions: list["Question"]


@dataclass
class Question:
    id: str
    task: str  # single_hop | multi_hop | temporal | open_domain
    text: str
    gold: str


@dataclass
class Prediction:
    qid: str
    task: str
    predicted: str
    gold: str
    retrieved_context: str  # what cairn handed us, for trace inspection
    f1: float = 0.0
    judge_accept: bool | None = None


@dataclass
class RunSummary:
    cairn_commit: str
    answerer: str
    judge: str
    dataset_sha256: str
    started_at: float
    finished_at: float = 0.0
    per_task_f1: dict[str, float] = field(default_factory=dict)
    overall_f1: float = 0.0
    n_questions: int = 0


# ---------------------------------------------------------------------------
# Dataset fetch
# ---------------------------------------------------------------------------


def fetch_dataset() -> Path:
    """Clone or update snap-research/locomo into ./data/ and verify hash."""
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    repo_dir = DATA_DIR / "locomo"
    if not repo_dir.exists():
        print(f"cloning {LOCOMO_REPO} → {repo_dir}")
        subprocess.run(
            ["git", "clone", "--depth", "1", LOCOMO_REPO, str(repo_dir)],
            check=True,
        )
    else:
        print(f"updating {repo_dir}")
        subprocess.run(["git", "-C", str(repo_dir), "pull", "--ff-only"], check=True)
    data_path = repo_dir / "data" / LOCOMO_DATA_FILE
    if not data_path.exists():
        # Layout has shifted between paper releases; search shallowly.
        candidates = list(repo_dir.rglob(LOCOMO_DATA_FILE))
        if not candidates:
            raise FileNotFoundError(f"{LOCOMO_DATA_FILE} not found under {repo_dir}")
        data_path = candidates[0]
    sha = _sha256_file(data_path)
    (DATA_DIR / "locomo10.sha256").write_text(f"{sha}  {LOCOMO_DATA_FILE}\n")
    print(f"  dataset sha256: {sha}")
    return data_path


def load_dataset(path: Path) -> list[Conversation]:
    raw = json.loads(path.read_text())
    out: list[Conversation] = []
    for item in raw:
        # TODO: snap-research/locomo's actual schema may differ. Fix the
        # field names after we pull a real copy.
        out.append(
            Conversation(
                id=str(item.get("id") or item.get("conversation_id")),
                turns=list(item.get("conversation", [])),
                questions=[
                    Question(
                        id=str(q.get("id")),
                        task=str(q.get("type") or q.get("task")),
                        text=str(q.get("question")),
                        gold=str(q.get("answer") or q.get("gold")),
                    )
                    for q in item.get("qa", [])
                ],
            )
        )
    return out


# ---------------------------------------------------------------------------
# Cairn client
# ---------------------------------------------------------------------------


class CairnClient:
    """Thin HTTP wrapper around cairn's local API surface.

    Cairn exposes a small REST surface on localhost while the app is
    running (see memory/src-tauri/src/api.rs). We use it instead of MCP
    stdio because Python on Linux can hit it without a Claude Desktop
    sidecar.
    """

    def __init__(self, base_url: str, agent_id: str):
        self.base_url = base_url.rstrip("/")
        self.agent_id = agent_id

    def reset(self) -> None:
        """Wipe Cairn DB so each conversation starts fresh.

        TODO: this assumes a dev-mode endpoint or we point Cairn at a
        temp `CAIRN_DATA_DIR` per conversation. The latter is cleaner.
        """
        raise NotImplementedError

    def capture(self, text: str, source: str = "locomo-harness") -> str:
        """POST /api/v1/capture — return note_id."""
        raise NotImplementedError

    def search(self, query: str, top_k: int = 8) -> list[dict]:
        """POST /api/v1/search — return ranked context items."""
        raise NotImplementedError


# ---------------------------------------------------------------------------
# Answerer LLM
# ---------------------------------------------------------------------------


def answer_question(question: str, context: str, *, model: str) -> str:
    """Send (question, retrieved-context) to the answerer LLM.

    TODO: provider router. For Anthropic, hit /v1/messages with a system
    prompt instructing extractive-QA style answers. Keep it stateless —
    each question is a fresh call.
    """
    raise NotImplementedError


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------


_token_split = re.compile(r"\w+")


def tokenize(s: str) -> list[str]:
    return _token_split.findall(s.lower())


def f1_score(pred: str, gold: str) -> float:
    """Token-stem F1 — matches the LoCoMo paper's scorer."""
    pred_tokens = tokenize(pred)
    gold_tokens = tokenize(gold)
    if not pred_tokens or not gold_tokens:
        return 0.0
    common: dict[str, int] = {}
    for t in pred_tokens:
        common[t] = min(pred_tokens.count(t), gold_tokens.count(t))
    overlap = sum(common.values())
    if overlap == 0:
        return 0.0
    precision = overlap / len(pred_tokens)
    recall = overlap / len(gold_tokens)
    return 2 * precision * recall / (precision + recall)


# ---------------------------------------------------------------------------
# Run loop
# ---------------------------------------------------------------------------


def evaluate(
    conversations: list[Conversation],
    *,
    tasks: Iterable[str],
    answerer_model: str,
    judge_model: str,
    cairn: CairnClient,
) -> tuple[list[Prediction], RunSummary]:
    preds: list[Prediction] = []
    summary = RunSummary(
        cairn_commit=_cairn_commit(),
        answerer=answerer_model,
        judge=judge_model,
        dataset_sha256=(DATA_DIR / "locomo10.sha256").read_text().split()[0]
        if (DATA_DIR / "locomo10.sha256").exists()
        else "unknown",
        started_at=time.time(),
    )
    selected_tasks = set(tasks)

    for conv in conversations:
        # TODO: cairn.reset() + ingest every turn before querying.
        print(f"  conversation {conv.id}: {len(conv.turns)} turns, "
              f"{len(conv.questions)} questions")
        for turn in conv.turns:
            # cairn.capture(_render_turn(turn))
            pass

        for q in conv.questions:
            if q.task not in selected_tasks:
                continue
            # TODO: hits = cairn.search(q.text)
            # TODO: context = "\n".join(h["text"] for h in hits)
            # TODO: pred = answer_question(q.text, context, model=answerer_model)
            context = ""
            pred = ""
            f1 = f1_score(pred, q.gold)
            preds.append(
                Prediction(
                    qid=q.id, task=q.task,
                    predicted=pred, gold=q.gold,
                    retrieved_context=context, f1=f1,
                )
            )

    summary.finished_at = time.time()
    summary.n_questions = len(preds)
    by_task: dict[str, list[float]] = {}
    for p in preds:
        by_task.setdefault(p.task, []).append(p.f1)
    summary.per_task_f1 = {t: sum(v) / len(v) for t, v in by_task.items() if v}
    if preds:
        summary.overall_f1 = sum(p.f1 for p in preds) / len(preds)
    return preds, summary


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def cli() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("fetch", help="clone the LoCoMo dataset into ./data")

    ev = sub.add_parser("evaluate", help="run the full pipeline")
    ev.add_argument("--conversations", default="all")
    ev.add_argument("--tasks", nargs="+",
                    default=["single_hop", "multi_hop", "temporal", "open_domain"])
    ev.add_argument("--answerer", default=os.environ.get("LLM_MODEL", "claude-haiku-4-5-20251001"))
    ev.add_argument("--judge", default=os.environ.get("JUDGE_MODEL", "claude-sonnet-4-7"))
    ev.add_argument("--api-url", default=os.environ.get("CAIRN_API_URL", "http://localhost:8787"))
    ev.add_argument("--agent-id", default=os.environ.get("CAIRN_AGENT_ID", "locomo-harness"))

    args = parser.parse_args()

    if args.cmd == "fetch":
        fetch_dataset()
        return 0

    if args.cmd == "evaluate":
        ds_path = next(DATA_DIR.rglob(LOCOMO_DATA_FILE), None)
        if ds_path is None:
            print("dataset not present — run `python harness.py fetch` first", file=sys.stderr)
            return 2
        convs = load_dataset(ds_path)
        if args.conversations != "all":
            wanted = set(args.conversations.split(","))
            convs = [c for c in convs if c.id in wanted]
        cairn = CairnClient(args.api_url, args.agent_id)
        preds, summary = evaluate(
            convs, tasks=args.tasks,
            answerer_model=args.answerer, judge_model=args.judge,
            cairn=cairn,
        )
        out_dir = RESULTS_DIR / time.strftime("%Y%m%d-%H%M%S")
        out_dir.mkdir(parents=True, exist_ok=True)
        (out_dir / "results.json").write_text(
            json.dumps([asdict(p) for p in preds], indent=2, ensure_ascii=False)
        )
        (out_dir / "summary.json").write_text(
            json.dumps(asdict(summary), indent=2, ensure_ascii=False)
        )
        latest = RESULTS_DIR / "latest"
        if latest.exists() or latest.is_symlink():
            latest.unlink()
        latest.symlink_to(out_dir.name)
        print(f"\nOverall F1: {summary.overall_f1:.4f} ({summary.n_questions} questions)")
        for task, f1 in summary.per_task_f1.items():
            print(f"  {task:14s} {f1:.4f}")
        print(f"\n→ {out_dir}")
        return 0

    return 1


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _cairn_commit() -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(HERE.parent.parent), "rev-parse", "HEAD"],
            check=True, capture_output=True, text=True,
        )
        return out.stdout.strip()
    except (subprocess.SubprocessError, FileNotFoundError):
        return "unknown"


if __name__ == "__main__":
    sys.exit(cli())
