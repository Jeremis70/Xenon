# Compiler CLI

This page documents the currently implemented `xenonc` command-line interface.

## Command shape

`xenonc` exposes two subcommands:

- `compile`
- `check`

Both require one or more source files.

```bash
xenonc compile path/to/file.xe
xenonc check path/to/file.xe
```

## Shared options (`compile` and `check`)

- Input/session: `--crate-name`, `--crate-type`, `--edition`, `--target`, `--sysroot`, `--cfg`, `--feature`, `-I/--include`, `-L`, `--extern`, `--print`
- Pipeline: `--stage`
- Output: `--emit`, `--dep-info`
- Diagnostics: `--error-format`, `--color`, `--warnings-as-errors`, `-v/--verbose`, `-q/--quiet`
- Internal/unstable: `-Z`

## `compile`-only options

- Artifact output: `-o/--output`, `--out-dir`
- Codegen: `-O/--opt-level`, `-g/--debuginfo`, `--incremental`, `--lto`, `--code-model`, `--relocation-model`, `--jobs`, `-C`
- Link: `--linker`, `--link-arg`, `--prefer-dynamic`, `--prefer-static`

## `check`-only differences

- `check` does not expose codegen/link options.
- `check --emit` supports: `ast`, `hir`, `mir`, `metadata`, `dep-info`, `tokens`.
- `check --stage` defaults to `mir`; other check stages are not implemented.

## Current implementation status

`check` type-checks input, lowers it to MIR, runs MIR flow checks, and
normalizes runtime checks without invoking LLVM. `check --emit=mir` prints
the resulting Runtime MIR. `compile` runs that same pipeline, then uses the
MIR-only LLVM backend to emit native artifacts.

The `llvm-backend` Cargo feature is enabled by default. Builds made with
`--no-default-features` retain parsing, type checking, MIR analysis, and
`check --emit=mir`, but native artifact requests fail with an explicit
feature-required diagnostic.

`compile` currently supports `--emit=link` (the default), `--emit=obj`,
`--emit=ir`, and `--emit=mir`. A normal linked build also writes `out.o` and
`out.ll` in the output directory. `compile --emit=mir` writes `out.mir`
without initializing the backend. The `check` command supports MIR output
and metadata's default no-op selection; other advertised check emit kinds
are not implemented yet. Native code generation currently targets the host;
explicit `--target` selection is rejected. `compile --stage` supports only
`mir` (with `--emit=mir`) and `link`; unsupported stage/emit combinations fail
with a diagnostic rather than silently continuing.

`-o` selects the linked executable when linking. With exactly one non-link
artifact (`mir`, `ir`, or `obj`), it selects that artifact's file path; use
`--out-dir` for multiple outputs.

## Print metadata

The `--print` option can return:

- `target-list`
- `host-target`
- `sysroot`
- `code-models`
- `relocation-models`
- `codegen-options`

## Related pages

- [Build System](build-system.md)
- [Getting Started](../getting-started/installation.md)
