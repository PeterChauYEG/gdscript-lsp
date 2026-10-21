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

## Crates

[`crates/`](crates) is a Cargo workspace, with each crate owning one concern of
the server:

| Crate | Package | Purpose |
| --- | --- | --- |
| [`core`](crates/core) | `gdscript-core` | Core types and utilities shared across the workspace. |
| [`parser`](crates/parser) | `gdscript-parser` | GDScript parsing using `tree-sitter-gdscript`. |
| [`api-db`](crates/api-db) | `gdscript-api-db` | Godot engine type database, generated from `extension_api.json`. |
| [`indexer`](crates/indexer) | `gdscript-indexer` | Project-wide file indexer — class registry, scene parser, file watching. |
| [`checker`](crates/checker) | `gdscript-checker` | Type checker and linter for GDScript. |
| [`lsp`](crates/lsp) | `gdscript-lsp` | LSP server implementation, built on `tower-lsp`. |
| [`cli`](crates/cli) | `gdscript-lsp-bin` | The `gdscript-lsp` binary — standalone language server entry point. |

## Editor support

### VS Code

[`editors/vscode`](editors/vscode) contains the official VS Code extension. It
activates automatically on `.gd` files, downloads the matching `gdscript-lsp`
server binary for your platform from the
[latest GitHub Release](https://github.com/PeterChauYEG/gdscript-lsp/releases)
on first use, verifies it against its published SHA256 checksum, and keeps it
up to date. See the [extension README](editors/vscode/README.md) for settings,
commands, and supported platforms.

## Development

### Requirements

- Rust `stable` (see [`rust-toolchain.toml`](rust-toolchain.toml)), with the
  `rustfmt` and `clippy` components — `rustup` installs these automatically.
- Node.js (for the VS Code extension under `editors/vscode`).

### Building and testing the server

```sh
cargo build          # debug build of the whole workspace
cargo build --release
cargo run -p gdscript-lsp-bin -- --help   # run the gdscript-lsp CLI binary

cargo test            # run the test suite
cargo fmt --check      # formatting, as enforced in CI
cargo clippy -- -D warnings   # lints, as enforced in CI
```

These are the same checks run by [CI](.github/workflows/main.yml) on every
push to `main`.

### VS Code extension

From [`editors/vscode`](editors/vscode):

```sh
npm install
npm run compile    # bundle the extension with esbuild
npm run watch      # rebuild on change, for iterating in an Extension
                   # Development Host (F5 in VS Code)
npm run typecheck
```

See the [extension README](editors/vscode/README.md) for its settings,
commands, and supported platforms.

## License

MIT — see [LICENSE](LICENSE).
