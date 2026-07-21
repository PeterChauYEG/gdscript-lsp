#!/usr/bin/env bash
#
# Dead-code Scan Script
#
# Runs cargo-machete over the workspace to detect unused Cargo
# dependencies, and renders the findings as a PR-friendly markdown
# report. Mirrors the pattern established in mecha10
# (scripts/ci/dead-code-scan.sh), the first Rust repo to get
# dead-code detection.
#
# Note: this only covers unused *dependencies*. Unused private items
# are already caught by `cargo clippy -D warnings` (clippy job).
# Unused pub exports across workspace crates have no good Rust tool
# available and are intentionally out of scope here.
#
# Usage: ./scripts/dead-code-scan.sh
#
# Exits non-zero if cargo-machete reports any unused dependency.

set -euo pipefail

BASE_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REPORT_PATH="$BASE_DIR/dead-code-report.md"

cd "$BASE_DIR"

# cargo-machete exits non-zero when it finds unused deps, so don't let
# `set -e` abort us before we've captured the output.
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

    # cargo-machete prints one "cratename -- path/to/Cargo.toml:" header
    # per crate with unused deps, followed by indented dep names.
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
