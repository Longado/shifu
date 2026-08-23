#!/usr/bin/env bash
# SHIFU test runner — runs all test-*.sh files
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TOTAL_PASS=0
TOTAL_FAIL=0

for test_file in "${SCRIPT_DIR}"/test-*.sh; do
  echo ""
  echo "Running $(basename "$test_file")..."
  echo "---"
  if bash "$test_file"; then
    :
  else
    exit_code=$?
    TOTAL_FAIL=$((TOTAL_FAIL + exit_code))
  fi
done

echo ""
echo "================================"
echo "All test suites complete."
if [ "$TOTAL_FAIL" -gt 0 ]; then
  echo "Some tests failed."
  exit 1
else
  echo "All tests passed."
  exit 0
fi
