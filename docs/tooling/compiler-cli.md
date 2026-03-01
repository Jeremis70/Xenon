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
- `check --stage` defaults to `borrowck`.

## Current implementation status

The compiler is pre-alpha. Current pipeline stages largely parse input and print internal/session data rather than producing finalized binaries.

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
