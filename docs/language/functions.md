# Functions

This page captures the documented function model, overloading rules, and return conventions.

## Function declarations

```xe
fn foo(u32 x) -> u32 { ... }
```

Function nesting is allowed since they should be regular objects that implement a callable trait. 

## Overloading

Overloading is documented by parameter types/count and return type:

```xe
fn foo(u32 x) -> u32 { ... }
fn foo(u64 x) -> u64 { ... }
fn foo(u32 x, u32 y) -> u32 { ... }
fn foo(u32 x) -> u64 { ... }
```

If overload resolution is ambiguous, the compiler should emit an error and require explicit typing.

## Default and named arguments

```xe
fn foo(u32 x, u32 y = 10) -> u32 { ... }

u32 out = foo(5);
u32 out = foo(5, 20);
u32 out = foo(y = 12, x = 5);
```

## Return types and `()`

A function without `->` returns `()`, the unit type. `fn f() { .. }` and
`fn f() -> () { .. }` are the same signature. In such a function `return;`
and `return ();` are equivalent, and falling off the end of the body returns
`()`. Every other function needs an explicit `return` on every path.

## Multiple results

A function can return several values. Its return type is the tuple of
their types:

```xe
fn min_max(i32 a, i32 b) -> i32, i32 {   // returns (i32, i32)
    return a, b if a < b;
    return b, a;
}
```

A single result stays scalar: `-> i32` and `-> i32 r` return `i32`, not
`(i32,)`.

### Named results

Results can be named. Named results are local variables of the function, and
a bare `return;` returns their current values:

```xe
fn divmod(i32 n, i32 d) -> i32 q, i32 r {
    q = n / d;
    r = n % d;
    return;            // returns (q, r)
}
```

- Either every result is named or none is.
- Named results start **unassigned**. Each must be assigned on every path
  that reaches a bare `return;` (or reads it); otherwise compilation fails
  with "named result `r` is not assigned on every path".
- A bare `return;` is an error while a named result is shadowed by a
  variable of the same name in an inner scope, since it would silently
  return the outer variable.
- `return e;` still returns `e`, ignoring the named results.
- A named result cannot have a reference type, since a reference must be
  bound at its declaration.

### Taking results apart

The caller usually destructures the results immediately, spelling each
type, or assigns them to existing variables:

```xe
let i32 q, i32 r = divmod(17, 5);   // new variables
q, r = divmod(9, 4);                // existing variables
let _, i32 rest = divmod(9, 4);     // ignore the quotient
let (i32, i32) both = divmod(9, 4); // or keep the tuple
i32 three = divmod(17, 5).0;
```

## Callable model

See [Traits](traits.md). The design notes describe functions as regular objects that implement a callable trait.
