#!/usr/bin/env bash
# SHIFU PostToolUse Hook — file-size-guard
# Warns when a file exceeds the recommended line limit after editing.
# Default limit: 800 lines (configurable via SHIFU_MAX_LINES env var).

set -euo pipefail

MAX_LINES="${SHIFU_MAX_LINES:-800}"

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [ -z "$FILE_PATH" ] || [ ! -f "$FILE_PATH" ]; then
  exit 0
fi

# Skip non-code files
case "$FILE_PATH" in
  *.md|*.json|*.yaml|*.yml|*.toml|*.lock|*.log) exit 0 ;;
  *.png|*.jpg|*.gif|*.svg|*.ico|*.woff|*.woff2) exit 0 ;;
esac

LINE_COUNT=$(wc -l < "$FILE_PATH" | tr -d ' ')

if [ "$LINE_COUNT" -gt "$MAX_LINES" ]; then
  SHORT_PATH=$(basename "$FILE_PATH")
  bash "$(dirname "$0")/metrics.sh" log "file_size_warn" "" "$SHORT_PATH:$LINE_COUNT" 2>/dev/null || true
  jq -n --arg msg "SHIFU file-size-guard: $SHORT_PATH has $LINE_COUNT lines (limit: $MAX_LINES). Consider extracting into smaller modules." \
    '{ hookSpecificOutput: { hookEventName: "PostToolUse", additionalContext: $msg } }'
fi

exit 0
