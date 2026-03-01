# Build System

This page documents the current project build setup.

## Workspace

The repository is a Cargo workspace with one member crate:

- `compiler` (package name: `xenonc`)

## Common commands

```bash
cargo build -p xenonc
cargo run -p xenonc -- --help
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Watch mode

The VS Code task named `Watch Build` runs `cargo watch` for the compiler source tree.

```bash
cargo install cargo-watch --locked
```
