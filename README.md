# Xenon Programming Language (Xe)

[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](LICENSE)

Xenon is a hobby systems programming language I’m building for fun and learning.

- File extension: `.xe`
- Compiler: `xenonc` (implemented in Rust)
- Planned backend: LLVM IR (via `inkwell`)

The language design/spec lives in [docs/xenon.md](docs/xenon.md).

## Status

Pre-alpha. Right now `xenonc` is a stub (it just prints a message). Expect breaking changes.

## Repository layout

- [compiler/](compiler/) — Rust crate that builds the `xenonc` compiler binary
- [docs/](docs/) — language documentation/spec

## Build and run

Prerequisite: a recent Rust toolchain (stable).

- Build:
	- `cargo build -p xenonc`
- Run:
	- `cargo run -p xenonc`

### Useful dev commands

- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Docs: `cargo doc --workspace --no-deps`

If you use the VS Code tasks in [.vscode/tasks.json](.vscode/tasks.json), note that the “Watch Build” task requires `cargo-watch`:

- Install: `cargo install cargo-watch --locked`

## License

This project is licensed under the Mozilla Public License Version 2.0 (MPL-2.0).
See [LICENSE](LICENSE).