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
| `middle/typecheck/mod.rs` | `check_program`, plus the global and per-function contexts. |
| `middle/typecheck/scope.rs` | Lexical scopes that map names to bindings. |
| `middle/typecheck/literal.rs` | Classifies untyped literal expressions. |
| `middle/typecheck/expr.rs` | Expressions, places, literals, and calls. |
| `middle/typecheck/operator.rs` | Unary, binary, compound, shift, and comparison operators. |
| `middle/typecheck/stmt.rs` | Blocks, statements, `break`, and loops. |

## Typing rules

- **Literals.** An integer or float literal takes its expected type when
  there is one; otherwise it defaults to `i64` or `f64`. In a binary
  operation, a literal operand takes the type of the other operand. A
  literal that does not fit its type is an error. When the literal
  initializes a variable, the error names the variable.
- **Arithmetic and bitwise operators** require both operands to have the
  same type. A shift amount may be any integer type.
- **Comparisons** of two different integer types widen both operands to
  the wider type. On equal widths, the unsigned type wins. Two floats of
  different types widen to the wider one. Pointers compare only with `==`
  and `!=`, and only to the same pointer type.
- **Logical operators** `&&`, `||`, and `^^` take `bool` operands and
  evaluate both sides. MIR lowers them to `BitAnd`, `BitOr`, and `BitXor`.
- **Places** are bindings and dereferences. A binding of type `&T` is
  dereferenced automatically wherever it is used.
- **Loops.** The first `break` fixes a loop's type. A loop with no `break`
  takes the type its context expects, or `i64` if there is none. Every
  `break` in the same loop must agree.
- **Scopes.** A declaration is visible from the next statement until the
  end of its block.

## Differences from the legacy validator

`middle/validate.rs` still decides whether a program compiles, until the
driver switches to MIR. THIR is deliberately stricter in these cases:

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
