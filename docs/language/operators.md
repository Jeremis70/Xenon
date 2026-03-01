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
| `**`     | Exponentiation |

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
