# Compiler Architecture

`xenonc` is a single Rust crate (`compiler/`) organized as a pipeline of
stages. Each stage depends only on the stages and shared modules below it.

## Layering

```text
driver      CLI, session, diagnostics, pipeline wiring
backend     backend contract, LLVM MIR codegen, linking
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
| `middle/ids.rs` | `DefId` (a function across IRs) and `BindingId` (a THIR variable). |
| `middle/ops.rs` | Operators shared by THIR and MIR. |
| `middle/target.rs` | `TargetSpec`: pointer width and integer ranges, without LLVM. |

`frontend::tokens::Span` and `frontend::ast::Type` re-export the shared
definitions for compatibility.

## Driver pipeline

```text
lex → parse → constant-fold → entry validation → typecheck → THIR → MIR
    → flow analysis → runtime-check normalization → LLVM backend → link
```

`check` follows the same frontend and MIR correctness pipeline but does not
initialize LLVM. `check --emit=mir` prints Runtime MIR. `compile --emit=mir`
writes `out.mir`; `compile --emit=ir,obj` requests LLVM IR and/or a native
object file. `compile` defaults to emitting IR and an object before linking.
The current LLVM target is the host; explicit `--target` values are rejected
until target-machine selection is implemented.
`check --stage` defaults to `mir`; `compile --stage mir --emit mir` writes
Runtime MIR without invoking LLVM. Unsupported stage/emit combinations fail
explicitly.

The driver selects checked integer overflow behavior at `-O0` and wrapping
behavior at higher optimization levels. Division/remainder by zero and
out-of-range shifts are guarded in Runtime MIR independently of LLVM.

The pipeline's shared backend contract lives in `backend/contract.rs`.
`LlvmBackend` accepts verified Runtime MIR, target properties, codegen
options, and explicit output requests. Backend symbol assignment and
non-`main` entry-wrapper construction are isolated in `backend/prepare.rs`;
the wrapper is added as MIR and verified before LLVM emission. Preparation
consumes function identities and signatures, not AST names or syntax. A future
Cranelift backend can implement the same trait without changing type checking
or MIR passes.
LLVM is enabled by the default Cargo feature `llvm-backend`; without it,
checking and MIR emission remain available while native output is rejected.

The type-checking boundary also validates source-oriented entry/attribute
constraints. Semantic expression and binding validation is owned by THIR
construction, while control-flow correctness is owned by MIR analysis.
See [THIR and Type Checking](thir.md) and [MIR](mir.md).

### Which checks go where

- **AST or THIR** (needs source structure and names): name resolution, type
  checking, literal range checks, `break`/`continue` placement, and
  user-facing diagnostics.
- **MIR** (needs control flow): missing `return`, use of uninitialized
  locals, unreachable code, insertion of runtime overflow, division, and
  shift checks, and every optimization. The MIR verifier only reports
  compiler bugs, never user errors.
