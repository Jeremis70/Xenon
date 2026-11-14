# Types

This page documents currently listed primitive and numeric type ideas.

## Integer types

- Signed forms: `iN` (examples: `i8`, `i16`, `i32`, `i64`, `i128`)
- Unsigned forms: `uN` (examples: `u8`, `u16`, `u32`, `u64`, `u128`)

### Special integer notes (draft)

- `u1` and `i1` are described as equivalent one-bit integer forms.
- `u0` and `i0` are zero-sized integers: they have no bits and only hold `0`.
  Non-trapping arithmetic on them yields `0`. Division or remainder by a
  zero-sized value always traps, and so does shifting one. They are distinct
  from the planned unit type `()`.
- Integer polymorphism/object model is discussed but explicitly not finalized.

## Floating-point types

- Listed: `f16`, `bf16`, `f32`, `f64`, `f128`
- Uncertain draft idea: `float<mantissa, exponent>` generic form.

## Boolean

- `bool` with values `true` and `false`
- Draft distinction: `bool` is logical, while `u1` participates in integer arithmetic.

## Pointers

Pointers use a prefix `*` followed by the pointee type:

```xe
let *u32 p = @x;
let **u32 p_to_p = @p;
```

`*u32` is a pointer to a `u32`; `**u32` is a pointer to a pointer to a
`u32`. Pointer types are distinct from integer types, including `usize`, and
there is no implicit conversion in either direction.

The address-of operator `@` creates a pointer:

```xe
let u32 x = 0;
let *u32 p = @x;
let **u32 p_2 = @p;
```

Pointers are opaque: reading or writing the pointee requires an explicit
dereference with `*`.

```xe
let u32 value = *p; // read through the pointer
*p = 7;             // write through the pointer
```

An integer literal prefixed with `@` is an address literal rather than an
integer literal:

```xe
let *u32 device_register = @0xFFFFFFFF;
```

An address literal only type-checks where a pointer or reference type is
expected. The literal is checked against the target pointer width at compile
time. It may still be invalid, unmapped, or unsuitable for the requested
pointee type; using or dereferencing such an address is the programmer's
responsibility.

The only operators defined on pointers are `==` and `!=` between two values of
the same pointer type. Pointer arithmetic is not specified yet.

### Converting between integers and pointers

Converting an integer *value* into a pointer requires an explicit cast, which
is different from taking the address of the variable holding it. Pointers
convert only to and from `usize`, which has the same width:

```xe
let usize address = 0xFFFFFFFF;
let *u32 p = address as *u32; // the address stored in `address`
let *usize q = @address;      // the address of the variable `address`
let usize back = p as usize;
let *u8 bytes = p as *u8;     // pointer to pointer
```

To convert a pointer to another integer type, go through `usize`:
`p as usize as u32`. The second cast is checked like any integer cast (see
[Conversions](#conversions)).

## References

References use a prefix `&` in the type. The `@` operator creates a reference
just as it creates a pointer — the expected type at the use site decides which
one is produced.

References are *transparent*: access uses ordinary variable syntax with no
explicit dereference, and assignment writes through to the referent.

```xe
let u32 x = 42;
let &u32 r = @x;
r = 10;             // modifies x directly
let u32 copy = r;   // reads through to x
```

Because assignment writes through, a reference can only be bound at its
declaration; it cannot be rebound to a different location afterwards. Writing
`*r` is an error, since `r` already denotes the referent.

When a function expects a reference parameter, pass the variable with `@`:

```xe
fn increment(&u32 value) -> u32 {
    value = value + 1;
    return value;
}

let u32 n = 5;
increment(@n); // n is now 6
```

References are non-owning. Mutable references, written `&mut T`, are planned
but their borrowing and lifetime rules are not finalized. The compiler does
**not** currently check lifetimes, aliasing, or exclusivity — see the
[Memory Model](../semantics/memory-model.md).

## Tuples

Tuples are anonymous positional product types written as `(T1, T2, ...)`:

```xe
let (u32, u32) pair = (10, 20);
let u32 x = pair.0;
pair.1 = 30;              // elements of a variable are assignable
let (i32, (bool, u8)) nested = (1, (true, 2));
let u8 inner = nested.1.1;
```

- `()` is the **unit** type, the empty tuple. A function without a return
  type returns `()`.
- `(T,)` is a one-element tuple; `(T)` is just `T`. The same holds for
  values: `(x,)` is a tuple, `(x)` is `x`.
- Elements are read by 0-based index, `t.0`, which binds tighter than any
  operator (`-t.0` is `-(t.0)`). The index must be plain decimal digits
  without a leading zero: `t.0x1`, `t.01`, and `t.1e3` are errors.
- An element of type `&T` denotes its `T`, like a variable of type `&T`.
  A pointer to a tuple must be dereferenced explicitly: `(*p).0`.
- Tuples compare with `==` and `!=`, element by element, and only with a
  tuple of exactly the same type. Both sides are evaluated completely, left
  to right, before any element is compared. Ordering (`<`, ...) is not
  defined on tuples.
- A tuple literal checked against a tuple type gives each element its
  expected type: in `let (u8, i64) p = (1, 2);` the literals are `u8` and
  `i64`. Elements are never widened implicitly.

Most code never names a tuple type. Multiple function results
(`fn foo() -> u32 x, u32 y`) are tuples, and they are usually taken apart
right away with a destructuring declaration:

```xe
let u32 x, u32 y = foo();
let u32 first, _ = foo();           // `_` ignores a value
let i32 a, (bool b, u8 c) = 1, (true, 2);
```

Every new name spells its type: there is no type inference. See
[Syntax Basics](syntax-basics.md#tuples-and-parallel-assignment) for
destructuring and parallel assignment, and [Functions](functions.md) for
multiple results.

## Related pages

- [Operators](operators.md)
- [Syntax Basics](syntax-basics.md)
- [Memory Model](../semantics/memory-model.md)

## Conversions

Values change type only through an explicit cast, `value as Type`. **A cast
never changes a value silently: it is exact or it panics.**

- A cast that can represent every value of the source type, such as
  `u8 as i32` or `f32 as f64`, always succeeds and costs nothing.
- Any other cast checks the value when the program runs and panics if the
  target type cannot represent it exactly.
- If the value is known at compile time, a cast that would panic is a
  compile error instead.

| Cast | Never panics when | Otherwise panics unless |
| --- | --- | --- |
| integer to integer | the target range contains the source range | the value is in the target's range |
| integer to float | the integer fits the float's significand: `iN` needs N - 1 bits, `uN` N bits; `f16` has 11, `bf16` 8, `f32` 24, `f64` 53, `f128` 113 | the float holds the value exactly |
| float to integer | never | the value is finite, a whole number, and in range |
| float to float | widening: `f16`/`bf16` to `f32`, `f64`, `f128`; `f32` to `f64`, `f128`; `f64` to `f128` | the narrower float holds the value exactly; NaN stays NaN |
| pointer to pointer, `usize` to/from pointer | always | — |

```xe
let i32 total = 200;
let u8 code = total as u8;    // checked: panics unless 0 <= total <= 255
let i64 wide = total as i64;  // lossless: no check
let f64 x = 2.5;
let i32 n = x as i32;         // panics: 2.5 is not a whole number
let u8 bad = 300 as u8;       // compile error: 300 is out of range for `u8`
```

Whether a cast between `usize`/`isize` and another integer type needs a check
depends on the target's pointer width: `u32 as usize` is lossless on 64-bit
targets and checked on 16-bit ones.

An untyped literal takes the target type of the cast when it can, so
`5 as u8` is the `u8` literal `5`. `bool`, tuples, and references have no
casts.

Casts that deliberately wrap, truncate, or saturate are planned as separate,
explicitly named operations.
