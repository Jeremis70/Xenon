# Ranges and Slices

Ranges are implemented inside `for` loops. Slices are still a draft.

## Range

A range counts through integers. It is written in the header of a `for`
loop (see [Control Flow](control-flow.md)); it is not a value yet, so it
cannot be stored or passed around.

```xe
for i32 i in 0 to 10 { ... }      // 0, 1, .. 10: `to` includes the end
for i32 i in 0 until 10 { ... }   // 0, 1, .. 9: `until` stops before it
for i in 0 until n by 4 { ... }   // 0, 4, 8, ..
for i in a to b by -1 { ... }     // counts down
```

Both bounds have the same integer type and are evaluated once, the start
first. `by` takes a non-zero integer literal that fits that type.

### Direction

The direction of a range is always known at compile time, so a loop never
pays to find it out:

- With `by`, the sign of the step is the direction.
- Without `by`, when both bounds are integer literals, the range runs from
  the first to the second: `for u8 i in 10 to 0` counts down.
- Otherwise the range counts up by one. A start past the end is an empty
  range: `for i in 0 until n` does nothing when `n` is zero or negative.
  Write `by -1` to count down between two runtime bounds.

A range never leaves its type: `for u8 i in 0 to 255` visits every `u8`
without overflowing, and a step stops at the last value it can reach
(`250 to 255 by 3` is 250, 253).

## Slice

Draft. A slice is a view over contiguous elements with:

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
