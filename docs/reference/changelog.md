# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Tuples: tuple types `(T1, T2)`, the unit type `()`, one-element tuples
  `(T,)`, tuple expressions, element access `t.0` (assignable on places),
  and element-wise `==` / `!=`.
- Multiple function results, named (`-> i32 q, i32 r`) or unnamed
  (`-> i32, bool`), returned as tuples. A bare `return;` returns the named
  results, or `()`.
- Destructuring declarations `let i32 a, (bool b, _) = ...;` and parallel
  assignment `a, b = b, a;`, which evaluates every value before storing.
- `_` as a discard pattern and assignment target.
- The return type may be omitted: `fn f() {}` returns `()`.
- Explicit casts `value as Type` between integers, floats, and pointers.
  A cast is exact or it panics: lossless casts such as `u8 as i32` cost
  nothing, any other cast checks at runtime that the target type represents
  the value exactly, and a constant that would fail is a compile error.
  Pointers convert to other pointers and to and from `usize`.

- Structs: `struct Rect { f64 width; f64 height; }`, built by naming every
  field in any order (`Rect(width = 3.0, height = 4.0)`), with assignable
  fields (`r.width`), field-wise `==` / `!=`, and copy semantics. A struct
  may point to itself through `*T`.
- `impl` blocks: methods taking `self` (a copy) or `&self` (a reference
  that can change the value), called as `r.scale(2.0)`, and associated
  functions called as `Rect.unit()`.
- Computed properties: `f64 area { return self.width * self.height; }` in
  an `impl` block is read as `r.area`, takes no room in the struct, and is
  read-only.

- The names of a `let` list can share one type, written on the first:
  `let i32 a, b = 0, 1;`. Either every name has its own type or only the
  first one does; mixing the two is an error.

### Changed

- `struct`, `impl`, and `self` are keywords.
- Any name that is not a built-in type is a type name: an unknown type is
  now reported by the type checker as "unknown type `foo`". `let x = 5;`
  reports "expected a type before `x`".
- Only `i` or `u` followed by digits is an integer type. Names such as
  `item`, `user`, and `u` no longer report a missing or invalid bit width.
- A float exponent needs digits after the point: `1.5e3` is a float, `1.e3`
  is not, so that `t.0.e3` reads the member `e3` of a tuple element.
- Assigning the same element twice in one parallel assignment is also
  rejected through a reference variable (`t.0, t.0 = 1, 2` with `&(i32,
  i32) t`); it was only detected on plain variables.
- Taking a reference `&T` to a variable requires the variable to be
  assigned already. Taking a pointer `*T` does not.

- The entry point returns `u8` (the exit code) or `()` (exit code 0) instead
  of `i32`, whose high bits POSIX systems silently drop. A C `int main`
  wrapper is now always emitted.
- Named results start unassigned and must be assigned on every path before
  they are read; they were previously zero-initialized.
- A bare `return;` with a shadowed named result is an error.
- Floats need digits before the point: `.5` is no longer a float, so that
  `t.5` is a tuple index.
- `_` is a reserved token rather than an identifier.
- A conditional expression inside a bare list must be parenthesized:
  `return a, (b if c else d);`.