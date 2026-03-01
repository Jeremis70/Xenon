# Language Overview

This page summarizes Xenon goals, scope, and current status.

## Philosophy

Xenon aims to be a systems language that stays close to the machine while still allowing high-level composition.

The design intent is that core constructs are composable objects. This includes functions, loops, and conditionals being treated in a user-extensible way.

## Current known facts

- Source extension: `.xe`
- Compiler binary: `xenonc`
- Compiler implementation language: Rust
- Planned backend direction: LLVM IR via `inkwell`

## Uncertain and evolving areas

- Syntax and semantics for some trait and control-flow forms are not final.
- Several sections in this documentation are placeholders pending language decisions.

## Related pages

- [Syntax Basics](syntax-basics.md)
- [Types](types.md)
- [Functions](functions.md)
- [Design Rationale](../reference/design-rationale.md)
