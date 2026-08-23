#!/usr/bin/env bash
# Verify Codex adapter skill sources and installer.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/test-helpers.sh"

echo "=== Codex Adapter Tests ==="

for skill in shifu shifu-research shifu-review; do
  skill_file="${REPO_ROOT}/codex/skills/${skill}/SKILL.md"
  assert_file_exists "$skill_file" "codex/${skill}: SKILL.md exists"
  if [ ! -f "$skill_file" ]; then
    continue
  fi
  content="$(cat "$skill_file")"
  assert_contains "$content" "^---" "codex/${skill}: has frontmatter"
  assert_contains "$content" "^name: ${skill}$" "codex/${skill}: name matches directory"
  assert_contains "$content" "^description: Use when" "codex/${skill}: description is trigger-oriented"
  assert_contains "$content" "Codex" "codex/${skill}: names Codex context"
  assert_not_contains "$content" "PLACEHOLDER|TODO|TBD" "codex/${skill}: no placeholders"
  assert_line_count_under "$skill_file" 180 "codex/${skill}: under 180 lines"
done

main_skill="${REPO_ROOT}/codex/skills/shifu/SKILL.md"
if [ -f "$main_skill" ]; then
  main_content="$(cat "$main_skill")"
  assert_contains "$main_content" "G1" "codex/shifu: defines G1"
  assert_contains "$main_content" "G2" "codex/shifu: defines G2"
  assert_contains "$main_content" "G3" "codex/shifu: defines G3"
  assert_contains "$main_content" "test-driven-development" "codex/shifu: maps to Codex TDD skill"
  assert_contains "$main_content" "systematic-debugging" "codex/shifu: maps to Codex debugging skill"
  assert_contains "$main_content" "verification-before-completion" "codex/shifu: maps to Codex verification skill"
  assert_contains "$main_content" "Claude hooks do not run in Codex" "codex/shifu: documents hook limitation"
  assert_contains "$main_content" "explicitly requests SHIFU, G1, G2, or G3" "codex/shifu: trigger is explicit and narrow"
  assert_contains "$main_content" "never overrides a more specific" "codex/shifu: gear cannot bypass specific skills"
  assert_not_contains "$main_content" "1-2 files|3-5 files|6\+ files" "codex/shifu: gear is not based on file count"
  assert_not_contains "$main_content" "Keep technical progress quiet" "codex/shifu: does not redefine platform update cadence"
  assert_contains "$main_content" "reportclaw-coordinator" "codex/shifu: formal report handoff uses coordinator only"
  assert_contains "$main_content" "terminal handoff" "codex/shifu: ReportClaw handoff ends SHIFU ownership"
  assert_contains "$main_content" 'native `review`, `health`, or `verification-before-completion`' "codex/shifu: selects native review depth"
  assert_not_contains "$main_content" 'route internally to `shifu-review`' "codex/shifu: does not route duplicate review"
fi

research_skill="${REPO_ROOT}/codex/skills/shifu-research/SKILL.md"
review_skill="${REPO_ROOT}/codex/skills/shifu-review/SKILL.md"
if [ -f "$research_skill" ]; then
  assert_contains "$(cat "$research_skill")" 'Use only when `shifu` routes' "codex/shifu-research: internal route only"
fi
if [ -f "$review_skill" ]; then
  review_content="$(cat "$review_skill")"
  assert_contains "$review_content" "explicitly requests the legacy" "codex/shifu-review: compatibility trigger is explicit"
  assert_contains "$review_content" "compatibility" "codex/shifu-review: is a compatibility redirect"
  assert_not_contains "$review_content" "## Checklist" "codex/shifu-review: does not execute a second checklist"
fi

installer="${REPO_ROOT}/scripts/install-codex-adapter.sh"
assert_file_exists "$installer" "installer: exists"
if [ -f "$installer" ]; then
  assert_contains "$(cat "$installer")" "codex/skills" "installer: copies adapter source"
  assert_contains "$(cat "$installer")" ".codex/skills" "installer: targets Codex skill directory"
  assert_contains "$(cat "$installer")" "check_mode" "installer: supports drift checking"
  assert_not_contains "$(cat "$installer")" 'rm -rf "\$target"' "installer: does not delete targets directly"

  temp_codex_home="$(mktemp -d)"
  if CODEX_HOME="$temp_codex_home" bash "$installer" >/dev/null 2>&1 \
    && CODEX_HOME="$temp_codex_home" bash "$installer" >/dev/null 2>&1 \
    && CODEX_HOME="$temp_codex_home" bash "$installer" --check >/dev/null 2>&1; then
    echo -e "${GREEN}PASS${NC}: installer: repeat install and --check succeed"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: installer: repeat install and --check succeed"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  canary="${temp_codex_home}/skills/shifu/user-note.txt"
  mkdir -p "$(dirname "$canary")"
  printf 'keep\n' > "$canary"
  if CODEX_HOME="$temp_codex_home" bash "$installer" >/dev/null 2>&1; then
    echo -e "${RED}FAIL${NC}: installer: refuses unknown target files"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  elif [ "$(cat "$canary" 2>/dev/null)" = "keep" ]; then
    echo -e "${GREEN}PASS${NC}: installer: refuses unknown target files"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: installer: preserves unknown target files"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  rm -f "$canary"
  unknown_dir="${temp_codex_home}/skills/shifu/local-empty"
  mkdir -p "$unknown_dir"
  if CODEX_HOME="$temp_codex_home" bash "$installer" >/dev/null 2>&1; then
    echo -e "${RED}FAIL${NC}: installer: refuses unknown empty directories"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  elif [ -d "$unknown_dir" ]; then
    echo -e "${GREEN}PASS${NC}: installer: refuses unknown empty directories"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: installer: preserves unknown empty directories"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
  rmdir "$unknown_dir"

  before_hash="$(find "${temp_codex_home}/skills" -type f -exec shasum -a 256 {} \; | sort | shasum -a 256)"
  if CODEX_HOME="$temp_codex_home" ADAPTER_INSTALL_FAIL_ON_SKILL=shifu-research \
      bash "$installer" >/dev/null 2>&1; then
    echo -e "${RED}FAIL${NC}: installer: injected batch failure is reported"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    after_hash="$(find "${temp_codex_home}/skills" -type f -exec shasum -a 256 {} \; | sort | shasum -a 256)"
    if [ "$before_hash" = "$after_hash" ]; then
      echo -e "${GREEN}PASS${NC}: installer: whole batch rolls back on failure"
      PASS_COUNT=$((PASS_COUNT + 1))
    else
      echo -e "${RED}FAIL${NC}: installer: whole batch rolls back on failure"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
  fi

  invalid_source="$(mktemp -d)"
  cp -R "${REPO_ROOT}/codex/skills/." "$invalid_source/"
  sed 's/^name: shifu-review$/name: wrong-name/' \
    "$invalid_source/shifu-review/SKILL.md" > "$invalid_source/shifu-review/SKILL.md.tmp"
  mv "$invalid_source/shifu-review/SKILL.md.tmp" "$invalid_source/shifu-review/SKILL.md"
  if CODEX_HOME="$temp_codex_home" ADAPTER_SOURCE_DIR="$invalid_source" \
      bash "$installer" >/dev/null 2>&1; then
    echo -e "${RED}FAIL${NC}: installer: rejects invalid source frontmatter"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    echo -e "${GREEN}PASS${NC}: installer: rejects invalid source frontmatter"
    PASS_COUNT=$((PASS_COUNT + 1))
  fi
  rm -rf "$invalid_source"
  rm -rf "$temp_codex_home"
fi

print_summary
