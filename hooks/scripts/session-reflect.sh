#!/usr/bin/env bash
# SHIFU Stop Hook — session-end self-reflection nudge
# Fires when Claude Code session ends. Injects a reminder for the AI
# to reflect on what happened, update campaign state, and log friction.
#
# This implements Hermes-style "periodic nudge" at session boundaries.

set -euo pipefail

# Refire guard: suppress repeated Stop events only when Claude provides a safe session ID.
_HOOK_INPUT=$(cat 2>/dev/null || true)
_SID=$(printf '%s' "$_HOOK_INPUT" | jq -r '.session_id // empty' 2>/dev/null || true)
if [[ "$_SID" =~ ^[A-Za-z0-9._-]+$ ]]; then
  _SENTINEL="/tmp/shifu-$(basename "$0")-${_SID}.fired"
  [ -f "$_SENTINEL" ] && exit 0
  touch "$_SENTINEL" 2>/dev/null || true
fi


SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLUGIN_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Read campaign to check if there's an active task
CAMPAIGN_SCRIPT="${PLUGIN_ROOT}/hooks/scripts/campaign.sh"
campaign_gear=""
campaign_task=""

if [ -x "$CAMPAIGN_SCRIPT" ]; then
  campaign_json=$(CLAUDE_PLUGIN_DATA="${CLAUDE_PLUGIN_DATA:-${HOME}/.claude/plugins/data/shifu}" "$CAMPAIGN_SCRIPT" read 2>/dev/null || echo '{}')
  campaign_gear=$(echo "$campaign_json" | python3 -c 'import json,sys; d=json.load(sys.stdin); v=d.get("gear") or ""; print(json.dumps(v)[1:-1])' 2>/dev/null || echo '')
  campaign_task=$(echo "$campaign_json" | python3 -c 'import json,sys; d=json.load(sys.stdin); v=d.get("task") or ""; print(json.dumps(v)[1:-1])' 2>/dev/null || echo '')
fi

# Build reflection prompt
reflect_msg="SHIFU session-end reflection: Before ending, briefly consider:"
reflect_msg="${reflect_msg}\\n1. What was accomplished this session?"
reflect_msg="${reflect_msg}\\n2. Was the gear (${campaign_gear:-none}) appropriate?"
reflect_msg="${reflect_msg}\\n3. Any friction worth logging? (unexpected failures, wrong assumptions, process overhead)"
reflect_msg="${reflect_msg}\\n4. Should campaign state be updated or reset?"

if [ -n "$campaign_task" ]; then
  reflect_msg="${reflect_msg}\\n\\nActive campaign task: ${campaign_task}"
  reflect_msg="${reflect_msg}\\nIf task is done, reset campaign. If continuing, update lastSessionSummary."
fi

jq -n --arg msg "$reflect_msg" \
  '{ hookSpecificOutput: { hookEventName: "Stop", additionalContext: $msg } }'

exit 0
