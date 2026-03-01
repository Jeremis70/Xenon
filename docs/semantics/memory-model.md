# Memory Model

This page describes the current documented state of Xenon memory semantics.

## Current guarantees

- No stable ownership or borrowing model is finalized yet.
- No lifetime system is specified yet.
- No aliasing rules are specified yet.
- No stable object/value representation guarantees are documented yet.

## Current compiler behavior

The compiler is currently pre-alpha and focused on early frontend/pipeline behavior. Existing commands primarily parse inputs and expose intermediate/compiler-session data.

Because of this, memory behavior should be treated as unspecified unless and until this page defines it explicitly.

## Guidance for users (pre-alpha)

- Do not rely on any implicit ownership or borrowing behavior.
- Do not assume pointer/reference safety guarantees.
- Treat examples in other pages as syntax exploration unless this page marks semantics as stable.

## Planned scope for this page

Future revisions should define:

- ownership model and move/copy behavior,
- reference and mutability rules,
- lifetime semantics,
- aliasing guarantees and undefined behavior boundaries,
- value vs object identity semantics.
