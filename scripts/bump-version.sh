#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
cargo_toml="$repo_root/Cargo.toml"

VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
  echo "Error: Version argument required" >&2
  echo "Usage: $0 <version>" >&2
  exit 1
fi

if [[ ! -f "$cargo_toml" ]]; then
  echo "Error: $cargo_toml not found" >&2
  exit 1
fi

echo "Bumping version to: $VERSION"

awk -v new_version="$VERSION" '
  /^\[package\]/ { in_package=1 }
  /^\[/ && !/^\[package\]/ { in_package=0 }
  in_package && /^version = / {
    sub(/version = ".*"/, "version = \"" new_version "\"")
  }
  { print }
' "$cargo_toml" > "$cargo_toml.tmp"
mv "$cargo_toml.tmp" "$cargo_toml"

echo "✓ Updated Cargo.toml"
echo "Version bump complete: $VERSION"
