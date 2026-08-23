#!/usr/bin/env bash
# SHIFU metrics logger — records hook events to SQLite
# All event_type values are hardcoded by SHIFU hooks (not user input).
#
# Usage:
#   metrics.sh log <event_type> [gear] [value]  — log an event
#   metrics.sh summary [days]                    — print summary (default 7d)
#   metrics.sh reset                             — clear all events

set -euo pipefail

# Allow test override via env var
METRICS_DIR="${CLAUDE_PLUGIN_DATA:-${HOME}/.claude/plugins/data/shifu}/metrics"
METRICS_DB="${METRICS_DB_OVERRIDE:-${METRICS_DIR}/metrics.db}"

mkdir -p "$(dirname "$METRICS_DB")"

_init_db() {
  python3 - "$METRICS_DB" <<'PYEOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute("""
  CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY,
    event_type  TEXT NOT NULL,
    gear        TEXT,
    value       TEXT,
    created_at  TEXT DEFAULT (datetime('now','localtime'))
  )
""")
db.commit()
db.close()
PYEOF
}

_log_event() {
  local event_type="$1"
  local gear="${2:-}"
  local value="${3:-}"
  python3 - "$METRICS_DB" "$event_type" "$gear" "$value" <<'PYEOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute(
  "INSERT INTO events (event_type, gear, value) VALUES (?, ?, ?)",
  (sys.argv[2], sys.argv[3] or None, sys.argv[4] or None)
)
db.commit()
db.close()
PYEOF
}

_summary() {
  local days="${1:-7}"
  python3 - "$METRICS_DB" "$days" <<'PYEOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
days = int(sys.argv[2])
rows = db.execute(
  "SELECT event_type, COUNT(*) FROM events WHERE created_at >= datetime('now','localtime',?) GROUP BY event_type ORDER BY COUNT(*) DESC",
  (f"-{days} days",)
).fetchall()
if rows:
  print(f"{'Event':<30} {'Count':>5}  (last {days}d)")
  print("-" * 40)
  for ev, cnt in rows:
    print(f"{ev:<30} {cnt:>5}")
else:
  print(f"No events in the last {days} days.")
db.close()
PYEOF
}

_reset() {
  python3 - "$METRICS_DB" <<'PYEOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute("DELETE FROM events")
db.commit()
db.close()
PYEOF
  echo "Metrics cleared"
}

_init_db

case "${1:-summary}" in
  log)     _log_event "${2:?'event_type required'}" "${3:-}" "${4:-}" ;;
  summary) _summary "${2:-7}" ;;
  reset)   _reset ;;
  *)
    echo "Usage: metrics.sh [log|summary|reset]" >&2
    exit 1
    ;;
esac
