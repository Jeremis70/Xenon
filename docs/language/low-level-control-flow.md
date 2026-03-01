# Low-Level Control Flow

This page lists low-level control primitives currently documented.

## Primitives

```xe
goto label;
label:
nop;
unreachable;
halt;
asm { ... }
llvm { ... }
```

## Notes

- `halt` is a non-recoverable exit
- `llvm { ... }` is described as potentially more portable than raw `asm`.

## Uncertain

- Semantics, constraints, and safety model for `asm`/`llvm` blocks are not documented yet.
