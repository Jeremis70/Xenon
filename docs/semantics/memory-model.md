# Memory Model

This page describes the current documented state of Xenon memory semantics.

## Current guarantees

- No stable ownership or borrowing model is finalized yet.
- No lifetime system is specified yet.
- No aliasing rules are specified yet.
- No stable object/value representation guarantees are documented yet.

## Pointer syntax

Xenon distinguishes pointer values from integer values. A pointer type is
written `*T`, and additional `*` prefixes represent multiple indirection
levels. `@x` takes the address of an addressable location, while
`@integer_literal` creates a pointer from a literal machine address.

For example:

```xe
let u32 x = 0;
let *u32 p = @x;
let *u32 device_register = @0xFFFFFFFF;
```

The `@` operator does not make an address valid. The address may be unmapped,
misaligned, outside the target's address space, or otherwise unsuitable for
access. Pointer dereference and the resulting memory access therefore remain
subject to the language's eventual validity, alignment, lifetime, and aliasing
rules.

`@name` means the address of the variable `name`, even when `name` has an
integer type. Converting an integer value into a pointer is a separate,
explicit operation, which is **not implemented yet**:

```xe
let usize address = 0xFFFFFFFF;
let *u32 p = address as *u32; // planned
```

## References

References use `&T` as their type. The `@` operator creates a reference the
same way it creates a pointer — the expected type at the use site determines
whether a pointer or reference is produced.

```xe
let u32 x = 42;
let &u32 r = @x;
r = 10; // modifies x directly
```

References are transparent: reads load through to the referent and assignment
writes through to it, so no explicit `*` is used. A consequence is that a
reference is bound once, at its declaration, and cannot later be rebound to a
different location.

When passing a variable to a function that expects a reference, use `@`:

```xe
fn increment(&u32 value) -> u32 {
    value = value + 1;
    return value;
}

let u32 n = 5;
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

The compiler lowers pointers and references to opaque LLVM pointers in the
default address space. What is implemented today:

- `*T` and `&T` types in bindings, parameters, and return types,
- `@place` (address-of) and `@literal` (address literal, range-checked against
  the target pointer width),
- `*p` dereference, both as a value and as an assignment target,
- reference auto-deref on read and on assignment,
- `==` / `!=` between two pointers of the same type.

What is **not** implemented and must not be relied upon:

- lifetime, aliasing, exclusivity, or validity checking of any kind,
- `&mut T`,
- pointer arithmetic, indexing, or `null`,
- `as` casts between integers and pointers.

Nothing in the list above introduces a safety guarantee: `@` produces an
address, and the compiler does not verify that the address is still valid when
it is used.

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
