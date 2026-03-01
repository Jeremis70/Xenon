# Traits

This page summarizes special trait-like protocols currently described in the draft documentation.

## Iterable

`Iterable` allows use in `for (each x in obj)` loops.

```xe
impl Iterable for u32 -> u1 {
    yield (x >> 0) & 1
    yield (x >> 1) & 1
}
```

## Indexable

`Indexable_get` and `Indexable_set` provide `obj[index]` read/write behavior.

```xe
impl Indexable_get for u32 -> u1 {
    x[i] => (x >> i) & 1
}

impl Indexable_set for u32 {
    x[i] = value => x = (x & ~(1 << i)) | ((value & 1) << i)
}
```

## Callable

`Callable` allows custom function-like objects.

```xe
struct Closure { ... }
impl Callable for Closure {
    call(args) => { ... }
}
```

## Rangeable

`Rangeable` is described as controlling range-expression behavior.

## Uncertain

- Syntax for these trait forms is explicitly marked as not final.
- Exact trait declaration grammar and method signatures are TBD.
