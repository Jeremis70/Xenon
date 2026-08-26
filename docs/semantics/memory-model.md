# Memory Model

This page describes the current documented state of Xenon memory semantics.

## Current guarantees

- No stable ownership or borrowing model is finalized yet.
- No lifetime system is specified yet.
- No aliasing rules are specified yet.
- No stable object/value representation guarantees are documented yet.

## Pointer syntax (draft)

Xenon distinguishes pointer values from integer values. A pointer type is
written `*T`, and additional `*` prefixes represent multiple indirection
levels. `@x` takes the address of an addressable location, while
`@integer_literal` creates a pointer from a literal machine address.

For example:

```xe
u32 x;
*u32 p = @x;
*u32 device_register = @0xFFFFFFFF;
```

The `@` operator does not make an address valid. The address may be unmapped,
misaligned, outside the target's address space, or otherwise unsuitable for
access. Pointer dereference and the resulting memory access therefore remain
subject to the language's eventual validity, alignment, lifetime, and aliasing
rules.

`@name` means the address of the variable `name`, even when `name` has an
integer type. Converting an integer value into a pointer is a separate,
explicit operation:

```xe
usize address = 0xFFFFFFFF;
*u32 p = address as *u32;
```

## References (draft)

References use `&T` as their type. The `@` operator creates a reference, the
same way it creates a pointer — the target type determines whether a pointer
or reference is produced.

```xe
u32 x = 42;
&u32 r = @x;
r = 10; // modifies x directly
```

When passing a variable to a function that expects a reference, use `@`:

```xe
fn increment(&u32 value) {
    value = value + 1;
}

u32 n = 5;
increment(@n); // n is now 6
```

References are non-owning and are intended to provide borrowed access rather
than allocation or destruction. They do not replace the ownership of the
referred-to value. Mutable references are planned as `&mut T`.

The intended model is similar to Rust: references should not outlive the
values they refer to, and mutable access should be exclusive. However, Xenon
does not currently implement or specify a borrow checker, lifetime inference,
or complete aliasing model. These guarantees are therefore design goals, not
current compiler guarantees.

## Current compiler behavior

The compiler is currently pre-alpha and focused on early frontend/pipeline behavior. Existing commands primarily parse inputs and expose intermediate/compiler-session data.

Because of this, memory behavior should be treated as unspecified unless and until this page defines it explicitly.

## Guidance for users (pre-alpha)

- Do not rely on any implicit ownership or borrowing behavior.
- Do not assume pointer/reference safety guarantees.
- Treat examples in other pages as syntax exploration unless this page marks semantics as stable.

## Planned scope for this page

Future revisions should define:

- ownership model and move/copy behavior,
- reference and mutability rules,
- lifetime semantics,
- aliasing guarantees and undefined behavior boundaries,
- value vs object identity semantics.
