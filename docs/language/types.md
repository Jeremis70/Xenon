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

### Converting an integer to a pointer (planned)

Converting an integer *value* into a pointer requires an explicit cast, which
is different from taking the address of the variable holding it. The `as`
operator is **not implemented yet**; only `@` is available today.

```xe
let usize address = 0xFFFFFFFF;
let *u32 p = address as *u32; // planned: address stored in `address`
let *usize q = @address;      // available: address of the variable `address`
```

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
(u32, u32) pair = (10, 20);
u32 x = pair.0;
u32 y = pair.1;
```

Tuples are nameless (fields are accessed strictly by 0-based index or via destructuring `u32 x, u32 y = pair`). Multiple function returns (`fn foo() -> u32 x, u32 y`) evaluate to tuple types under the hood.

## Related pages

- [Operators](operators.md)
- [Syntax Basics](syntax-basics.md)
- [Memory Model](../semantics/memory-model.md)
