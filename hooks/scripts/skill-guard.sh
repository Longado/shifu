#!/usr/bin/env bash
# SHIFU PreToolUse Hook — skill-guard (patch-first enforcement)
# Warns when Claude uses Write (full rewrite) on any SKILL.md file.
# Nudges toward Edit (targeted patch) which preserves diff context.
#
# Principle (Hermes-inspired): patch > rewrite for skill evolution.
# Small targeted Edit calls are reviewable; Write replaces the whole file.
#
# Does NOT block — advisory only. Claude can proceed after reading the warning.

set -euo pipefail

INPUT=$(cat)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

# Only intercept Write on SKILL.md files
[ "$TOOL_NAME" = "Write" ] || exit 0
[ -n "$FILE_PATH" ] || exit 0

case "$FILE_PATH" in
  */SKILL.md|SKILL.md) ;;  # match — fall through to warning
  *) exit 0 ;;             # not a skill file — silent pass
esac

SKILL_NAME=$(basename "$(dirname "$FILE_PATH")")

# Log to metrics
bash "$(dirname "$0")/metrics.sh" log "skill_rewrite_warned" "" "$SKILL_NAME" 2>/dev/null || true

# Emit patch-first advisory
jq -n --arg skill "$SKILL_NAME" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    additionalContext: ("SHIFU skill-guard [patch-first]: You are about to full-rewrite skills/" + $skill + "/SKILL.md.\n\nPrefer Edit over Write for skill modifications:\n  ✓ Edit = targeted patch — reviewable diff, minimal surface area\n  ✗ Write = full rewrite — loses diff context, hard to review\n\nIf Write is truly necessary: add a one-line comment above explaining what changed and why.")
  }
}'

exit 0
