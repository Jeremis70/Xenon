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

## Pointers and references

| Operator | Description |
| --- | --- |
| `@x` | Address-of / reference-of the place `x` |
| `@0x...` | Address literal |
| `*p` | Dereference a pointer `p` |

The prefix `*` has different meanings based on context: it is multiplication
between expressions, a pointer-type constructor before a type, and
dereference before a pointer expression. As a prefix operator it binds tighter
than every infix operator, so `*p + 1` is `(*p) + 1` and `*p * b` is
`(*p) * b`.

`@` accepts only an addressable location (a variable or a dereference), never
a temporary such as a call result.

Dereference expressions are assignable places, so assignment can write through
a pointer:

```xe
let *u32 p = @x;
*p = 42;
*p += 1;   // reads, updates, and writes back through the same address
```

Compound assignment (`+=`, `-=`, …) and `++` / `--` behave as `place = place op
value` would, but the place is evaluated only once.

`@name` always takes the address of the variable named `name`. If an integer
variable contains a numeric address, it must be converted explicitly. The `as`
operator is not implemented yet:

```xe
let usize address = 0xFFFFFFFF;
let *u32 p = address as *u32; // planned, not yet available
```

References use `&T` in a declaration and are created with `@`:

```xe
let u32 x = 42;
let &u32 r = @x;
r = 10; // writes through to x; no `*` needed
```

The `@` operator is used for both pointers (`let *u32 p = @x`) and references
(`let &u32 r = @x`). The expected type determines which is produced. The `&` in
`&T` is a type marker, not an address-of operator. Between expressions, `&`
remains bitwise AND, so `@x & mask` is `(@x) & mask`.

`==` and `!=` compare two pointers of the same type for identity. No other
operator is defined on pointers; pointer arithmetic is not specified yet.

References are non-owning and are intended to follow Rust-like borrowing
rules, but Xenon's lifetime and aliasing rules are not finalized.


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
