# Installation

This page explains how to build and run the current Xenon compiler.

## Prerequisites

- A recent stable Rust toolchain
- Cargo

## Build

```bash
cargo build -p xenonc
```

## Run

```bash
cargo run -p xenonc -- --help
```

## Optional development tools

The workspace includes a watch task that uses `cargo-watch`.

```bash
cargo install cargo-watch --locked
```

Next: [Hello World](hello-world.md)
