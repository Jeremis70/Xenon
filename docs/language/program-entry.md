# Program Entry

The entry point of an executable is the function marked `#[entry]`. It takes
no parameters and returns either nothing or a `u8` exit code:

```xe
#[entry]
fn main() {          // exits with 0
}
```

```xe
#[entry]
fn start() -> u8 {   // exits with the returned code
    return 3;
}
```

- Omitting the return type means `()`, exactly as for any other function.
  Falling off the end, `return;`, and `return ();` all exit with 0.
- The exit code is a `u8` because POSIX systems only report the low 8 bits
  of a process's exit status. A wider type such as `i32` would be truncated
  silently (`346` would be seen as `90`), so it is rejected:

  ```text
  entry function must return `u8` or `()`, found `i32`
  ```

- By shell convention, `0` means success. Codes 126 and above are best
  avoided: shells use 126 and 127 for "cannot execute" and "not found", and
  `128 + N` for a process killed by signal `N`.
- The entry may have any name. The compiler emits a C `int main` wrapper that
  calls it and converts the result: the `u8` is zero-extended, and `()`
  becomes `0`. A Xenon function literally named `main` is renamed so it does
  not clash with the C symbol.

## Planned

The `u8` exit code is a stopgap. A cleaner model, such as a dedicated exit
code type or an error-returning entry that maps failures to an exit status,
is planned once error handling exists.
