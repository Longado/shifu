#!/usr/bin/env bash
# SHIFU campaign state — read/write/reset + task queue operations (v2)
# Used by session-start hook to inject previous campaign context
# and by other hooks to persist state across sessions.
#
# Usage:
#   campaign.sh read                    — print current campaign JSON (auto-migrates v1→v2)
#   campaign.sh write '<json>'          — overwrite campaign state
#   campaign.sh update key value        — update a single field
#   campaign.sh reset                   — clear campaign state
#   campaign.sh queue-add "title" gear  — add task to queue
#   campaign.sh queue-list              — list all queued tasks
#   campaign.sh queue-claim <id>        — claim task as active
#   campaign.sh queue-complete          — mark active task done, show next
#   campaign.sh queue-next              — show next queued task
#   campaign.sh queue-clear             — remove all queued tasks

set -euo pipefail

CAMPAIGN_DIR="${CLAUDE_PLUGIN_DATA:-${HOME}/.claude/plugins/data/shifu}/campaign"
CAMPAIGN_FILE="${CAMPAIGN_DIR}/campaign.json"

umask 077
mkdir -p "$CAMPAIGN_DIR"

init_campaign() {
  cat <<'ENDJSON'
{
  "version": 2,
  "gear": null,
  "phase": null,
  "task": null,
  "failureCount": 0,
  "lastVerification": null,
  "lastSessionSummary": null,
  "updatedAt": null,
  "queue": []
}
ENDJSON
}

migrate_v1_to_v2() {
  local data="$1"
  echo "$data" | jq '.version = 2 | if .queue == null then .queue = [] else . end'
}

read_campaign() {
  local data
  if [ -f "$CAMPAIGN_FILE" ]; then
    data=$(cat "$CAMPAIGN_FILE")
  else
    data=$(init_campaign)
  fi
  # Auto-migrate v1 → v2
  local version
  version=$(echo "$data" | jq '.version // 1')
  if [ "$version" -lt 2 ]; then
    data=$(migrate_v1_to_v2 "$data")
  fi
  echo "$data"
}

write_campaign() {
  local json="$1"
  local updated
  updated=$(echo "$json" | jq --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '.updatedAt = $ts')
  echo "$updated" > "$CAMPAIGN_FILE"
}

update_field() {
  local key="$1"
  local value="$2"
  local current
  current=$(read_campaign)
  local updated
  # Handle null, numbers, and strings
  if [ "$value" = "null" ] || echo "$value" | grep -qE '^[0-9]+$'; then
    updated=$(echo "$current" | jq --arg k "$key" --argjson v "$value" '.[$k] = $v')
  else
    updated=$(echo "$current" | jq --arg k "$key" --arg v "$value" '.[$k] = $v')
  fi
  write_campaign "$updated"
}

reset_campaign() {
  local fresh
  fresh=$(init_campaign)
  write_campaign "$fresh"
}

# --- Queue operations ---

queue_add() {
  local title="${1:?'task title required'}"
  local gear="${2:-G2}"
  local current
  current=$(read_campaign)
  local new_id
  new_id=$(echo "$current" | jq '.queue | if length > 0 then map(.id) | max + 1 else 1 end')
  local updated
  updated=$(echo "$current" | jq \
    --argjson id "$new_id" \
    --arg title "$title" \
    --arg gear "$gear" \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '.queue += [{"id": $id, "title": $title, "gear": $gear, "status": "queued", "addedAt": $ts}]')
  write_campaign "$updated"
  echo "Added task #${new_id}: ${title} (${gear})"
}

queue_list() {
  local current
  current=$(read_campaign)
  local count
  count=$(echo "$current" | jq '.queue | length')
  if [ "$count" -eq 0 ]; then
    echo "Queue is empty"
    return
  fi
  echo "$current" | jq -r '.queue[] | "#\(.id) [\(.gear)] \(.title) — \(.status)"'
}

queue_claim() {
  local id="${1:?'task id required'}"
  local current
  current=$(read_campaign)
  local task_title
  task_title=$(echo "$current" | jq -r --argjson id "$id" '.queue[] | select(.id == $id) | .title')
  local task_gear
  task_gear=$(echo "$current" | jq -r --argjson id "$id" '.queue[] | select(.id == $id) | .gear')
  if [ -z "$task_title" ] || [ "$task_title" = "null" ]; then
    echo "Error: task #$id not found in queue" >&2
    exit 1
  fi
  local updated
  updated=$(echo "$current" | jq \
    --argjson id "$id" \
    --arg title "$task_title" \
    --arg gear "$task_gear" \
    '(.queue[] | select(.id == $id)).status = "active"
     | .task = $title
     | .gear = $gear
     | .phase = "engage"')
  write_campaign "$updated"
  echo "Claimed task #${id}: ${task_title} (${task_gear})"
}

queue_complete() {
  local current
  current=$(read_campaign)
  local active_id
  active_id=$(echo "$current" | jq 'first(.queue[] | select(.status == "active") | .id) // empty')
  if [ -z "$active_id" ]; then
    # No queue item — just reset active fields
    local updated
    updated=$(echo "$current" | jq '.task = null | .gear = null | .phase = null | .failureCount = 0')
    write_campaign "$updated"
    echo "Completed (no queue item was active)"
    return
  fi
  local updated
  updated=$(echo "$current" | jq \
    --argjson id "$active_id" \
    'del(.queue[] | select(.id == $id))
     | .task = null | .gear = null | .phase = null | .failureCount = 0')
  write_campaign "$updated"
  echo "Completed task #${active_id}"
  # Show next queued task if any
  local next
  next=$(echo "$updated" | jq -r 'first(.queue[] | select(.status == "queued")) | "#\(.id) [\(.gear)] \(.title)"' 2>/dev/null || echo "")
  if [ -n "$next" ]; then
    echo "Next: ${next}"
  else
    echo "Queue empty — all done"
  fi
}

queue_next() {
  local current
  current=$(read_campaign)
  local next
  next=$(echo "$current" | jq -r 'first(.queue[] | select(.status == "queued")) | "#\(.id) [\(.gear)] \(.title)"' 2>/dev/null || echo "")
  if [ -n "$next" ]; then
    echo "Next: ${next}"
  else
    echo "Queue empty"
  fi
}

queue_clear() {
  local current
  current=$(read_campaign)
  local updated
  updated=$(echo "$current" | jq '.queue = []')
  write_campaign "$updated"
  echo "Queue cleared"
}

case "${1:-read}" in
  read)            read_campaign ;;
  write)           write_campaign "${2:?'JSON argument required'}" ;;
  update)          update_field "${2:?'key required'}" "${3:?'value required'}" ;;
  reset)           reset_campaign ;;
  queue-add)       queue_add "${2:-}" "${3:-G2}" ;;
  queue-list)      queue_list ;;
  queue-claim)     queue_claim "${2:?'task id required'}" ;;
  queue-complete)  queue_complete ;;
  queue-next)      queue_next ;;
  queue-clear)     queue_clear ;;
  *)
    echo "Usage: campaign.sh [read|write|update|reset|queue-add|queue-list|queue-claim|queue-complete|queue-next|queue-clear]" >&2
    exit 1
    ;;
esac
