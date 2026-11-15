# Control Flow

This page documents loop and branch forms currently present in the language notes.

## Loops

```xe
while (condition) { ... }
do { ... } while (condition);
loop { ... }
for x in items { ... }
repeat 10 { ... }
```

There is no C-style `for (init; condition; update)`: a `while` loop covers it.

## `for`

A `for` loop runs its body once per element of an array or of an integer
range (see [Ranges and Slices](ranges-and-slices.md)).

```xe
for x in bytes { ... }            // x has the element type
for i in 0 until n { ... }        // i has the type of n
for u8 i in 0 to 255 { ... }      // both bounds are literals: write the type
for _ in 0 until 3 { ... }        // no loop variable
```

The type of the loop variable may be left out. It is the element type of
the array, or the type of whichever range bound has one. When both bounds
are bare literals nothing gives the range a type, so it must be written. A
written type is checked against the elements.

An array is walked in place, exactly like the index loop one would write by
hand: each element is copied into the loop variable when its iteration
starts, so a write to the array is seen by the iterations after it. Which
array is walked, and the bounds of a range, are evaluated once, before the
loop.

`break` and `continue` work as in `while`; `continue` moves on to the next
element. The loop variable cannot be assigned.

### `with` and `where`

Any number of `with` and `where` clauses may follow, in any order.

```xe
for number in numbers with index where number.is_prime() with nth {
    // runs for every prime; `index` is its position in `numbers`,
    // `nth` counts the primes found so far
}
```

- `with name` counts the elements that reach the clause, from 0. Before any
  `where` that is the index of the element; after one it numbers the
  elements that passed. The counter is a `usize` unless a type is written
  (`with u8 i`). Counting more elements than its type holds panics.
- `where condition` skips the element when the condition is false.

A clause sees the loop variable and the `with` names before it, not those
after. Counters cannot be assigned.

## `repeat`

`repeat count { ... }` runs its body `count` times, with no loop variable.
The count is any integer, evaluated once. A negative literal is an error; a
negative value at runtime runs the body zero times.

```xe
repeat 3 { beep(); }
```

`for` and `repeat` cost nothing over the `while` loop they stand for.

## Break and continue

```xe
break;
break label;
break value;
break label value;

continue;
continue label;
```

## Loop labels

```xe
label: while (condition) {
    // ...
    break label; // breaks out of the labeled loop
    continue label; // continues the next iteration of the labeled loop
}
```

## Loop values

Loops may produce values through `break`. 

```xe
u8 x = while (condition) {
    if (condition) { break 5; }
} break 0; // If the loop exits without hitting a `break` with a value, this is the result.
```

## Loop `else`

```xe
while (condition) {
    // body
} else {
    // runs only if the loop never executed
}
```

## Loop with everything at once:

```xe
outer: u8 outer_value = while (condition) {
    // body
    inner: u8 inner_value = while (condition) {
        // body
        if (condition) { continue inner; }
        if (condition) { break inner 5; }
    } else {
        // runs only if the inner loop never executed
    }
} else {
    // runs only if the loop never executed
} break 0;
```     

## Conditional expressions

See [Syntax Basics](syntax-basics.md) for `if` statement and expression forms.
