# Ranges and Slices

This page documents draft range and slice behavior.

## Range

A range object is described with:

- `start` (inclusive)
- `end` (inclusive)
- `step`

```xe
from 0 to 10 step 1;
```

If omitted, `step` is inferred as `1` or `-1` depending on direction.

Range iteration example:

```xe
for (each i from 0 to 10) { ... }
```

## Slice

A slice is a view over contiguous elements with:

- `pointer`
- `length`
- `stride`

Examples:

```xe
array[0 to 10] = 3;
Slice s = array[0 to 10];
array[0 to 10] = array[10 to 0];
array[0 to 10] = array2[0 to 10];
```

Assignment requires equal lengths, but stride may differ.

## Function compatibility

Draft notes say functions that accept arrays should also accept slices.

```xe
any(x == 3 for x in array);
any(x == 3 for x in s);
```
