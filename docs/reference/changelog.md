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

### Changed

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