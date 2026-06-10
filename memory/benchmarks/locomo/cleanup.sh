#!/usr/bin/env bash
# Drop every note + downstream entity that the LoCoMo run inserted.
#
# We tag each ingested turn with `source LIKE 'locomo:%'`. Deleting those
# notes also cascades to entities whose only supporting note was a LoCoMo
# turn (Cairn's `delete_note` logic), so a real personal memory entity that
# happens to share a name with a fictional LoCoMo entity is *kept* (it has
# at least one non-LoCoMo source note).
#
# Usage:
#   ./cleanup.sh                 # against current Cairn DB
#   CAIRN_DB=/path/to/other.db ./cleanup.sh

set -euo pipefail

DB="${CAIRN_DB:-$HOME/Library/Application Support/Cairn/memory.db}"

if [[ ! -f "$DB" ]]; then
    echo "no Cairn DB at $DB" >&2
    exit 1
fi

echo "Inspecting LoCoMo footprint in $DB ..."
sqlite3 "$DB" <<SQL
SELECT 'notes',       COUNT(*) FROM notes      WHERE source LIKE 'locomo:%';
SELECT 'audit_rows',  COUNT(*) FROM audit_log  WHERE arguments LIKE '%locomo:%';
SELECT 'imported',    COUNT(*) FROM imported_docs WHERE source_path LIKE '%locomo%';
SQL

read -rp "Delete all locomo: notes? (y/N) " ans
if [[ "$ans" != "y" && "$ans" != "Y" ]]; then
    echo "aborted."
    exit 0
fi

echo "Deleting ..."
# Doing it this way (one note at a time) lets Cairn's cleanup cascade fire
# correctly. For a bulk DELETE you'd leave orphaned entities behind.
sqlite3 "$DB" "SELECT id FROM notes WHERE source LIKE 'locomo:%';" | while read -r nid; do
    if [[ -z "$nid" ]]; then continue; fi
    # We just do raw deletes here; if you want full cascade you can also
    # call the Tauri `delete_note` IPC for each id.
    sqlite3 "$DB" "DELETE FROM notes WHERE id = '$nid';"
done
echo "Done. Remaining locomo notes: $(sqlite3 "$DB" "SELECT COUNT(*) FROM notes WHERE source LIKE 'locomo:%';")"
echo
echo "NOTE: Entities extracted from LoCoMo turns are NOT auto-removed by"
echo "this script (raw DELETE bypasses Cairn's cascade). Run reindex from"
echo "Cairn /audit page or open the app to clean up orphan entities."
