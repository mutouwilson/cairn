"""LoCoMo harness for Cairn.

Two phases: ingest the conversation into Cairn via /api/capture, then query
each Q&A pair via /api/search and score the retrieval. Output is JSONL,
one record per question.
"""

import argparse
import json
import os
import sys
import time
from collections import defaultdict

import requests
from tqdm import tqdm

from scoring import f1_token, exact_match, score_record, score_adversarial


# LoCoMo's five canonical question categories. We accept anything but bucket
# unknown labels into "other" so the summary still totals.
KNOWN_CATEGORIES = {
    "single-hop",
    "multi-hop",
    "temporal",
    "open-domain",
    "adversarial",
}


def load_conversation(path, conv_index=0):
    """Read a LoCoMo-shaped JSON file and normalise to internal shape.

    Handles two layouts:
      A. Bundled fixture: `{sessions: [{session_id, turns: [{text, speaker, ...}]}], qa: [...]}`
      B. Real LoCoMo (snap-research/locomo): top-level is a list of 10
         conversations; each has `{conversation: {speaker_a, speaker_b,
         session_1: [...], session_1_date_time, ...}, qa: [{question,
         answer, evidence: ["D1:3"], category: int}], sample_id}`.

    Pass `conv_index` (default 0) to pick one conversation when given B.
    Returns the same internal shape regardless: `{conversation_id, sessions, qa}`.
    """
    with open(path) as f:
        raw = json.load(f)

    # Layout B (real LoCoMo): top-level list of conversations
    if isinstance(raw, list):
        if conv_index >= len(raw):
            raise IndexError(f"--conv-index {conv_index} but only {len(raw)} conversations")
        conv = raw[conv_index]
        return _normalize_real_locomo(conv, conv_index)

    # Layout A (our fixture)
    conv_id = raw.get("conversation_id") or raw.get("id") or os.path.basename(path)
    sessions = raw.get("sessions") or raw.get("dialog") or []
    qa = raw.get("qa") or raw.get("questions") or []
    return {"conversation_id": conv_id, "sessions": sessions, "qa": qa, "raw": raw}


# Real LoCoMo uses int categories. Mapping verified empirically by sampling
# the dataset (2026-05-15): category 5 always has answer=None ⇒ adversarial;
# category 2 has "When did …" questions ⇒ temporal; category 1 has direct
# fact lookups ("What did X research?") ⇒ single-hop.
LOCOMO_CATEGORY_MAP = {
    1: "single-hop",
    2: "temporal",
    3: "open-domain",
    4: "multi-hop",
    5: "adversarial",
}


def _normalize_real_locomo(conv, conv_index):
    """Real LoCoMo → our internal shape."""
    conv_id = conv.get("sample_id") or f"locomo-{conv_index}"
    inner = conv.get("conversation", {})
    sessions = []
    # Sessions live as flat session_<n> / session_<n>_date_time keys.
    session_ids = sorted(
        {k[len("session_"):] for k in inner.keys()
         if k.startswith("session_") and not k.endswith("_date_time")},
        key=lambda s: int(s) if s.isdigit() else 999,
    )
    for sid in session_ids:
        turns_raw = inner.get(f"session_{sid}") or []
        timestamp = inner.get(f"session_{sid}_date_time")
        turns = []
        for t in turns_raw:
            turns.append({
                "text": t.get("text") or "",
                "speaker": t.get("speaker"),
                "turn_id": t.get("dia_id"),
                "timestamp": timestamp,
            })
        sessions.append({"session_id": f"s{sid}", "timestamp": timestamp, "turns": turns})

    qa = []
    for q in conv.get("qa", []):
        cat_raw = q.get("category")
        cat = LOCOMO_CATEGORY_MAP.get(cat_raw, f"cat-{cat_raw}") if cat_raw is not None else "other"
        qa.append({
            "question_id": q.get("question_id") or f"{conv_id}-{len(qa)}",
            "question": q.get("question") or "",
            "answer": q.get("answer") if q.get("answer") is not None else "",
            "category": cat,
            "evidence": q.get("evidence", []),
        })
    return {"conversation_id": conv_id, "sessions": sessions, "qa": qa, "raw": conv}


def ingest(conv, base_url, sleep_after_capture):
    """Phase 1: POST every turn to /api/capture.

    We sleep between sessions (not between turns) so the async entity
    extractor has a chance to drain. Tune --sleep-after-capture if Cairn's
    queue is slower in your build.
    """
    url = f"{base_url.rstrip('/')}/api/capture"
    conv_id = conv["conversation_id"]
    n_turns = sum(len(s.get("turns", [])) for s in conv["sessions"])
    pbar = tqdm(total=n_turns, desc="ingest", unit="turn")
    for session in conv["sessions"]:
        session_id = session.get("session_id") or session.get("id") or "s?"
        for turn in session.get("turns", []):
            payload = {
                "text": turn.get("text") or turn.get("utterance") or "",
                "source": f"locomo:{conv_id}:{session_id}",
                "metadata": {
                    "turn_id": turn.get("turn_id") or turn.get("id"),
                    "speaker": turn.get("speaker"),
                    "timestamp": turn.get("timestamp") or session.get("timestamp"),
                },
            }
            if not payload["text"]:
                pbar.update(1)
                continue
            try:
                r = requests.post(url, json=payload, timeout=30)
                r.raise_for_status()
            except requests.RequestException as e:
                tqdm.write(f"[capture-fail] {session_id}/{payload['metadata'].get('turn_id')}: {e}")
            pbar.update(1)
        # Drain pause between sessions to let the LLM extractor catch up.
        time.sleep(sleep_after_capture)
    pbar.close()


def query_one(base_url, question, limit=20):
    """GET /api/search and pull the texts the retriever returned."""
    url = f"{base_url.rstrip('/')}/api/search"
    t0 = time.perf_counter()
    r = requests.get(url, params={"q": question, "limit": limit}, timeout=30)
    r.raise_for_status()
    elapsed_ms = int((time.perf_counter() - t0) * 1000)
    data = r.json()
    entity_names = [e.get("entity", {}).get("name") or "" for e in data.get("entities", [])]
    note_texts = [n.get("note", {}).get("text") or n.get("note", {}).get("content") or ""
                  for n in data.get("notes", [])]
    return {
        "entity_names": [e for e in entity_names if e],
        "note_texts": [t for t in note_texts if t],
        "elapsed_ms": elapsed_ms,
        "raw_diagnostics": data.get("diagnostics"),
    }


def query_and_score(conv, base_url, limit_qa, output_path):
    """Phase 2: iterate Q&A, score, write JSONL, return per-category aggregate."""
    qa = conv["qa"]
    if limit_qa:
        qa = qa[:limit_qa]
    os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
    by_cat = defaultdict(list)  # category -> list of (em, f1)

    with open(output_path, "w") as out:
        for q in tqdm(qa, desc="query", unit="q"):
            qid = q.get("question_id") or q.get("id") or ""
            category = q.get("category") or "other"
            question = q.get("question") or ""
            golden = q.get("answer") or q.get("gold") or ""
            try:
                ret = query_one(base_url, question, limit=20)
            except requests.RequestException as e:
                tqdm.write(f"[search-fail] {qid}: {e}")
                continue
            candidates = ret["entity_names"] + ret["note_texts"]
            scorer = score_adversarial if category == "adversarial" else score_record
            scored = scorer(candidates, golden)
            record = {
                "question_id": qid,
                "category": category,
                "question": question,
                "golden": golden,
                "retrieved_entities": ret["entity_names"],
                "retrieved_notes": ret["note_texts"],
                "exact_match": scored["exact"],
                "f1": scored["f1"],
                "best_retrieval_idx": scored["best_retrieval_idx"],
                "retrieval_time_ms": ret["elapsed_ms"],
            }
            out.write(json.dumps(record) + "\n")
            by_cat[category].append((scored["exact"], scored["f1"]))
    return by_cat


def print_summary(by_cat):
    """Pretty-print per-category EM/F1 + overall."""
    print("\n=== LoCoMo summary ===")
    print(f"{'category':<14} {'n':>5} {'EM':>8} {'F1':>8}")
    print("-" * 40)
    total_em, total_f1, total_n = 0, 0.0, 0
    for cat in sorted(by_cat.keys()):
        rows = by_cat[cat]
        n = len(rows)
        em = sum(r[0] for r in rows) / n if n else 0
        f1 = sum(r[1] for r in rows) / n if n else 0
        flag = "" if cat in KNOWN_CATEGORIES else " (unknown cat)"
        print(f"{cat:<14} {n:>5} {em:>8.3f} {f1:>8.3f}{flag}")
        total_n += n
        total_em += sum(r[0] for r in rows)
        total_f1 += sum(r[1] for r in rows)
    if total_n:
        print("-" * 40)
        print(f"{'OVERALL':<14} {total_n:>5} {total_em/total_n:>8.3f} {total_f1/total_n:>8.3f}")


def build_parser():
    p = argparse.ArgumentParser(description="Run Cairn against a LoCoMo conversation.")
    p.add_argument("--conversation", required=True, help="path to JSON file (LoCoMo shape)")
    p.add_argument("--conv-index", type=int, default=0,
                   help="when the JSON is a list of conversations (real LoCoMo), pick this one")
    p.add_argument("--base-url", default="http://127.0.0.1:7717")
    p.add_argument("--limit", type=int, default=None, help="cap Q&A pairs for smoke runs")
    p.add_argument("--session-limit", type=int, default=None,
                   help="ingest only the first N sessions of the conversation")
    p.add_argument("--output", default="results/run.jsonl")
    p.add_argument("--ingest-only", action="store_true")
    p.add_argument("--query-only", action="store_true")
    p.add_argument("--sleep-after-capture", type=float, default=2.0,
                   help="seconds to wait between sessions for the extractor")
    p.add_argument("--all-convs", action="store_true",
                   help="when --conversation is a list-shaped real LoCoMo, ingest+query every conversation")
    p.add_argument("--drain-seconds", type=int, default=0,
                   help="extra sleep after all ingest, before query phase, so the async extractor catches up")
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    if args.ingest_only and args.query_only:
        print("--ingest-only and --query-only are mutually exclusive", file=sys.stderr)
        return 2
    if args.all_convs:
        with open(args.conversation) as f:
            raw = json.load(f)
        if not isinstance(raw, list):
            print("--all-convs needs the real list-shaped LoCoMo JSON", file=sys.stderr)
            return 2
        n_convs = len(raw)
        # Phase 1: ingest every conversation
        if not args.query_only:
            for i in range(n_convs):
                conv = load_conversation(args.conversation, conv_index=i)
                if args.session_limit:
                    conv["sessions"] = conv["sessions"][: args.session_limit]
                print(f"\n=== Ingesting conv {i+1}/{n_convs} ({conv['conversation_id']}) ===", flush=True)
                ingest(conv, args.base_url, args.sleep_after_capture)
            if args.drain_seconds > 0:
                print(f"\nDraining {args.drain_seconds}s so the extractor catches up …", flush=True)
                drain = tqdm(total=args.drain_seconds, desc="drain", unit="s")
                t0 = time.time()
                while time.time() - t0 < args.drain_seconds:
                    time.sleep(min(2.0, args.drain_seconds - (time.time() - t0)))
                    drain.update(min(2, args.drain_seconds - int(drain.n)))
                drain.close()
        # Phase 2: query each conv, accumulate per-cat
        by_cat_all = defaultdict(list)
        if not args.ingest_only:
            base_out = args.output
            os.makedirs(os.path.dirname(base_out) or ".", exist_ok=True)
            for i in range(n_convs):
                conv = load_conversation(args.conversation, conv_index=i)
                stem, ext = os.path.splitext(base_out)
                out_path = f"{stem}.conv{i}{ext or '.jsonl'}"
                print(f"\n=== Querying conv {i+1}/{n_convs} ({conv['conversation_id']}) — {len(conv['qa'])} QA ===", flush=True)
                by_cat = query_and_score(conv, args.base_url, args.limit, out_path)
                for k, v in by_cat.items():
                    by_cat_all[k].extend(v)
            print_summary(by_cat_all)
        return 0

    # Single-conversation path
    conv = load_conversation(args.conversation, conv_index=args.conv_index)
    if args.session_limit:
        conv["sessions"] = conv["sessions"][: args.session_limit]
    if not args.query_only:
        ingest(conv, args.base_url, args.sleep_after_capture)
    if args.drain_seconds > 0 and not args.ingest_only:
        print(f"\nDraining {args.drain_seconds}s ...")
        time.sleep(args.drain_seconds)
    if not args.ingest_only:
        by_cat = query_and_score(conv, args.base_url, args.limit, args.output)
        print_summary(by_cat)
    return 0


if __name__ == "__main__":
    sys.exit(main())
