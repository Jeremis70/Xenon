# Error Handling

This page defines the currently documented state of Xenon error handling.

## Current guarantees

- No stable language-level error model is finalized yet.
- Result-style returns, exceptions, and panic semantics are all currently unspecified.
- Error propagation syntax is not finalized.

## Current related constructs

Draft control-flow material references `unreachable` and `halt`, but these do not currently define a full recoverable/non-recoverable error model.

## Guidance for users (pre-alpha)

- Do not assume compatibility with Rust/Go/C++ error conventions.
- Do not assume stack unwinding, exception handling, or panic recovery behavior.
- Treat all error-handling behavior as subject to change until this page defines stable rules.

## Planned scope for this page

Future revisions should specify:

- recoverable vs non-recoverable error categories,
- propagation and conversion rules,
- default diagnostics and exit-code conventions,
- interaction with low-level control-flow constructs.
