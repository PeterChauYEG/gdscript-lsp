#!/usr/bin/env bash
# duplicate-code-scan.sh — runs cargo-dupes (AST-normalizing, Rust-native
# duplicate/near-duplicate detector; config in ../dupes.toml) against the
# repo and renders its stats + report into a human-readable
# duplicate-code-report.md for the CI sticky-comment step.
#
# jscpd has no meaningful Rust tokenizer, so cargo-dupes — which parses
# real ASTs via syn — is the right call here. See LAB-898 / the
# "Duplicate-Code & File-Size CI Job" Linear doc for why.
#
# Exceptions/thresholds live in dupes.toml, not here or in the CI job, per
# the "exceptions live in the tool's own config, not the CI job" convention
# in that doc.
#
# Usage: scripts/duplicate-code-scan.sh [report.md]
#
# cargo-dupes's own `check` exit code (0 = clean, 1 = over threshold, 2 =
# error) is propagated after the report is rendered, so the sticky-comment
# step always gets a report whether the scan passed or failed — the CI job
# decides pass/fail from this script's exit code, not from parsing the
# report itself.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

report_md="${1:-duplicate-code-report.md}"

cd "$repo_root"

stats_json=$(cargo dupes stats --format json)
text_report=$(cargo dupes report 2>&1 || true)

groups=$(echo "$stats_json" | jq -r '.exact_duplicate_groups')
dup_lines=$(echo "$stats_json" | jq -r '.exact_duplicate_lines')
total_lines=$(echo "$stats_json" | jq -r '.total_lines')
percentage=$(echo "$stats_json" | jq -r '.exact_duplicate_percent')

{
  echo "## Duplicate Code Report (cargo-dupes)"
  echo
  if [[ "$groups" -eq 0 ]]; then
    echo "No duplicate code blocks found. ✅"
  else
    echo "**${groups} duplicate group(s) found — ${dup_lines}/${total_lines} lines duplicated (${percentage}%).**"
    echo
    echo "<details><summary>Clone details</summary>"
    echo
    echo '```'
    echo "$text_report"
    echo '```'
    echo
    echo "</details>"
  fi
} > "$report_md"

echo "[duplicate-code-scan] wrote $report_md (${groups} group(s), ${percentage}% duplicated)"

check_status=0
cargo dupes check || check_status=$?

exit "$check_status"
