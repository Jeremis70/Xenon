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

## Address and pointer expressions (draft)

Pointer declarations and address operations follow these forms:

```xe
// Pointer to a u32:
u32 x;
*u32 p = @x;

// Pointer to a fixed memory address:
*u32 p_2 = @0xFFFFFFFF;

// Pointer to pointer:
**u32 p_3 = @p_2;

// Read and write through a pointer:
u32 value = *p_2;
*p_2 = value;

// Bind a reference to x:
&u32 r = @x;
r = 10; // modifies x directly
```

`@x` means the address of `x` and is used for both pointers (`*u32 p = @x`)
and references (`&u32 r = @x`). `@0xFFFFFFFF` means the address represented
by the literal. A plain integer remains an integer; converting an integer
variable to a pointer requires an explicit cast such as `address as *u32`.

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
