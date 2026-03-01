# Control Flow

This page documents loop and branch forms currently present in the language notes.

## Loops

```xe
while (condition) { ... }
for (init; condition; update) { ... }
do { ... } while (condition);
loop { ... }
```

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
