#!/usr/bin/env bash
# Verify session-start hook produces valid output
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

echo "=== Hook Output Tests ==="

hook="${REPO_ROOT}/hooks/session-start"

# Hook file exists and is executable
assert_file_exists "$hook" "session-start hook exists"

if [ ! -x "$hook" ]; then
  echo -e "\033[0;31mFAIL\033[0m: session-start is not executable"
  ((FAIL_COUNT++))
else
  echo -e "\033[0;32mPASS\033[0m: session-start is executable"
  ((PASS_COUNT++))
fi

# Run hook with Claude Code env var and capture output
export CLAUDE_PLUGIN_ROOT="$REPO_ROOT"
output=$(bash "$hook" 2>/dev/null || echo "HOOK_FAILED")

# Output is valid JSON
if echo "$output" | python3 -m json.tool > /dev/null 2>&1; then
  echo -e "\033[0;32mPASS\033[0m: hook output is valid JSON"
  ((PASS_COUNT++))
else
  echo -e "\033[0;31mFAIL\033[0m: hook output is not valid JSON"
  ((FAIL_COUNT++))
fi

# Output contains engage skill content
assert_contains "$output" "SHIFU" "hook output contains SHIFU marker"
assert_contains "$output" "Gear System" "hook output contains Gear System"

# A real campaign must survive the context budget and appear in the injected hint.
TEMP_PLUGIN_DATA=$(mktemp -d)
trap 'rm -rf "$TEMP_PLUGIN_DATA"' EXIT
export CLAUDE_PLUGIN_DATA="$TEMP_PLUGIN_DATA"
bash "$REPO_ROOT/hooks/scripts/campaign.sh" write \
  '{"version":2,"gear":"G2","phase":"implementation","task":"portable release check","failureCount":0,"lastVerification":null,"lastSessionSummary":null,"updatedAt":null,"queue":[]}' \
  >/dev/null
campaign_output=$(bash "$hook" 2>/dev/null || echo "HOOK_FAILED")
assert_contains "$campaign_output" "portable release check" "hook preserves campaign hint within context budget"

print_summary
