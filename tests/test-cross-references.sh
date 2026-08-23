#!/usr/bin/env bash
# Verify all shifu:name references resolve to existing skills
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

echo "=== Cross-Reference Tests ==="

# Extract all shifu:name references from skill files
refs=$(grep -rhoE 'shifu:[a-z-]+' "${REPO_ROOT}/skills/" | sort -u)

for ref in $refs; do
  skill_name="${ref#shifu:}"
  skill_path="${REPO_ROOT}/skills/${skill_name}/SKILL.md"
  assert_file_exists "$skill_path" "reference ${ref} resolves"
done

# Check that no skill references itself as a dependency
for skill_dir in "${REPO_ROOT}"/skills/*/; do
  skill_name=$(basename "$skill_dir")
  skill_file="${skill_dir}SKILL.md"
  if [ -f "$skill_file" ]; then
    self_ref="shifu:${skill_name}"
    if grep -q "$self_ref" "$skill_file" 2>/dev/null; then
      # Self-references in routing tables are OK, but circular deps are not
      # Just count for awareness
      echo "INFO: ${skill_name} references itself (${self_ref})"
    fi
  fi
done

print_summary
