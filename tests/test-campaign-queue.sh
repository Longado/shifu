#!/usr/bin/env bash
# Tests for SHIFU campaign queue operations (v2.0.0)

echo "=== Campaign Queue Tests ==="

SCRIPT="$(cd "$(dirname "$0")/.." && pwd)/hooks/scripts/campaign.sh"
TEMP_DIR=$(mktemp -d)
export CLAUDE_PLUGIN_DATA="$TEMP_DIR"

cleanup() { rm -rf "$TEMP_DIR"; }
trap cleanup EXIT

pass=0; fail=0

assert_contains() {
  local desc="$1" expected="$2" actual="$3"
  if echo "$actual" | grep -q "$expected"; then
    printf '\033[0;32mPASS\033[0m: %s\n' "$desc"; ((pass++)) || true
  else
    printf '\033[0;31mFAIL\033[0m: %s — expected "%s" in: %s\n' "$desc" "$expected" "$actual"; ((fail++)) || true
  fi
}

assert_eq() {
  local desc="$1" expected="$2" actual="$3"
  if [ "$actual" = "$expected" ]; then
    printf '\033[0;32mPASS\033[0m: %s\n' "$desc"; ((pass++)) || true
  else
    printf '\033[0;31mFAIL\033[0m: %s — expected "%s", got "%s"\n' "$desc" "$expected" "$actual"; ((fail++)) || true
  fi
}

# --- Setup ---
mkdir -p "$TEMP_DIR/campaign"
bash "$SCRIPT" reset

# --- queue-add ---
out=$(bash "$SCRIPT" queue-add "Fix npm link" "G1")
assert_contains "queue-add creates task #1" "Added task #1" "$out"

out=$(bash "$SCRIPT" queue-add "Add auth middleware" "G2")
assert_contains "queue-add creates task #2" "Added task #2" "$out"

# --- queue-list ---
out=$(bash "$SCRIPT" queue-list)
assert_contains "queue-list shows first task" "Fix npm link" "$out"
assert_contains "queue-list shows second task" "Add auth middleware" "$out"

# --- queue-next ---
out=$(bash "$SCRIPT" queue-next)
assert_contains "queue-next shows task id" "#1" "$out"
assert_contains "queue-next shows gear" "G1" "$out"

# --- queue-claim ---
out=$(bash "$SCRIPT" queue-claim 1)
assert_contains "queue-claim reports success" "Claimed task #1" "$out"

active=$(bash "$SCRIPT" read | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("task",""))')
assert_eq "queue-claim sets .task field" "Fix npm link" "$active"

gear=$(bash "$SCRIPT" read | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("gear",""))')
assert_eq "queue-claim sets .gear field" "G1" "$gear"

# --- queue-complete ---
out=$(bash "$SCRIPT" queue-complete)
assert_contains "queue-complete reports completion" "Completed" "$out"

remaining=$(bash "$SCRIPT" read | python3 -c \
  'import json,sys; d=json.load(sys.stdin); q=[t for t in d.get("queue",[]) if t.get("status")=="queued"]; print(len(q))')
assert_eq "queue-complete leaves remaining tasks in queue" "1" "$remaining"

# --- queue-clear ---
bash "$SCRIPT" queue-add "Extra task" "G1" > /dev/null
bash "$SCRIPT" queue-clear > /dev/null
count=$(bash "$SCRIPT" read | python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("queue",[])))')
assert_eq "queue-clear empties queue" "0" "$count"

# --- v2 schema: queue field present after reset ---
bash "$SCRIPT" reset
schema=$(bash "$SCRIPT" read | python3 -c \
  'import json,sys; d=json.load(sys.stdin); print(d.get("version"), type(d.get("queue")).__name__)')
assert_eq "reset produces v2 schema with queue list" "2 list" "$schema"

# --- v1 → v2 auto-migration ---
cat > "$TEMP_DIR/campaign/campaign.json" <<'EOF'
{"version":1,"gear":"G2","phase":"test-first","task":"old task","failureCount":0,"lastVerification":null,"lastSessionSummary":null,"updatedAt":null}
EOF
migrated=$(bash "$SCRIPT" read | python3 -c \
  'import json,sys; d=json.load(sys.stdin); print(d.get("version"), type(d.get("queue")).__name__)')
assert_eq "v1 campaign auto-migrates to v2 on read" "2 list" "$migrated"

echo ""
echo "================================"
printf "Results: \033[0;32m%s passed\033[0m, \033[0;31m%s failed\033[0m\n" "$pass" "$fail"
echo "================================"

[ "$fail" -eq 0 ]
