#!/usr/bin/env bash
# Tests for SHIFU skill-guard hook (patch-first enforcement)

echo "=== Skill Guard Tests ==="

SCRIPT="$(cd "$(dirname "$0")/.." && pwd)/hooks/scripts/skill-guard.sh"
TEMP_DIR=$(mktemp -d)
export CLAUDE_PLUGIN_DATA="$TEMP_DIR"

cleanup() { rm -rf "$TEMP_DIR"; }
trap cleanup EXIT

pass=0; fail=0

assert_contains() {
  local desc="$1" expected="$2" actual="$3"
  if echo "$actual" | grep -q "$expected"; then
    printf '\033[0;32mPASS\033[0m: %s\n' "$desc"; ((pass++)) || true
  else
    printf '\033[0;31mFAIL\033[0m: %s — expected "%s" in output\n' "$desc" "$expected"; ((fail++)) || true
  fi
}

assert_empty() {
  local desc="$1" actual="$2"
  if [ -z "$actual" ]; then
    printf '\033[0;32mPASS\033[0m: %s\n' "$desc"; ((pass++)) || true
  else
    printf '\033[0;31mFAIL\033[0m: %s — expected no output, got: %s\n' "$desc" "$actual"; ((fail++)) || true
  fi
}

assert_valid_json() {
  local desc="$1" actual="$2"
  if echo "$actual" | python3 -m json.tool > /dev/null 2>&1; then
    printf '\033[0;32mPASS\033[0m: %s\n' "$desc"; ((pass++)) || true
  else
    printf '\033[0;31mFAIL\033[0m: %s — not valid JSON: %s\n' "$desc" "$actual"; ((fail++)) || true
  fi
}

# Test 1: Write on SKILL.md → emits warning
input='{"tool_name":"Write","tool_input":{"file_path":"/tmp/example/shifu/skills/engage/SKILL.md","content":"..."}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_valid_json    "Write on SKILL.md produces valid JSON"  "$out"
assert_contains      "Write on SKILL.md warns patch-first"    "patch-first"  "$out"
assert_contains      "Write on SKILL.md names the skill"      "engage"       "$out"

# Test 2: Write on non-SKILL.md → silent pass
input='{"tool_name":"Write","tool_input":{"file_path":"/tmp/example/project/src/foo.py","content":"..."}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_empty "Write on non-SKILL.md produces no output" "$out"

# Test 3: Edit on SKILL.md → silent pass (Edit is the correct pattern)
input='{"tool_name":"Edit","tool_input":{"file_path":"/tmp/example/shifu/skills/review/SKILL.md","old_string":"old","new_string":"new"}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_empty "Edit on SKILL.md produces no output (Edit is allowed)" "$out"

# Test 4: Write on /path/to/SKILL.md (not inside skills/ dir) → still warns
input='{"tool_name":"Write","tool_input":{"file_path":"/tmp/custom/SKILL.md","content":"..."}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_valid_json "Write on any file named SKILL.md warns" "$out"

# Test 4b: Write on my-SKILL.md (different filename) → silent
input='{"tool_name":"Write","tool_input":{"file_path":"/tmp/my-SKILL.md","content":"..."}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_empty "Write on my-SKILL.md (not named SKILL.md) is silent" "$out"

# Test 5: Missing file_path → silent pass
input='{"tool_name":"Write","tool_input":{}}'
out=$(echo "$input" | bash "$SCRIPT" 2>/dev/null)
assert_empty "Missing file_path produces no output" "$out"

echo ""
echo "================================"
printf "Results: \033[0;32m%s passed\033[0m, \033[0;31m%s failed\033[0m\n" "$pass" "$fail"
echo "================================"

[ "$fail" -eq 0 ]
