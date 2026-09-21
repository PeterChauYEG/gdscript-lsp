#!/usr/bin/env bash
# duplicate-code-scan.test.sh — smoke tests for duplicate-code-scan.sh.
#
# Mocks `cargo` on $PATH (same technique file-linear-ticket.test.sh uses for
# `curl`) intercepting the `dupes` subcommand, so no real scan/cargo install
# happens. Exercises the two outcomes: clean report (exit 0) and
# over-threshold report (exit 1), plus that cargo-dupes's exit code
# propagates to the caller either way.
#
# Usage: scripts/duplicate-code-scan.test.sh
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$script_dir/duplicate-code-scan.sh"

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

mock_bin="$(mktemp -d)"
report_md="$(mktemp)"
trap 'rm -rf "$mock_bin" "$report_md"' EXIT

# Fake `cargo dupes {stats,report,check}` driven by env vars, so each test
# case just points the mock at its own fixture output/exit code.
cat > "$mock_bin/cargo" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" != "dupes" ]]; then
  echo "unexpected invocation: cargo $*" >&2
  exit 2
fi
case "${2:-}" in
  stats) cat "$DUPES_STATS_JSON" ;;
  report) cat "$DUPES_REPORT_TXT" ;;
  check) exit "${DUPES_CHECK_EXIT:-0}" ;;
  *)
    echo "unexpected cargo dupes subcommand: $2" >&2
    exit 2
    ;;
esac
EOF
chmod +x "$mock_bin/cargo"

# ── Test 1: clean scan (no duplicate groups) — exits 0, report says so ─────
stats_clean="$mock_bin/stats-clean.json"
cat > "$stats_clean" <<'JSON'
{"total_code_units":4,"total_lines":100,"exact_duplicate_groups":0,"exact_duplicate_units":0,"near_duplicate_groups":0,"near_duplicate_units":0,"exact_duplicate_lines":0,"near_duplicate_lines":0,"exact_duplicate_percent":0.0,"near_duplicate_percent":0.0}
JSON
report_clean="$mock_bin/report-clean.txt"
echo "no duplicates found" > "$report_clean"

output=$(PATH="$mock_bin:$PATH" DUPES_STATS_JSON="$stats_clean" DUPES_REPORT_TXT="$report_clean" DUPES_CHECK_EXIT=0 \
  bash "$script" "$report_md" 2>&1)
status=$?
[[ $status -eq 0 ]] && echo "PASS: exits 0 on a clean scan" || { echo "FAIL: expected exit 0, got $status"; fail=1; }
assert_contains "$output" "0 group(s)" "logs zero duplicate groups on a clean scan"
assert_contains "$(cat "$report_md")" "No duplicate code blocks found" "renders a clean report"

# ── Test 2: over-threshold scan — exits 1, report lists the clone details ──
stats_dirty="$mock_bin/stats-dirty.json"
cat > "$stats_dirty" <<'JSON'
{"total_code_units":4,"total_lines":100,"exact_duplicate_groups":1,"exact_duplicate_units":2,"near_duplicate_groups":0,"near_duplicate_units":0,"exact_duplicate_lines":18,"near_duplicate_lines":0,"exact_duplicate_percent":18.0,"near_duplicate_percent":0.0}
JSON
report_dirty="$mock_bin/report-dirty.txt"
cat > "$report_dirty" <<'EOF'
Group 1: crates/lsp/src/hover.rs:10-28 <-> crates/lsp/src/completion.rs:40-58
EOF

set +e
output=$(PATH="$mock_bin:$PATH" DUPES_STATS_JSON="$stats_dirty" DUPES_REPORT_TXT="$report_dirty" DUPES_CHECK_EXIT=1 \
  bash "$script" "$report_md" 2>&1)
status=$?
set -e
[[ $status -eq 1 ]] && echo "PASS: propagates cargo-dupes's non-zero exit code" || { echo "FAIL: expected exit 1, got $status"; fail=1; }
report_content="$(cat "$report_md")"
assert_contains "$report_content" "1 duplicate group(s) found" "renders the duplicate group count"
assert_contains "$report_content" "crates/lsp/src/hover.rs" "renders the clone detail"

# ── Test 3: cargo-dupes error (exit 2) also propagates ─────────────────────
set +e
output=$(PATH="$mock_bin:$PATH" DUPES_STATS_JSON="$stats_clean" DUPES_REPORT_TXT="$report_clean" DUPES_CHECK_EXIT=2 \
  bash "$script" "$report_md" 2>&1)
status=$?
set -e
[[ $status -eq 2 ]] && echo "PASS: propagates cargo-dupes's error exit code" || { echo "FAIL: expected exit 2, got $status"; fail=1; }

if [[ $fail -ne 0 ]]; then
  echo "One or more tests FAILED"
  exit 1
fi
echo "All duplicate-code-scan.sh tests passed"
