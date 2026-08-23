#!/usr/bin/env bash
# Verify skill files have correct structure and stay within limits
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

MAX_LINES=200

echo "=== Skill Loading Tests ==="

for skill_dir in "${REPO_ROOT}"/skills/*/; do
  skill_name=$(basename "$skill_dir")
  skill_file="${skill_dir}SKILL.md"

  # File exists
  assert_file_exists "$skill_file" "${skill_name}: SKILL.md exists"

  if [ ! -f "$skill_file" ]; then continue; fi

  content=$(cat "$skill_file")

  # Has frontmatter
  assert_contains "$content" "^---" "${skill_name}: has frontmatter"
  assert_contains "$content" "^name:" "${skill_name}: has name field"
  assert_contains "$content" "^description:" "${skill_name}: has description field"

  # Under line limit
  assert_line_count_under "$skill_file" "$MAX_LINES" "${skill_name}: under ${MAX_LINES} lines"

  # No placeholder content (exclude lines that are instructions about placeholders)
  filtered=$(echo "$content" | grep -vE '"TBD"|"TODO"|NO "TBD"|Placeholder scan' || true)
  assert_not_contains "$filtered" '\bTBD\b|\bTODO\b|\bFIXME\b|\bPLACEHOLDER\b' "${skill_name}: no placeholders"
done

# Engage must have gear definitions
engage=$(cat "${REPO_ROOT}/skills/engage/SKILL.md")
assert_contains "$engage" "G1.*Light" "engage: defines G1"
assert_contains "$engage" "G2.*Medium" "engage: defines G2"
assert_contains "$engage" "G3.*Heavy" "engage: defines G3"

print_summary
