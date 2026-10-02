# MIR

MIR (mid-level IR) is the control-flow-graph representation of Xenon
functions. Its design follows rustc's MIR. It lives in
`compiler/src/middle/mir/`.

## Why MIR

- **Backend independence.** Backends translate a small, closed set of
  primitives and never see the AST. A Cranelift or custom backend only has
  to implement those primitives.
- **One place for control flow.** `for` loops, generators, short-circuiting,
  and early returns all lower to blocks and jumps. Flow-sensitive checks and
  optimizations are written once, over MIR.
- **Explicit semantics.** Evaluation order, temporaries, implicit casts, and
  runtime checks are explicit statements, so they can be inspected with
  `pretty` and verified.

## Vocabulary

A `Body` is one function:

- **Locals** (`_0`, `_1`, ...). `_0` is the return place. `_1` to `_n` are
  the arguments. The others are user variables (with a `debug_name`) or
  temporaries. Every local has a type.
- **Basic blocks** (`bb0`, `bb1`, ...). Execution starts at `bb0`. Each block
  is a list of statements ended by exactly one terminator.
- **Statements**: `Assign(place, rvalue)`, `StorageLive`, `StorageDead`, `Nop`.
- **Terminators**: `Goto`, `SwitchInt`, `Call`, `Assert`, `Return`,
  `Unreachable`, and `EndOfBody` (allowed only in the `Built` phase).
- **Places**: a local plus projections, for example `(*_1)`. Arrays and
  structs will add projection elements.
- **Operands**: `copy place` or `const value`.
- **Rvalues**: `Use`, `UnaryOp`, `BinaryOp`, `Overflows`, `Cast`, `AddressOf`.

Example dump:

```text
// MIR for `add_one` (phase: built)
fn add_one(_1: i32) -> i32 {
    let _0: i32;
    let _2: bool;
    debug x => _1;

    bb0: {
        StorageLive(_2);
        _2 = OverflowsAdd(copy _1, const 1_i32);
        assert(!copy _2, "attempt to add with overflow") -> bb1;
    }

    bb1: {
        _0 = Add(copy _1, const 1_i32);
        StorageDead(_2);
        return;
    }
}
```

## Phases

`Body::phase()` only moves forward (`advance_phase`):

| Phase | Guarantees |
| --- | --- |
| `Built` | Freshly lowered. `EndOfBody` marks paths that fall off the end. |
| `Checked` | Flow checks have passed. No `EndOfBody`. |
| `Runtime` | Runtime checks are explicit. Ready for optimizations and backends. |

## Invariants

The builder guarantees that every block is terminated. `verify_body` and
`verify_program` check the rest and collect every violation:

- Every local, block, and scope index exists. A scope's parent precedes it.
- The signature matches the declaration: argument count, argument types, and
  return type.
- Every rvalue is well-typed by the rules in `typing.rs`, and its type equals
  the type of the place it is assigned to.
- Binary operands have identical types; shift amounts may be any integer.
- Constants match their type, and their values fit it on the target. This
  includes `usize`, `isize`, and addresses, through `TargetSpec`.
- A `SwitchInt` discriminant is a `bool` or an integer, and its values are
  distinct and in range. An `Assert` condition is a `bool`.
- A call matches its callee's declared arity, argument types, and return
  type.
- There are no storage markers on `_0` or on arguments.
- A construct appears only in a phase that allows it.

A verification error is a compiler bug, never a user error.

## Module map

| File | Role |
| --- | --- |
| `body.rs` | `Body`, `Local`, `BasicBlock`, scopes, `MirPhase`, cached predecessors. |
| `syntax.rs` | Statements, terminators, places, operands, rvalues, and their `Display`. |
| `program.rs` | `MirProgram`: function declarations (`FnDecl`) and bodies, in `DefId` order. |
| `typing.rs` | `place_ty`, `operand_ty`, `rvalue_ty`: the only typing rules. |
| `builder.rs` | `BodyBuilder`, the only way to create a `Body`. |
| `visit.rs` | `Visitor` and `MutVisitor`, generated from a single macro, with `PlaceContext`. |
| `traversal.rs` | `preorder`, `postorder`, `reverse_postorder`, `reachable_set`. |
| `verify.rs` | The invariant checker. |
| `pretty.rs` | Deterministic textual dumps. |

## Extending MIR

To add a language feature, first try to lower it to the existing
primitives. For example, a `for` loop becomes blocks, a `SwitchInt`, and
assignments, and needs no MIR change.

To add a new primitive, such as an `Index` projection for arrays:

1. Add the variant in `syntax.rs`, with its `Display`.
2. Give it a type rule in `typing.rs`.
3. Walk it in `visit.rs`, in the shared macro, choosing the right
   `PlaceContext`.
4. Add its invariants to `verify.rs`, with a negative test in
   `tests/mir_verify.rs`.
5. Implement it in each backend.

The compiler's exhaustive matches point to every other place that must
handle the new variant.
