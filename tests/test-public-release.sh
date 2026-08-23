#!/usr/bin/env bash
# Public-release contract: portable files, complete hook references, and synced metadata.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

echo "=== Public Release Tests ==="

assert_file_exists "$REPO_ROOT/.claude-plugin/marketplace.json" "Claude marketplace manifest exists"
if [ -f "$REPO_ROOT/.claude-plugin/marketplace.json" ]; then
  marketplace_source=$(python3 -c 'import json; print(json.load(open(".claude-plugin/marketplace.json"))["plugins"][0]["source"])')
  assert_contains "$marketplace_source" '^\.$' "marketplace installs plugin from repository root"
fi

tracked_text=$(find "$REPO_ROOT" \
  -path "$REPO_ROOT/.git" -prune -o \
  -path "$REPO_ROOT/tests/test-public-release.sh" -prune -o \
  -type f -print0 \
  | xargs -0 grep -Il . 2>/dev/null || true)

for forbidden in \
  '/Users/eddie' \
  '/Desktop/project-wenge' \
  'MyInvestPilot' \
  'Wangyzhou' \
  'wenge.com'; do
  if [ -n "$tracked_text" ] && grep -nF "$forbidden" $tracked_text >/dev/null 2>&1; then
    echo -e "\033[0;31mFAIL\033[0m: public files contain forbidden private marker: $forbidden"
    ((FAIL_COUNT++))
  else
    echo -e "\033[0;32mPASS\033[0m: no forbidden private marker: $forbidden"
    ((PASS_COUNT++))
  fi
done

while IFS= read -r command_path; do
  relative_path=${command_path#\$\{CLAUDE_PLUGIN_ROOT\}/}
  assert_file_exists "$REPO_ROOT/$relative_path" "hook command exists: $relative_path"
done < <(python3 - "$REPO_ROOT/hooks/hooks.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
for groups in data.get("hooks", {}).values():
    for group in groups:
        for hook in group.get("hooks", []):
            command = hook.get("command", "")
            if command.startswith("${CLAUDE_PLUGIN_ROOT}/"):
                print(command)
PY
)

plugin_version=$(python3 -c 'import json; print(json.load(open(".claude-plugin/plugin.json"))["version"])')
assert_contains "$(cat README.md)" "version-${plugin_version}" "README badge matches plugin version"

skill_count=$(find "$REPO_ROOT/skills" -mindepth 2 -maxdepth 2 -name SKILL.md | wc -l | tr -d ' ')
assert_contains "$(cat README.md)" "Skills \\(${skill_count} total\\)" "README skill count matches public tree"

print_summary
