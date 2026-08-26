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

## Multiple return values (named returns)

Named returns declare return variables that exist as local variables inside the function body. Functions with named return variables **require an explicit `return`** (implicit returns are not permitted).

```xe
fn count_colored_sheep(SheepImage img) -> u32 white, u32 black, u32 other, u32 total {
    white = count(img, Color.White);
    black = count(img, Color.Black);
    other = count(img, Color.Other);
    total = white + black + other;
    return;
}
```

Caller-side destructuring:
```xe
u32 white, u32 black, u32 other, u32 total = count_colored_sheep(img);
```

## Callable model

See [Traits](traits.md). The design notes describe functions as regular objects that implement a callable trait.
