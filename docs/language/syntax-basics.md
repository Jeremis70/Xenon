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

A plain integer remains an integer. Converting an integer *variable* to a
pointer requires an explicit cast such as `address as *u32`, which is planned
but not implemented yet.

## Conditionals

```xe
if (condition) { ... }
else if (condition) { ... }
else { ... }
```

`if` can also be used as an expression:

```xe
u8 y = 5 if (condition) else 10;
```

## Statement punctuation

- Examples consistently use semicolons for statements.
- Block forms use braces.

## Uncertain

- No finalized grammar document exists yet.
- Modifier syntax and declaration rules need a dedicated spec.
