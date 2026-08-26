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

## Pointers (draft)

Pointers use a prefix `*` followed by the pointee type:

```xe
*u32 p;
**u32 p_to_p;
```

`*u32` is a pointer to a `u32`; `**u32` is a pointer to a pointer to a
`u32`. Pointer types are distinct from integer types, including `usize`.

The address-of operator `@` creates a pointer:

```xe
u32 x;
*u32 p = @x;
**u32 p_2 = @p;
```

An integer literal prefixed with `@` is an address literal rather than an
integer literal:

```xe
*u32 device_register = @0xFFFFFFFF;
```

The literal address is checked against the target pointer width. It may still
be invalid, unmapped, or unsuitable for the requested pointee type; using or
dereferencing such an address is the programmer's responsibility.

To convert an integer value held in a variable into a pointer, use an explicit
cast. This is different from taking the address of that variable:

```xe
usize address = 0xFFFFFFFF;
*u32 p = address as *u32; // address stored in `address`
*usize q = @address;       // address of the variable `address`
```

## References (draft)

References use a prefix `&` in the type. The `@` operator creates a reference,
just as it creates a pointer. Access through a reference uses ordinary variable
syntax with no explicit dereference needed.

```xe
u32 x = 42;
&u32 r = @x;
r = 10; // modifies x directly
```

When a function expects a reference parameter, pass the variable with `@`:

```xe
fn increment(&u32 value) {
    value = value + 1;
}

u32 n = 5;
increment(@n); // n is now 6
```

Mutable references, written `&mut T`, are planned but their borrowing and
lifetime rules are not finalized.

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
