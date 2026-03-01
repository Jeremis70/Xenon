# Types

This page documents currently listed primitive and numeric type ideas.

## Integer types

- Signed forms: `iN` (examples: `i8`, `i16`, `i32`, `i64`, `i128`)
- Unsigned forms: `uN` (examples: `u8`, `u16`, `u32`, `u64`, `u128`)

### Special integer notes (draft)

- `u1` and `i1` are described as equivalent one-bit integer forms.
- `u0` and `i0` are suggested as possible aliases to a unit type.
- Integer polymorphism/object model is discussed but explicitly not finalized.

## Floating-point types

- Listed: `f16`, `bf16`, `f32`, `f64`, `f128`
- Uncertain draft idea: `float<mantissa, exponent>` generic form.

## Boolean

- `bool` with values `true` and `false`
- Draft distinction: `bool` is logical, while `u1` participates in integer arithmetic.

## Related pages

- [Operators](operators.md)
- [Syntax Basics](syntax-basics.md)
