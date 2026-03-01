# Hello World

This page shows the smallest documented Xenon entry-point example.

## Minimal program

```xe
entry fn main() {
    // TODO
}
```

## Compile or check

The `xenonc` CLI currently exposes `compile` and `check` subcommands.

```bash
cargo run -p xenonc -- check tests/main.xe
```

## Notes

- Current compiler behavior is pre-alpha and may print intermediate data.
- Standard output APIs (for example `print`) are not documented yet.
