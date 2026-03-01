# Syntax Basics

This page covers basic declarations and expression forms that are currently documented.

## Variables

Draft declaration shape:

```xe
<modifiers> <type> <name> = <initial value>;
```

Initial value is optional and is expected to default to zero or null.

```xe
u64 x = 42;
```

## Conditionals

```xe
if (condition) { ... }
else if (condition) { ... }
else { ... }
```

`if` can also be used as an expression:

```xe
u8 x = if (condition) 5 else 10;
u8 y = 5 if (condition) else 10;
```

## Statement punctuation

- Examples consistently use semicolons for statements.
- Block forms use braces.

## Uncertain

- No finalized grammar document exists yet.
- Modifier syntax and declaration rules need a dedicated spec.
