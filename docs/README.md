# Xenon Documentation

This documentation set describes the Xenon language and the `xenonc` compiler as currently documented.

Xenon is a systems programming language project with `.xe` source files and a Rust-based compiler. The language aims to combine low-level control with high-level composability.

## How to navigate

- Start here: [Getting Started](getting-started/installation.md)
- Language concepts: [Language Overview](language/overview.md)
- Compiler usage: [Compiler CLI](tooling/compiler-cli.md)
- Current and planned direction: [Design Rationale](reference/design-rationale.md) and [Roadmap](reference/roadmap.md)

For full navigation, see [SUMMARY.md](SUMMARY.md).

## Documentation conventions

- Syntax examples use `xe` code blocks.
- Items marked **Uncertain** are copied from draft notes and not finalized.
- Missing sections intentionally include TODO placeholders.

## Read in VS Code (mdBook)

This docs set is mdBook-compatible through the repository `book.toml` (`src = "docs"`).

```bash
# install once
cargo install mdbook

# run from repository root
mdbook serve
```

Open `http://localhost:3000` in VS Code Simple Browser (or any browser).

## Doc coverage

- [x] Getting started / installation / hello world
- [x] Language overview (goals and philosophy)
- [x] Syntax basics
- [x] Types and operators
- [x] Control flow
- [x] Functions (overloading, defaults, named returns)
- [x] Traits (`Iterable`, `Indexable`, `Callable`, `Rangeable`)
- [x] Ranges and slices
- [x] Low-level control flow (`goto`, `nop`, `asm`, `llvm`)
- [x] Compiler tooling (`xenonc` CLI)
- [x] Build system (Cargo workspace)
- [ ] Structs/enums details (no doc yet)
- [ ] Modules/imports details (no doc yet)
- [ ] Memory model / ownership / references (no doc yet)
- [ ] Standard library overview (no doc yet)
- [ ] Formatter (no doc yet)
- [ ] Linter (no doc yet)
- [ ] Package manager (no doc yet)
- [ ] FFI/interop design (no doc yet)
- [ ] Error handling model (no doc yet)
- [ ] Concurrency model (no doc yet)
- [ ] ABI / calling conventions (no doc yet)
- [ ] Changelog entries (no doc yet)
