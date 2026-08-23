#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

TMP_ROOT=$(mktemp -d)
SESSION_PREFIX="shifu-refire-test-$$"
DATE_FALLBACK="29991231-$$"
trap 'rm -rf "$TMP_ROOT"; rm -f "/tmp/shifu-session-reflect.sh-${SESSION_PREFIX}"*.fired "/tmp/shifu-session-reflect.sh-unknown-${DATE_FALLBACK}.fired"' EXIT

export HOME="${TMP_ROOT}/home"
export CLAUDE_PLUGIN_DATA="${TMP_ROOT}/plugin-data"
mkdir -p "$HOME" "$CLAUDE_PLUGIN_DATA" "${TMP_ROOT}/bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'case "${1:-}" in' \
  "  +%Y%m%d) echo \"${DATE_FALLBACK}\" ;;" \
  '  +%Y-%m-%d) echo "2999-12-31" ;;' \
  '  *) /bin/date "$@" ;;' \
  'esac' > "${TMP_ROOT}/bin/date"
chmod +x "${TMP_ROOT}/bin/date"
export PATH="${TMP_ROOT}/bin:${PATH}"

run_hook() {
  local hook="$1" input="$2"
  printf '%s' "$input" | bash "${REPO_ROOT}/hooks/scripts/${hook}" 2>/dev/null
}

assert_nonempty() {
  local content="$1" name="$2"
  if [ -n "$content" ]; then
    echo -e "${GREEN}PASS${NC}: $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: $name"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_empty() {
  local content="$1" name="$2"
  if [ -z "$content" ]; then
    echo -e "${GREEN}PASS${NC}: $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: $name"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

echo "=== Stop Hook Refire Tests ==="

for hook in session-reflect.sh; do
  sid_a="${SESSION_PREFIX}-${hook}-a"
  sid_b="${SESSION_PREFIX}-${hook}-b"

  first=$(run_hook "$hook" "{\"session_id\":\"${sid_a}\"}")
  second=$(run_hook "$hook" "{\"session_id\":\"${sid_a}\"}")
  different=$(run_hook "$hook" "{\"session_id\":\"${sid_b}\"}")
  missing_first=$(run_hook "$hook" '{}')
  missing_second=$(run_hook "$hook" '{}')
  empty_id=$(run_hook "$hook" '{"session_id":""}')
  malformed_id=$(run_hook "$hook" '{"session_id":"../../bad"}')

  assert_nonempty "$first" "$hook first call emits output"
  assert_empty "$second" "$hook same session is suppressed"
  assert_nonempty "$different" "$hook different session emits output"
  assert_nonempty "$missing_first" "$hook missing session id keeps normal behavior"
  assert_nonempty "$missing_second" "$hook missing session id does not share suppression"
  assert_nonempty "$empty_id" "$hook empty session id does not share suppression"
  assert_nonempty "$malformed_id" "$hook malformed session id does not share suppression"
done

print_summary
