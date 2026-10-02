# Compiler Architecture

`xenonc` is a single Rust crate (`compiler/`) organized as a pipeline of
stages. Each stage depends only on the stages and shared modules below it.

## Layering

```text
driver      CLI, session, diagnostics, pipeline wiring
backend     LLVM codegen, linking            (consumes MIR in later milestones)
middle      semantic analysis, MIR, passes   (target-independent)
frontend    lexer, parser, AST
shared      source (Span), types (Type), index (IndexVec, newtype_index!)
```

The shared modules at the crate root are stage-neutral, so the middle end
never imports syntax modules and a backend never imports the AST:

| Module | Contents |
| --- | --- |
| `source.rs` | `Span`, byte ranges into the source. |
| `types.rs` | `Type`, the semantic type shared by every stage. |
| `index.rs` | `Idx`, `IndexVec<I, T>`, and the `newtype_index!` macro. |
| `middle/ids.rs` | `DefId`, the identity of a function across IRs. |
| `middle/target.rs` | `TargetSpec`: pointer width and integer ranges, without LLVM. |

`frontend::tokens::Span` and `frontend::ast::Type` re-export the shared
definitions for compatibility.

## Current pipeline

```text
lex → parse → constant-fold → validate → codegen (LLVM) → link
```

## Target pipeline

```text
lex → parse → resolve/typecheck → THIR → MIR build
    → MIR verify + flow checks (Checked) → runtime lowering (Runtime)
    → MIR optimizations → backend (LLVM, Cranelift, ...) → link
```

The MIR core (`middle/mir/`) is in place. It is not wired into the driver
yet; the existing AST-based pipeline is unchanged. See [MIR](mir.md).

### Which checks go where

- **AST or THIR** (needs source structure and names): name resolution, type
  checking, literal range checks, `break`/`continue` placement, and
  user-facing diagnostics.
- **MIR** (needs control flow): missing `return`, use of uninitialized
  locals, unreachable code, insertion of runtime overflow, division, and
  shift checks, and every optimization. The MIR verifier only reports
  compiler bugs, never user errors.
