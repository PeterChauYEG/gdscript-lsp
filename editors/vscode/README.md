# GDScript LSP

Standalone GDScript language server for Godot 4 — diagnostics, hover, completions,
go-to-definition, formatting, and more, without a running Godot instance.

## Features

- Activates automatically when a `.gd` file is opened.
- Downloads the correct `gdscript-lsp` server binary for your platform from the
  [latest GitHub Release](https://github.com/PeterChauYEG/gdscript-lsp/releases) on
  first use, and keeps it up to date on subsequent activations.
- Verifies the downloaded binary against the SHA256 checksum published alongside
  each release.

## Settings

| Setting | Description |
| --- | --- |
| `gdscript-lsp.serverPath` | Path to a manually installed `gdscript-lsp` binary. Leave empty to auto-download. |
| `gdscript-lsp.gdformatPath` | Path to the `gdformat` binary used for formatting. Leave empty to search `$PATH`. |
| `gdscript-lsp.trace.server` | Trace LSP communication between VS Code and the server. |

## Commands

- **Restart GDScript LSP Server** — stops and restarts the language server, re-checking
  for binary updates.

## Output

Server logs (including stderr from the `gdscript-lsp` binary) are written to the
**GDScript LSP** output channel.

## Supported platforms

`linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `win32-x64`.
