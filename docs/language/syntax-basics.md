# Syntax Basics

This page covers basic declarations and expression forms that are currently documented.

## Variables

Draft declaration shape:

```xe
<modifiers> <type> <name> = <initial value>;
```

Initial value is optional and is expected to default to zero

```xe
u64 x = 42;
```

## Address and pointer expressions

Pointer declarations and address operations follow these forms:

```xe
// Pointer to a u32:
let u32 x = 0;
let *u32 p = @x;

// Pointer to a fixed memory address:
let *u32 p_2 = @0xFFFFFFFF;

// Pointer to pointer:
let **u32 p_3 = @p_2;

// Read and write through a pointer:
let u32 value = *p_2;
*p_2 = value;

// Bind a reference to x:
let &u32 r = @x;
r = 10; // modifies x directly, no `*` needed
```

`@x` means the address of `x` and is used for both pointers
(`let *u32 p = @x`) and references (`let &u32 r = @x`); the expected type
decides which is produced. `@0xFFFFFFFF` means the address represented by the
literal, and is only valid where a pointer or reference type is expected.

A plain integer remains an integer. Converting a `usize` variable to a
pointer requires an explicit cast such as `address as *u32` (see
[Conversions](types.md#conversions)).

## Conditionals

```xe
if (condition) { ... }
else if (condition) { ... }
else { ... }
```

`unless` runs its block when the condition is false. It takes no `else`:

```xe
unless (condition) { ... }
```

`if` can also be used as an expression:

```xe
u8 y = 5 if (condition) else 10;
```

A simple statement (`return`, `break`, `continue`, an assignment, or an
expression statement) can end with a trailing `if` or `unless` condition:

```xe
return x if (done);
count++ unless (skip);
```

## Tuples and parallel assignment

A `let` can declare several variables at once from a tuple, and nest:

```xe
let i32 q, i32 r = divmod(17, 5);
let i32 a, (bool b, _) = 1, (true, 2);
let (i32 x, i32 y) = point;            // same as `let i32 x, i32 y = point;`
```

All values are evaluated first, left to right, and only then are the names
declared, so the initializers still see the variables the new names shadow.

Assigning to a list of places assigns **in parallel**: every value on the
right is evaluated first, left to right, and then stored into each target,
left to right. This makes swaps work:

```xe
a, b = b, a;
t.0, t.1 = t.1, t.0;
_, r = divmod(9, 4);        // `_` discards a value
```

Assigning the same place twice (`a, a = 1, 2`), or a tuple and one of its
elements (`t, t.0 = ..`), is an error. A list cannot be used with compound
assignment (`a, b += 1` is an error).

A bare comma list is allowed only after `return`, after the `=` of a `let`
or an assignment, and as an assignment target. Everywhere else a tuple
needs parentheses: `f((a, b))`. A one-element tuple always needs them:
`return (x,);` (`return x,;` is an error).

A trailing `if` or `unless` on a list guards the **whole statement**:
`return a, b if done;` returns the pair when `done`. A conditional
expression inside a list must therefore be parenthesized:
`return a, (b if c else d);`. Writing it without the parentheses is an
error, since it would read as a guard.

## Statement punctuation

- Examples consistently use semicolons for statements.
- Block forms use braces.

## Uncertain

- No finalized grammar document exists yet.
- Modifier syntax and declaration rules need a dedicated spec.
