#!/usr/bin/env bash
# file-size-scan.sh — flags .rs files over a line-count threshold.
#
# clippy::too_many_lines only gates function length, not file length, and
# no Rust-native tool fills that gap. This is the small custom script
# called for by LAB-898 / the "Duplicate-Code & File-Size CI Job" Linear
# doc.
#
# Deliberately non-blocking (unlike duplicate-code-scan.sh): the CI job
# posts this report as a sticky comment but never fails the build on it.
#
# Usage: scripts/file-size-scan.sh [report.md] [threshold] [scan-dir]
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

report_md="${1:-file-size-report.md}"
threshold="${2:-300}"
scan_dir="${3:-$repo_root}"

oversized=()
while IFS= read -r -d '' file; do
  lines=$(wc -l < "$file" | tr -d ' ')
  if (( lines > threshold )); then
    oversized+=("${lines}|${file}")
  fi
done < <(find "$scan_dir" -name '*.rs' \
  -not -path '*/target/*' -not -path '*/.git/*' -print0)

{
  echo "## File-Size Report (.rs files over ${threshold} lines)"
  echo
  if [[ ${#oversized[@]} -eq 0 ]]; then
    echo "No files exceed ${threshold} lines. ✅"
  else
    echo "**${#oversized[@]} file(s) exceed ${threshold} lines** — non-blocking, for awareness only:"
    echo
    printf '%s\n' "${oversized[@]}" | sort -t'|' -k1 -rn | while IFS='|' read -r lines file; do
      echo "* \`${file#"$repo_root"/}\` — ${lines} lines"
    done
  fi
} > "$report_md"

echo "[file-size-scan] wrote $report_md (${#oversized[@]} file(s) over ${threshold} lines)"
exit 0
