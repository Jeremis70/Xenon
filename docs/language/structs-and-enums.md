# Structs and Enums

Structs are implemented. Enums are not designed yet.

## Structs

A struct is a named type made of named fields:

```xe
struct Rect {
    f64 width;
    f64 height;
}
```

- The body lists only fields, each written `<type> <name>;`. The fields are
  stored in the order they are declared, and nothing else takes room in the
  struct.
- A struct may have no fields: `struct Empty {}`.
- Two structs are different types even when their fields are the same.
- A struct may be used before it is declared: the order of declarations in
  a program does not matter.
- A struct cannot share its name with a function or a built-in type, and
  cannot be named `_xe` or `llvm`, which the compiler uses itself.

### Building a struct

A struct is built by naming every field, in any order:

```xe
let Rect r = Rect(width = 3.0, height = 4.0);
let Rect s = Rect(height = 1.0, width = 2.0);
let Empty e = Empty();
```

Every field must be given exactly once and by name: `Rect(3.0, 4.0)` and
`Rect(width = 3.0)` are errors. The values are computed in the order they
are written, whatever the order of the fields. A whole struct is always
built in one step: a variable cannot be filled in one field at a time.

### Fields

```xe
let f64 w = r.width;
r.height = 2.0;         // a field of a variable is assignable
r.width += 1.0;
line.from.x = 5;        // also when nested
```

- A field of a `&Rect` is reached the same way; the reference is followed
  automatically.
- A `*Rect` must be dereferenced explicitly: `(*p).width`.
- A field cannot have a reference type `&T`, directly or inside a tuple. A
  pointer `*T` is fine.
- A struct cannot contain itself, directly or through other structs and
  tuples. It can point to itself:

```xe
struct Node {
    i32 value;
    *Node next;
}
```

### Copying and comparing

Structs are values: assigning one, passing it to a function, and returning
it copy it.

Structs compare with `==` and `!=`, field by field, and only with a struct
of the same type. Ordering (`<`, ...) and casts (`as`) are not defined on
structs.

## Methods

The functions of a struct are declared in `impl` blocks. A struct may have
several:

```xe
impl Rect {
    fn scaled(self, f64 k) -> Rect {
        return Rect(width = self.width * k, height = self.height * k);
    }

    fn scale(&self, f64 k) {
        self.width *= k;
        self.height *= k;
    }

    fn unit() -> Rect {
        return Rect(width = 1.0, height = 1.0);
    }
}
```

The first parameter decides how the function is used:

| First parameter | Meaning | Call |
|---|---|---|
| `self` | A method that gets a **copy** of the value. Changing `self` does not change the caller's value. | `r.scaled(2.0)` |
| `&self` | A method that gets a **reference** to the value and can change it. | `r.scale(2.0)` |
| neither | An associated function, called on the struct itself. | `Rect.unit()` |

- `r.scale(2.0)` passes `@r` for you. The variable must already hold a
  value.
- A method can be called on any value of the struct type, not only on a
  variable: `Rect.unit().scaled(2.0)`. A `&self` method called on such a
  value changes a temporary.
- Arguments are computed before the call: in `r.scale(r.width)` the method
  receives the width from before it runs.
- A method is called through a pointer after dereferencing it:
  `(*p).scale(2.0)`.
- `self` is a keyword. It is only available in a method or a property, and
  cannot name a variable.
- A method and a free function may have the same name; they never collide.

## Computed properties

A property is a member that is read like a field but computed on every
read. It takes no room in the struct. It is declared in an `impl` block,
type first, like a field with a body:

```xe
impl Rect {
    f64 area {
        return self.width * self.height;
    }
}

let f64 a = r.area;
```

- The body receives `self` by reference and must return a value of the
  property's type on every path.
- A property is read without parentheses: `r.area()` is an error.
- A property is read-only: `r.area = 1.0`, `r.area += 1.0`, and `@r.area`
  are errors, and so is assigning to a field of what a property returns.
- A property can read other properties.

Fields, methods, associated functions, and properties of a struct share one
namespace: two members cannot have the same name, even in different `impl`
blocks.

## Not implemented yet

- Properties with accessors (`get`, `set`, and observers).
- User-defined constructors. They are meant to take over the
  `Rect(...)` spelling.
- Traits and `impl Trait for Rect`.
- `Self`, visibility, struct patterns in `let`, and generics.
- Named arguments for ordinary functions.

## Open questions

- Are enums sum types, tagged unions, or both?
- Is memory layout explicit or implementation-defined? Today the fields are
  laid out in declaration order with the platform's natural alignment.
