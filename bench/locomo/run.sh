#!/usr/bin/env bash
# Thin shell wrapper around harness.py so the README's quick-start works
# without people remembering the python module path.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

if [ ! -d .venv ]; then
  python3 -m venv .venv
  ./.venv/bin/pip install -q -r requirements.txt
fi

exec ./.venv/bin/python harness.py "$@"
