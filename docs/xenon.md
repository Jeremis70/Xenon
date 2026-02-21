# Xenon (Xe)

## Overview

Xenon is a strongly typed systems language that sits between C and Rust — taking what is great from both without the complexity of Rust's borrow checker or the footguns of raw C. It compiles to LLVM IR via `inkwell`, with the compiler (`xenonc`) implemented in Rust.

- File extension: `.xe`
- Compiler: `xenonc`
- Backend: LLVM IR via `inkwell`
- Compiler implementation: Rust

**Long-term goals (optional):**
- Native standard library
- Self-hosted compiler
- Cargo-like tooling (`xenon new`, `xenon build`, `xenon run`)
- VSCode extension with syntax highlighting, code completion, and debugging support
- Website (github pages or custom) with documentation, examples, and community resources
- Package registry for sharing libraries and tools (probably will never happen but would be cool)

## Variables

Variables are immutable by default.

### Modifiers

- `mut` — mutable variable.
- `const` — compile-time constant; must be initialized with a constant expression.
- `static` — global variable with static storage duration; must be initialized with a constant expression.
- `volatile` — value that may be modified externally and must not be optimized away (e.g., memory-mapped I/O).

### Types

#### Integers

- Signed: `iN` where N is the bit width — `i8`, `i16`, `i32`, `i64`, `i128`.
- Unsigned: `uN` where N is the bit width — `u8`, `u16`, `u32`, `u64`, `u128`.
- Supports any bit width but recommended to stick to standard sizes since I'm not sure yet what LLVM does when using their non-standard widths.

#### Floating-Point

- `f32`, `f64` — standard IEEE 754.
- Planned: `f16`, `bf16`, `f128`.

#### Boolean

- `bool` (`true` or `false`).
- `packed bool` modifier: stores consecutive booleans as bits in the smallest practical unit.
  - Grows through `u8 → u16 → u32 → u64`, capped at pointer width.
  - After one pointer-width chunk is full, packing restarts in a new `u8`.
  - Packed booleans are not addressable — taking their address is a compile error.
  - Applies to struct fields as well.

### Pointers

Raw pointer syntax:

```xe
u8* ptr; // raw pointer to u8
```

Pointer declaration and assignment (`ptr = &x`) are always allowed. Dereferencing, pointer arithmetic, and raw address assignment (`ptr = 0x650439`) might require an `unsafe` block TBD.

### Arrays

Fixed-size arrays with compile-time known length:

```xe
u8 arr[10]; // array of 10 u8s
```

Out-of-bounds accesses are caught at compile time when the index is known, and at runtime otherwise.

### Example

```xe
u8 x = 5;     // immutable
x = 6;        // compile error: cannot assign to immutable variable

mut u8 y = 5; // mutable
y = 6;        // ok
```

---

## Functions

Functions can have multiple input parameters and optionally one or more return values.

### Modifiers

- `entry` — program entry point.
- `inline` — request inlining (compiler may ignore).
- `noinline` — request no inlining (compiler may ignore).

### Overloading

Function overloading is supported based on parameter types, counts, and return types:

```xe
fn foo(u32 x) -> u32 { ... }
fn foo(u64 x) -> u64 { ... }
fn foo(u32 x, u32 y) -> u32 { ... }
fn foo(u32 x) -> u64 { ... }
```

If the compiler cannot resolve which overload to call, it emits an error and asks for an explicit type annotation.

### Default Parameter Values

```xe
fn foo(u32 x, u32 y = 10) -> u32 { ... }

u32 out = foo(5);              // y defaults to 10
u32 out = foo(5, 20);          // y is 20
u32 out = foo(y = 12, x = 5); // named arguments
```

Default values work alongside overloading:

```xe
fn foo(u32 x, u32 y = 10) -> u32 { ... }
fn foo(u64 x, u64 y = 20) -> u64 { ... }

u32 out = foo(5); // resolves to u32 overload, y = 10
u64 out = foo(5); // resolves to u64 overload, y = 20
```

### Multiple Return Values (Named Returns)

```xe
fn count_colored_sheep(SheepImage img) -> u32 white, u32 black, u32 other, u32 total {
    foreach (sheep in img) {
        if (sheep.color == white)      { white++; }
        else if (sheep.color == black) { black++; }
        else                           { other++; }
        total++;
    }
}
```

Named return values behave as local variables and are implicitly returned at function end.

Equivalent explicit form:

```xe
fn count_colored_sheep(SheepImage img) -> u32, u32, u32, u32 {
    mut u32 white = 0, black = 0, other = 0, total = 0;
    foreach (sheep in img) {
        if (sheep.color == white)      { white++; }
        else if (sheep.color == black) { black++; }
        else                           { other++; }
        total++;
    }
    return white, black, other, total;
}
```

Calling:

```xe
u32 white, u32 black, u32 other, u32 total = count_colored_sheep(img);
```

---

## Operators

### Arithmetic

| Operator | Description    |
| -------- | -------------- |
| `+`      | Addition       |
| `-`      | Subtraction    |
| `*`      | Multiplication |
| `/`      | Division       |
| `%`      | Modulo         |
| `**`     | Exponentiation |

### Bitwise

| Operator | Description |
| -------- | ----------- |
| `&`      | AND         |
| `\|`     | OR          |
| `^`      | XOR         |
| `~`      | NOT         |
| `<<`     | Left shift  |
| `>>`     | Right shift |

### Logical and Comparison

| Operator          | Description                                           |
| ----------------- | ----------------------------------------------------- |
| `&&`              | Logical AND                                           |
| `\|\|`            | Logical OR                                            |
| `^^`              | Logical XOR (true only when exactly one side is true) |
| `!`               | Logical NOT                                           |
| `==` `!=`         | Equality / inequality                                 |
| `<` `>` `<=` `>=` | Comparison                                            |

### Assignment Variants

Compound assignment for all arithmetic and bitwise operators: `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`.

Increment and decrement: `++`, `--` — statement-only, not usable as expressions.

---

## Overflow Modes

For overflow-prone operations, mode suffixes control behavior:

| Suffix | Behavior                                      |
| ------ | --------------------------------------------- |
| (none) | Default — may overflow                        |
| `%`    | Wrapping                                      |
| `\|`   | Saturating                                    |
| `?`    | Checked — returns result + bool overflow flag |

Examples using `+`:

```xe
a + b   // normal
a +% b  // wrapping
a +| b  // saturating
a +? b  // checked

u8 result, bool overflow = a +? b;
```

Suffixes can be combined:

```xe
a +%? b  // checked wrapping
a +|? b  // checked saturating
```

Assignment forms follow the same pattern: `+=%`, `+=|`, `+=?`, `+=%?`, etc.

---

## Control Flow

### Loops

```xe
while (condition) { ... }
for (init; condition; update) { ... }
do { ... } while (condition);
loop { ... }                          // infinite loop
foreach (x in y) { ... }             // iterate over arrays, slices, iterables
foreach (u8 byte in buffer) { ... }  // with optional type annotation
```

Ranges are handled by `for`, not `foreach`.

#### Loop Labels

```xe
outer: while (condition) {
    inner: for (i = 0; i < 10; i++) {
        break outer;
    }
}
```

#### Loop Return Values

Loops can produce values via `break`. The `finally` keyword provides a default value if the loop exits normally:

```xe
u8 x = while (condition) {
    if (found) { break 5; }
    finally 0;
};
```

#### Break and Continue

```xe
break;             // exit innermost loop
break label;       // exit labeled loop
break value;       // exit innermost loop, return value
break label value; // exit labeled loop, return value

continue;          // next iteration of innermost loop
continue label;    // next iteration of labeled loop
```

#### Loop `else`

```xe
while (condition) {
    // body
} else {
    // runs only if the loop never executed (condition was false from the start)
}
```

#### Example: Search with Return Value

```xe
u32 john_index = foreach (user in users) {
    if (user.name == "John") {
        break i;   // i is the current iteration index
    }
    finally 1024;  // sentinel: not found
} else {
    finally 1026;  // sentinel: database was empty or iteration failed
};
```

### Conditionals

```xe
if (condition) { ... }
else if (condition) { ... }
else { ... }
```

`if` as an expression:

```xe
u8 x = if (condition) 5 else 10;
```

Postfix form:

```xe
u8 x = 5 if (condition) else 10;
```

### `match`

```xe
match (value) {
    1       => { ... }
    2 | 3   => { ... }
    default => { ... }
}
```

Full pattern matching spec: TBD.

### Low-Level Control Flow

```xe
goto label;   // unconditional jump
label:        // jump target (also used for loop labels)
nop;          // no-op, may emit actual CPU NOP
unreachable;  // hint: this point should never be reached
asm { ... }   // inline assembly
```

### Program Entry Point

```xe
entry fn main() { ... }
```

Attribute form (alternative) TBD:

```xe
#[entry]
fn main() { ... }
```