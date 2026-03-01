# ABI and Calling Conventions

This page describes the current ABI status for Xenon.

## Current guarantees

- Xenon does not provide a stable ABI yet.
- Calling conventions are not finalized.
- Symbol naming and mangling are not finalized.
- Binary compatibility across compiler versions is not guaranteed.

## Current compiler scope

The compiler currently exposes target and code model metadata through CLI options, but this should not be interpreted as a stable ABI contract.

## Guidance for users (pre-alpha)

- Do not depend on binary compatibility between Xenon compiler revisions.
- Avoid distributing precompiled Xenon binary artifacts as long-term stable interfaces.
- Treat interop boundaries as experimental until ABI policy is explicitly versioned.

## Planned scope for this page

Future revisions should define:

- platform ABI reuse vs Xenon-defined ABI boundaries,
- function call/return conventions,
- symbol naming and visibility rules,
- layout and alignment guarantees for exported types,
- ABI versioning and compatibility policy.
