#!/usr/bin/env bash
# SHIFU test helpers — shared assertion functions

PASS_COUNT=0
FAIL_COUNT=0
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

assert_contains() {
  local content="$1" pattern="$2" name="$3"
  if echo "$content" | grep -qE "$pattern" 2>/dev/null; then
    echo -e "${GREEN}PASS${NC}: $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: $name (expected pattern: $pattern)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_not_contains() {
  local content="$1" pattern="$2" name="$3"
  if echo "$content" | grep -qE "$pattern" 2>/dev/null; then
    echo -e "${RED}FAIL${NC}: $name (unexpected pattern found: $pattern)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    echo -e "${GREEN}PASS${NC}: $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  fi
}

assert_file_exists() {
  local file="$1" name="$2"
  if [ -f "$file" ]; then
    echo -e "${GREEN}PASS${NC}: $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: $name (file not found: $file)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_line_count_under() {
  local file="$1" max="$2" name="$3"
  local count
  count=$(wc -l < "$file" | tr -d ' ')
  if [ "$count" -le "$max" ]; then
    echo -e "${GREEN}PASS${NC}: $name ($count lines)"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo -e "${RED}FAIL${NC}: $name ($count lines exceeds $max)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

print_summary() {
  echo ""
  echo "================================"
  echo -e "Results: ${GREEN}${PASS_COUNT} passed${NC}, ${RED}${FAIL_COUNT} failed${NC}"
  echo "================================"
  return "$FAIL_COUNT"
}
