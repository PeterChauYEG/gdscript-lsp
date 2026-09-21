#!/usr/bin/env bash
# file-size-scan.test.sh — smoke tests for file-size-scan.sh.
#
# Runs the real script (no tool to mock — it's just `find`/`wc -l`) against
# a throwaway fixture directory, so it never touches the real repo tree.
# Exercises: files under threshold (clean report), files over threshold
# (listed and sorted largest-first), non-.rs files ignored, and that the
# script always exits 0 (non-blocking).
#
# Usage: scripts/file-size-scan.test.sh
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$script_dir/file-size-scan.sh"

fail=0

assert_contains() {
  local haystack="$1" needle="$2" msg="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "FAIL: $msg"
    echo "  expected to contain: $needle"
    echo "  got: $haystack"
    fail=1
  else
    echo "PASS: $msg"
  fi
}

assert_not_contains() {
  local haystack="$1" needle="$2" msg="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "FAIL: $msg"
    echo "  expected NOT to contain: $needle"
    fail=1
  else
    echo "PASS: $msg"
  fi
}

fixture_dir="$(mktemp -d)"
report_md="$(mktemp)"
trap 'rm -rf "$fixture_dir" "$report_md"' EXIT

# ── Test 1: all files under threshold — clean report, exit 0 ───────────────
seq 1 5 > "$fixture_dir/small.rs"

set +e
output=$(bash "$script" "$report_md" 10 "$fixture_dir" 2>&1)
status=$?
set -e
[[ $status -eq 0 ]] && echo "PASS: exits 0 when nothing is oversized" || { echo "FAIL: expected exit 0, got $status"; fail=1; }
assert_contains "$(cat "$report_md")" "No files exceed 10 lines" "renders a clean report"

# ── Test 2: files over threshold — listed, sorted largest-first, non-.rs ignored ──
seq 1 5 > "$fixture_dir/small.rs"
seq 1 20 > "$fixture_dir/big.rs"
seq 1 30 > "$fixture_dir/bigger.rs"
seq 1 100 > "$fixture_dir/ignored.txt"

set +e
bash "$script" "$report_md" 10 "$fixture_dir" > /dev/null 2>&1
status=$?
set -e
[[ $status -eq 0 ]] && echo "PASS: still exits 0 when files ARE oversized (non-blocking)" || { echo "FAIL: expected exit 0, got $status"; fail=1; }
report_content="$(cat "$report_md")"
assert_contains "$report_content" "big.rs" "lists the oversized .rs file"
assert_contains "$report_content" "bigger.rs" "lists the other oversized .rs file"
assert_not_contains "$report_content" "small.rs" "omits the under-threshold file"
assert_not_contains "$report_content" "ignored.txt" "ignores non-.rs files"
bigger_pos=$(grep -n "bigger.rs" "$report_md" | head -1 | cut -d: -f1)
big_pos=$(grep -n "big.rs" "$report_md" | head -1 | cut -d: -f1)
[[ "$bigger_pos" -lt "$big_pos" ]] && echo "PASS: sorts largest file first" || { echo "FAIL: expected bigger.rs (30 lines) before big.rs (20 lines)"; fail=1; }

if [[ $fail -ne 0 ]]; then
  echo "One or more tests FAILED"
  exit 1
fi
echo "All file-size-scan.sh tests passed"
