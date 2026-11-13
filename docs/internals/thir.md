# THIR and type checking

THIR (typed high-level IR) is the output of type checking and the input
of MIR construction. It keeps the tree shape of the source, but every
question the AST leaves open has an answer:

- **Names are resolved.** A variable becomes a `BindingId` and a function a
  `DefId`. Two variables that shadow each other are two different bindings.
- **Every expression has a concrete `Type`.** Each literal has the type its
  context gave it and has been range-checked.
- **Implicit conversions are explicit.** Integer and float widening becomes
  an `ExprKind::Cast`. Reading through a `&T` becomes an `ExprKind::Deref`.
  `@x` records whether it builds a `*T` or a `&T`.

Code that consumes THIR never infers types again and never looks at the AST.

```text
AST ──typecheck::check_program──▶ THIR ──mir::build_mir──▶ MIR (Built)
```

## Module map

| File | Role |
| --- | --- |
| `middle/ops.rs` | `BinOp`, `UnOp`, `CastKind`, `IndirectionKind`, shared by THIR and MIR. |
| `middle/thir/mod.rs` | The THIR data types. |
| `middle/thir/visit.rs` | A read-only `Visitor` that walks nodes in evaluation order. |
| `middle/thir/pretty.rs` | Deterministic dumps (`x@b0`, `5_i32`, `(a as i64 [IntToInt])`). |
| `middle/typecheck/mod.rs` | `check_program`, item collection (signatures, attributes, the entry point), and the per-function context. |
| `middle/typecheck/entry.rs` | `validate_entry_point`: the executable entry contract, checked on THIR. |
| `middle/typecheck/scope.rs` | Lexical scopes that map names to bindings. |
| `middle/typecheck/literal.rs` | Classifies untyped literal expressions. |
| `middle/typecheck/expr.rs` | Expressions, places, literals, and calls. |
| `middle/typecheck/operator.rs` | Unary, binary, compound, shift, and comparison operators. |
| `middle/typecheck/stmt.rs` | Blocks, statements, `break`, and loops. |
| `middle/typecheck/tuple.rs` | Tuple expressions, `.N`, destructuring `let`, parallel assignment, and bare `return`. |

## Typing rules

- **Literals.** An integer or float literal takes its expected type when
  there is one; otherwise it defaults to `i64` or `f64`. In a binary
  operation, a literal operand takes the type of the other operand. A
  literal that does not fit its type is an error. When the literal
  initializes a variable, the error names the variable.
- **Arithmetic and bitwise operators** require both operands to have the
  same type. A shift amount may be any integer type.
- **Comparisons** of integer types with the same signedness widen to the
  wider type. Mixed-sign operands widen to a signed type whose width is the
  maximum of the signed width and the unsigned width plus one; for example,
  `i32 == u32` compares at `i33`. Comparisons requiring a width above the
  supported maximum of 8,388,608 bits are rejected. Two floats of
  different types widen to the wider one. Pointers compare only with `==`
  and `!=`, and only to the same pointer type.
- **Logical operators** `and`, `or`, and `xor` take `bool` operands and
  evaluate both sides. MIR lowers them to `BitAnd`, `BitOr`, and `BitXor`.
- **Places** are bindings, dereferences, and elements (`.N`) of places. A
  binding or tuple element of type `&T` is dereferenced automatically
  wherever it is used.
- **Tuples.** A tuple literal gives each element the matching element of
  its expected tuple type, if the arities agree. A tuple literal of the
  wrong length is reported as an arity mismatch ("expected 2 values, found
  3"), a non-tuple counting as one value. Tuples compare only with `==` and
  `!=`, only against the same type, and not when they contain references.
  THIR keeps such a comparison as a `Binary` on tuple operands, which MIR
  construction expands.
- **Partial expectations.** A `_` in a pattern or assignment target leaves
  its element unconstrained without dropping the expected types of the
  others (`Expectation::Tuple`), through tuple literals, conditional
  expressions, and loop `break` values.
- **Destructuring and parallel assignment are desugared** into ordinary
  statements over unnamed temporaries (bindings with no name):
  - `let i32 a, i32 b = e1, e2;` matches a tuple literal element by element
    and becomes `let a = e1; let b = e2;`. Every initializer is checked
    before any name is declared. Any other initializer is stored in a
    temporary and destructured with `.N` reads.
  - `a, b = e1, e2;` stores each value in a temporary, left to right, then
    assigns each target, left to right. A target assigned twice, or a
    tuple together with one of its elements, is an error.
  - `_` evaluates its value for side effects and drops it.
- **Results.** `-> i32 q, i32 r` declares two `NamedReturn` bindings and
  returns `(i32, i32)`. A bare `return;` returns the named results (a tuple
  of them if there are several), or `()` in a `()` function, and is an
  error if a named result is shadowed at that point. A function without
  `->` returns `()`.
- **Address expressions.** Address literals require a pointer/reference type
  from context. `@place` produces a reference when a reference is expected,
  and a raw pointer otherwise. In operand or conditional-branch pairs, a
  context-dependent address expression uses the other side's type when that
  side is not context-dependent; this works in either order. When both sides
  are context-dependent, they use the enclosing expected type instead.
- **Loops.** The first `break` fixes a loop's type. A loop with no `break`
  takes the type its context expects, or `i64` if there is none. Every
  `break` in the same loop must agree. Reference-valued loops are rejected.
- **Scopes.** A declaration is visible from the next statement until the
  end of its block.
- **Constant expressions are not folded.** `200 + 100` is an addition of two
  literals, typed like any other operation. A literal operand must fit its
  type on its own, and whether the operation traps is decided on MIR (see
  [MIR](mir.md#compile-time-traps)).
- **Attributes and entry point.** Item collection rejects unknown attributes
  and a second `#[entry]` function before any body is checked.
  `validate_entry_point` then requires an entry taking no parameters and
  returning `u8` or `()`.

## Migration from the AST validator

The driver uses THIR for semantic validation; the separate AST validator and
backend type-inference helper have been removed. The typed checker deliberately
differs from the former validator in these cases:

- A declaration no longer leaks out of the block it is in.
- An `i64` value is no longer treated as an untyped literal. For example,
  `let i32 x = some_i64` and `i32 + i64` are now errors.
- Every literal is range-checked against its contextual type, so
  `u8 < 300` is now an error.
- A mixed-sign comparison whose wider type is signed now compares as
  signed.
- Defining two functions with the same name is an error.
- Address literals are range-checked against the target's pointer width.

THIR also accepts compound assignment on floats (`x += 2.5`), which the
legacy validator rejected.
