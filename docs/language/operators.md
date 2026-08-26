# Operators

This page lists currently documented operator families and overflow suffix modes.

## Arithmetic

| Operator | Description    |
| -------- | -------------- |
| `+`      | Addition       |
| `-`      | Subtraction    |
| `*`      | Multiplication |
| `/`      | Division       |
| `%`      | Modulo         |

## Pointers and references (draft)

| Operator | Description |
| --- | --- |
| `@x` | Address-of / reference-of `x` |
| `@0x...` | Address literal |
| `*p` | Dereference a pointer `p` |

The prefix `*` has different meanings based on context: it is multiplication
between expressions, a pointer-type constructor before a type, and
dereference before a pointer expression.

Dereference expressions are assignable places, so assignment can write through
a pointer:

```xe
*u32 p = @x;
*p = 42;
```

`@name` always takes the address of the variable named `name`. If an integer
variable contains a numeric address, convert its value explicitly instead:

```xe
usize address = 0xFFFFFFFF;
*u32 p = address as *u32;
```

References use `&T` in a declaration and are created with `@`:

```xe
u32 x = 42;
&u32 r = @x;
r = 10; // modifies x directly
```

The `@` operator is used for both pointers (`*u32 p = @x`) and references
(`&u32 r = @x`). The target type determines whether a pointer or reference is
produced. The `&` in `&T` is a type marker, not an address-of operator.
Between expressions, `&` remains bitwise AND. References are non-owning and
are intended to follow Rust-like borrowing rules, but Xenon's lifetime and
aliasing rules are not finalized.


## Bitwise

| Operator | Description |
| --- | --- |
| `&` | AND |
| `|` | OR |
| `^` | XOR |
| `~` | NOT |
| `<<` | Left shift |
| `>>` | Right shift |

## Logical and comparison

| Operator | Description |
| --- | --- |
| `&&` | Logical AND |
| `||` | Logical OR |
| `^^` | Logical XOR |
| `!` | Logical NOT |
| `==`, `!=` | Equality and inequality |
| `<`, `>`, `<=`, `>=` | Comparison |

## Assignment variants

- Compound assignment includes arithmetic and bitwise forms (for example `+=`, `&=`, `>>=`).
- Increment/decrement (`++`, `--`) are statement-only in the draft notes.

## Overflow modes

Draft suffixes for overflow-sensitive operations:

| Suffix | Behavior |
| --- | --- |
| none | default behavior (may overflow) |
| `%` | wrapping |
| `|` | saturating |
| `?` | checked (returns result + overflow flag) |

Example:

```xe
a + b
a +% b
a +| b
a +? b
u8 result, bool overflow = a +? b;
```

Combined suffixes are documented as possible (`+%?`, `+|?`).
