# gdscript-lsp

[![CI](https://github.com/PeterChauYEG/gdscript-lsp/actions/workflows/main.yml/badge.svg)](https://github.com/PeterChauYEG/gdscript-lsp/actions/workflows/main.yml)
[![License: MIT](https://img.shields.io/github/license/PeterChauYEG/gdscript-lsp)](LICENSE)
[![Last Commit](https://img.shields.io/github/last-commit/PeterChauYEG/gdscript-lsp)](https://github.com/PeterChauYEG/gdscript-lsp/commits/main)
[![Top Language](https://img.shields.io/github/languages/top/PeterChauYEG/gdscript-lsp)](https://github.com/PeterChauYEG/gdscript-lsp)

A standalone GDScript language server for Godot 4, written in Rust — diagnostics,
hover, completions, go-to-definition, and formatting, without a running Godot
instance. The workspace is split into focused crates (`core`, `parser`, `api-db`,
`indexer`, `checker`, `lsp`, `cli`) built around a `tree-sitter` GDScript parser,
and ships a prebuilt `gdscript-lsp` binary for Linux and macOS.

## Editor support

### VS Code

[`editors/vscode`](editors/vscode) contains the official VS Code extension. It
activates automatically on `.gd` files, downloads the matching `gdscript-lsp`
server binary for your platform from the
[latest GitHub Release](https://github.com/PeterChauYEG/gdscript-lsp/releases)
on first use, verifies it against its published SHA256 checksum, and keeps it
up to date. See the [extension README](editors/vscode/README.md) for settings,
commands, and supported platforms.

## License

MIT — see [LICENSE](LICENSE).
