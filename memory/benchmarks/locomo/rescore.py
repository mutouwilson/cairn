"""Re-score the JSONL output without re-running queries.

Used after a category-mapping bugfix: every row in the JSONL keeps the raw
retrieved entities/notes + golden answer, so we can replay the scorer with
the corrected `--cat-map` and emit a fresh summary without burning more
network calls.
"""

import argparse
import glob
import json
import os
from collections import defaultdict

from scoring import score_record, score_adversarial


# Same source of truth as run.LOCOMO_CATEGORY_MAP; duplicated here so this
# script keeps working if run.py drifts. Adjust if you ever discover the
# mapping is wrong again.
CATEGORY_MAP_BY_INT = {
    1: "single-hop",
    2: "temporal",
    3: "open-domain",
    4: "multi-hop",
    5: "adversarial",
}

# Original (buggy) mapping that produced full.conv*.jsonl, so we can flip
# from old-label to int and then forward to the right label.
OLD_MAP = {
    "adversarial": 1,
    "temporal": 2,
    "open-domain": 3,
    "multi-hop": 4,
    "single-hop": 5,
}


def fix_category(old_label):
    int_cat = OLD_MAP.get(old_label)
    if int_cat is None:
        return old_label
    return CATEGORY_MAP_BY_INT.get(int_cat, old_label)


def rescore_file(path):
    out_rows = []
    by_cat = defaultdict(list)
    with open(path) as f:
        for line in f:
            rec = json.loads(line)
            new_cat = fix_category(rec["category"])
            golden = rec["golden"]
            candidates = (rec.get("retrieved_entities") or []) + (rec.get("retrieved_notes") or [])
            scorer = score_adversarial if new_cat == "adversarial" else score_record
            scored = scorer(candidates, golden)
            rec["category"] = new_cat
            rec["exact_match"] = scored["exact"]
            rec["f1"] = scored["f1"]
            rec["best_retrieval_idx"] = scored["best_retrieval_idx"]
            out_rows.append(rec)
            by_cat[new_cat].append((scored["exact"], scored["f1"]))
    return out_rows, by_cat


def merge_by_cat(buckets):
    out = defaultdict(list)
    for b in buckets:
        for k, v in b.items():
            out[k].extend(v)
    return out


def print_summary(by_cat):
    print("\n=== LoCoMo summary (rescored) ===")
    print(f"{'category':<14} {'n':>5} {'EM':>8} {'F1':>8}")
    print("-" * 40)
    total_em, total_f1, total_n = 0, 0.0, 0
    for cat in sorted(by_cat.keys()):
        rows = by_cat[cat]
        n = len(rows)
        em = sum(r[0] for r in rows) / n if n else 0
        f1 = sum(r[1] for r in rows) / n if n else 0
        print(f"{cat:<14} {n:>5} {em:>8.3f} {f1:>8.3f}")
        total_n += n
        total_em += sum(r[0] for r in rows)
        total_f1 += sum(r[1] for r in rows)
    if total_n:
        print("-" * 40)
        print(f"{'OVERALL':<14} {total_n:>5} {total_em/total_n:>8.3f} {total_f1/total_n:>8.3f}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--glob", default="results/full.conv*.jsonl")
    p.add_argument("--rewrite", action="store_true", help="overwrite the JSONL files with corrected scores")
    args = p.parse_args()

    files = sorted(glob.glob(args.glob))
    if not files:
        print(f"no files matching {args.glob}")
        return 1

    all_buckets = []
    for path in files:
        rows, by_cat = rescore_file(path)
        if args.rewrite:
            with open(path, "w") as f:
                for r in rows:
                    f.write(json.dumps(r) + "\n")
        all_buckets.append(by_cat)

    print_summary(merge_by_cat(all_buckets))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
