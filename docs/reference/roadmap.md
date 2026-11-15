# Roadmap

This page tracks long-term goals currently listed in project notes.

## Long-term goals

- Native standard library
- Self-hosted compiler
- Cargo-like tooling (`xenon new`, `xenon build`, `xenon run`)
- VS Code extension (syntax highlighting, completion, debugging support)
- Documentation website/community resources
- Package registry
- More platforms and architectures (including embedded/mobile)
- JIT compilation and REPL support

## Before modules and FFI

- **Mangle struct and function names.** Compiled symbols are the source
  names today: a function is `name`, a member of a struct is
  `Struct.member`, and a property getter is `Struct.property.get`. That
  leaks backend names into the language, which is why a struct cannot be
  named `_xe` or `llvm`, and it cannot tell apart two `Point` structs
  declared in different modules. A mangling scheme has to land before
  modules; the two reserved struct names are dropped with it. FFI then
  needs a way to opt a function out of mangling.

## Status

No milestone dates are documented yet.

No doc yet. TODO: add phased milestones, priorities, and acceptance criteria.
