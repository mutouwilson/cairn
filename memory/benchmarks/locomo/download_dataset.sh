#!/usr/bin/env bash
# Pull the LoCoMo dataset from HuggingFace.
#
# You may need to `huggingface-cli login` first; LoCoMo may be gated (the
# authors gate some long-form benchmarks to discourage train-set leakage).
#
# If the slug below 404s, check https://huggingface.co/snap-stanford for the
# current name — at time of writing the candidates are:
#   - snap-stanford/locomo10
#   - snap-stanford/LoCoMo
# Adjust DATASET below as needed.

set -euo pipefail

DATASET="${1:-snap-stanford/locomo10}"
OUT_DIR="${2:-./data}"

mkdir -p "$OUT_DIR"

echo "Downloading $DATASET into $OUT_DIR ..."
huggingface-cli download \
    --repo-type dataset \
    --local-dir "$OUT_DIR" \
    "$DATASET"

echo "Done. Files:"
ls -la "$OUT_DIR"
