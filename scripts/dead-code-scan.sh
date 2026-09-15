#!/usr/bin/env bash
set -euo pipefail

BASE_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REPORT_PATH="$BASE_DIR/dead-code-report.md"

cd "$BASE_DIR"

MACHETE_OUTPUT=$(cargo machete 2>&1) && MACHETE_EXIT=0 || MACHETE_EXIT=$?

build_report() {
    local output="$1"
    local exit_code="$2"

    echo "## Dead-code scan (cargo-machete)"
    echo ""

    if [ "$exit_code" -eq 0 ]; then
        echo "✅ No unused dependencies found."
        return
    fi

    local total
    total=$(echo "$output" | grep -cE '^\s+[A-Za-z0-9_-]+$' || true)

    echo "❌ Found ${total} unused dependenc$([ "$total" -eq 1 ] && echo y || echo ies)."
    echo ""
    echo "<details><summary><strong>Unused dependencies</strong> — ${total}</summary>"
    echo ""
    echo '```'
    echo "$output"
    echo '```'
    echo ""
    echo "</details>"
}

build_report "$MACHETE_OUTPUT" "$MACHETE_EXIT" >"$REPORT_PATH"

cat "$REPORT_PATH"

if [ "$MACHETE_EXIT" -ne 0 ]; then
    echo "" >&2
    echo "Found unused dependencies — see $REPORT_PATH" >&2
    exit 1
fi

echo "No dead code found."
