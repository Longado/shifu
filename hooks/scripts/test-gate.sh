#!/usr/bin/env bash
# SHIFU PreToolUse Hook — test-gate
# Warns when writing source files that have no corresponding test file.
# After 3+ consecutive untested writes, asks for user confirmation.
#
# Checks: *.ts, *.js, *.py, *.go files (skips test files themselves,
# config files, type definitions, and non-source files).

set -euo pipefail

STATE_DIR="${CLAUDE_PLUGIN_DATA:-${HOME}/.claude/plugins/data/shifu}/state"
mkdir -p "$STATE_DIR"

INPUT=$(cat)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

# Only check Write and Edit on source files
if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# Skip non-source files
case "$FILE_PATH" in
  *.test.ts|*.test.js|*.spec.ts|*.spec.js|*_test.go|*_test.py|*.test.py) exit 0 ;;
  *.d.ts|*.config.ts|*.config.js) exit 0 ;;
  *.md|*.json|*.yaml|*.yml|*.toml|*.css|*.html|*.sh) exit 0 ;;
esac

# Check if it's a source file we care about
case "$FILE_PATH" in
  *.ts|*.js) TEST_EXT="${FILE_PATH##*.}" ;;
  *.py) TEST_EXT="py" ;;
  *.go) TEST_EXT="go" ;;
  *) exit 0 ;;
esac

# Determine expected test file path
DIR=$(dirname "$FILE_PATH")
BASE=$(basename "$FILE_PATH")
NAME="${BASE%.*}"

case "$TEST_EXT" in
  ts|js)
    TEST_FILE="$DIR/${NAME}.test.${TEST_EXT}"
    TEST_FILE_ALT="$DIR/${NAME}.spec.${TEST_EXT}"
    # Also check tests/ sibling directory
    PARENT=$(dirname "$DIR")
    LEAF=$(basename "$DIR")
    TEST_FILE_DIR="${PARENT}/tests/${LEAF}/${NAME}.test.${TEST_EXT}"
    ;;
  py)
    TEST_FILE="$DIR/test_${NAME}.py"
    TEST_FILE_ALT="$DIR/${NAME}_test.py"
    TEST_FILE_DIR="$DIR/tests/test_${NAME}.py"
    ;;
  go)
    TEST_FILE="$DIR/${NAME}_test.go"
    TEST_FILE_ALT=""
    TEST_FILE_DIR=""
    ;;
esac

# Check if any test file exists
if [ -f "$TEST_FILE" ] || [ -n "${TEST_FILE_ALT:-}" ] && [ -f "${TEST_FILE_ALT:-/nonexistent}" ] || [ -n "${TEST_FILE_DIR:-}" ] && [ -f "${TEST_FILE_DIR:-/nonexistent}" ]; then
  # Test exists — reset counter
  echo "0" > "$STATE_DIR/test-gate-failures.txt"
  exit 0
fi

# No test file found — increment counter
FAILURES=$(cat "$STATE_DIR/test-gate-failures.txt" 2>/dev/null || echo "0")
[[ "$FAILURES" =~ ^[0-9]+$ ]] || FAILURES=0
FAILURES=$((FAILURES + 1))
echo "$FAILURES" > "$STATE_DIR/test-gate-failures.txt"

SHORT_PATH=$(basename "$FILE_PATH")

if [ "$FAILURES" -ge 3 ]; then
  # Circuit breaker: log to metrics and escalate
  bash "$(dirname "$0")/metrics.sh" log "test_gate_triggered" "" "$SHORT_PATH" 2>/dev/null || true
  jq -n --arg msg "SHIFU test-gate: $FAILURES consecutive writes without test files. Consider writing tests first (shifu:test-first). File: $SHORT_PATH" \
    '{ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: $msg } }'
  exit 0
else
  # Warn but allow
  jq -n --arg msg "SHIFU test-gate: No test file for $SHORT_PATH ($FAILURES/3 before escalation)" \
    '{ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: $msg } }'
  exit 0
fi
