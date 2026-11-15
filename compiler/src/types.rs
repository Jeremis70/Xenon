//! Semantic types shared by every compiler stage.
//!
//! The AST spells types with its own syntax (`ast::TypeExpr`), which the
//! type checker resolves to this representation. The type itself is a
//! semantic concept: the type checker, MIR, and backends all consume it. It
//! therefore lives below the frontend in the dependency graph so that the
//! middle end never has to depend on syntax modules.

use crate::error::TypeError;
use crate::index::{IndexVec, newtype_index};
use crate::source::Span;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

/// Largest supported integer width, matching LLVM's `IntegerType::MAX_INT_BITS`.
pub const MAX_INT_WIDTH: u32 = 1 << 23;

/// A fully resolved Xenon type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Signed integer with a bit width from 0 through [`MAX_INT_WIDTH`].
    Int(u32),
    /// Unsigned integer with a bit width from 0 through [`MAX_INT_WIDTH`].
    UInt(u32),

    /// Pointer-sized unsigned integer; its width depends on the target.
    USize,
    /// Pointer-sized signed integer; its width depends on the target.
    ISize,

    Float16,
    BFloat16,
    Float32,
    Float64,
    Float128,

    Bool,

    /// Opaque pointer `*T`: reads and writes require an explicit dereference.
    Pointer(Box<Type>),
    /// Transparent reference `&T`: the source language auto-dereferences it.
    Reference(Box<Type>),

    /// Anonymous positional product `(T1, T2, ..)`. The empty tuple `()` is
    /// the unit type, the return type of functions that return nothing.
    Tuple(Vec<Type>),
    /// A fixed-size homogeneous array, written `[N]T`.
    Array(Box<Type>, u32),

    /// A user-defined `struct`. Its fields live in the program's
    /// [`StructTable`].
    Struct(StructTy),
}

newtype_index! {
    /// Identifies a struct definition within a compiled program.
    pub struct StructId = "struct{}";
}

/// A reference to a struct definition.
///
/// Two struct types are the same type exactly when they have the same
/// [`StructId`]; the name only labels the type in diagnostics and dumps,
/// which therefore need no [`StructTable`] to print a type.
#[derive(Debug, Clone)]
pub struct StructTy {
    pub id: StructId,
    pub name: Arc<str>,
}

impl PartialEq for StructTy {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for StructTy {}

impl std::hash::Hash for StructTy {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// One field of a struct.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Type,
    /// Where the field is declared.
    pub span: Span,
}

/// The definition of a struct: its fields, in declaration order, which is
/// also their layout order.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: Arc<str>,
    pub fields: Vec<FieldDef>,
    /// Where the struct is declared.
    pub span: Span,
}

/// Every struct definition of a program, indexed by [`StructId`].
///
/// A [`Type::Struct`] only names its definition, so whatever depends on the
/// fields of a struct (its layout, the type of a field, whether it has any
/// bits) is answered here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructTable {
    defs: IndexVec<StructId, StructDef>,
}

impl StructTable {
    /// Adds a definition and returns its identifier.
    pub fn push(&mut self, def: StructDef) -> StructId {
        self.defs.push(def)
    }

    /// The definition of `id`, if it exists.
    pub fn get(&self, id: StructId) -> Option<&StructDef> {
        self.defs.get(id)
    }

    /// Mutable access to the definition of `id`, if it exists.
    pub fn get_mut(&mut self, id: StructId) -> Option<&mut StructDef> {
        self.defs.get_mut(id)
    }

    /// Every definition with its identifier, in declaration order.
    pub fn iter_enumerated(&self) -> impl DoubleEndedIterator<Item = (StructId, &StructDef)> + '_ {
        self.defs.iter_enumerated()
    }

    /// The type of the struct `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is not defined in this table.
    pub fn ty(&self, id: StructId) -> Type {
        Type::Struct(StructTy {
            id,
            name: self.defs[id].name.clone(),
        })
    }

    /// The type of element `index` of a tuple or struct `ty`; `None` for
    /// other types, unknown structs, and indices out of range.
    pub fn field_ty<'a>(&'a self, ty: &'a Type, index: u32) -> Option<&'a Type> {
        match ty {
            Type::Tuple(elems) => elems.get(index as usize),
            Type::Struct(struct_ty) => {
                let field = self.get(struct_ty.id)?.fields.get(index as usize)?;
                Some(&field.ty)
            }
            _ => None,
        }
    }

    /// The element types of a tuple or struct `ty`, in order; `None` for
    /// other types and unknown structs.
    pub fn field_tys(&self, ty: &Type) -> Option<Vec<Type>> {
        match ty {
            Type::Tuple(elems) => Some(elems.clone()),
            Type::Struct(struct_ty) => {
                let def = self.get(struct_ty.id)?;
                Some(def.fields.iter().map(|field| field.ty.clone()).collect())
            }
            _ => None,
        }
    }
}

impl Type {
    /// Returns `(min, max)` bounds for fixed-width integer types.
    ///
    /// Returns `None` for non-integer types and for `usize`/`isize`, whose
    /// width depends on the target (see `middle::target::TargetSpec`).
    pub fn bounds(&self) -> Option<(BigInt, BigInt)> {
        match self {
            Type::UInt(n) => Some(unsigned_bounds(*n)),
            Type::Int(n) => Some(signed_bounds(*n)),
            _ => None,
        }
    }

    /// Returns the pointee type for `*T` and `&T`, `None` otherwise.
    pub fn pointee(&self) -> Option<&Type> {
        match self {
            Type::Pointer(inner) | Type::Reference(inner) => Some(inner),
            _ => None,
        }
    }

    /// Returns `true` for pointer and reference types.
    pub fn is_indirect(&self) -> bool {
        matches!(self, Type::Pointer(_) | Type::Reference(_))
    }

    /// Returns `true` for every integer type, including `usize`/`isize`.
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::Int(_) | Type::UInt(_) | Type::USize | Type::ISize
        )
    }

    /// Returns `true` for types with a single value and no bits: `i0` and
    /// `u0`, which only hold `0`, and aggregates of such types,
    /// including `()`, structs without fields, and empty arrays.
    pub fn is_zero_sized(&self, structs: &StructTable) -> bool {
        self.is_zero_sized_with(structs, &mut HashMap::new())
    }

    /// [`Self::is_zero_sized`], deciding each struct once: `known` holds
    /// the answers so far. A struct used for many fields is one definition,
    /// however large the tree of fields it spans.
    fn is_zero_sized_with(
        &self,
        structs: &StructTable,
        known: &mut HashMap<StructId, bool>,
    ) -> bool {
        match self {
            Type::Int(0) | Type::UInt(0) => true,
            Type::Array(elem, len) => *len == 0 || elem.is_zero_sized_with(structs, known),
            Type::Tuple(elems) => elems
                .iter()
                .all(|elem| elem.is_zero_sized_with(structs, known)),
            Type::Struct(struct_ty) => {
                if let Some(&zero_sized) = known.get(&struct_ty.id) {
                    return zero_sized;
                }
                let zero_sized = structs.get(struct_ty.id).is_some_and(|def| {
                    def.fields
                        .iter()
                        .all(|field| field.ty.is_zero_sized_with(structs, known))
                });
                known.insert(struct_ty.id, zero_sized);
                zero_sized
            }
            _ => false,
        }
    }

    /// Returns `true` for tuples, structs, and arrays, the types made of elements.
    pub fn is_aggregate(&self) -> bool {
        matches!(self, Type::Tuple(_) | Type::Struct(_) | Type::Array(..))
    }

    /// Returns `true` for a reference type or a tuple or array containing one. A
    /// struct never contains one: its fields cannot have reference types.
    pub fn contains_reference(&self) -> bool {
        match self {
            Type::Reference(_) => true,
            Type::Array(elem, _) => elem.contains_reference(),
            Type::Tuple(elems) => elems.iter().any(Type::contains_reference),
            _ => false,
        }
    }

    /// The unit type `()`.
    pub fn unit() -> Self {
        Type::Tuple(Vec::new())
    }

    /// Returns `true` for the unit type `()`.
    pub fn is_unit(&self) -> bool {
        matches!(self, Type::Tuple(elems) if elems.is_empty())
    }

    /// The element types of a tuple, `None` for other types.
    pub fn tuple_elems(&self) -> Option<&[Type]> {
        match self {
            Type::Tuple(elems) => Some(elems),
            _ => None,
        }
    }

    /// Returns `true` for signed integer types (`iN`, `isize`).
    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Type::Int(_) | Type::ISize)
    }

    /// Returns `true` for unsigned integer types (`uN`, `usize`).
    pub fn is_unsigned_integer(&self) -> bool {
        matches!(self, Type::UInt(_) | Type::USize)
    }

    /// Returns `true` for every floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(
            self,
            Type::Float16 | Type::BFloat16 | Type::Float32 | Type::Float64 | Type::Float128
        )
    }

    /// Returns `true` for `bool`.
    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Bool)
    }

    /// Bit width of a floating-point type, `None` for non-floats.
    ///
    /// Float widths are target-independent, unlike `usize`/`isize`.
    pub fn float_width(&self) -> Option<u32> {
        match self {
            Type::Float16 | Type::BFloat16 => Some(16),
            Type::Float32 => Some(32),
            Type::Float64 => Some(64),
            Type::Float128 => Some(128),
            _ => None,
        }
    }

    /// Significand precision of a floating-point type in bits, counting the
    /// implicit leading bit; `None` for non-floats. Every integer of at most
    /// this many bits is exactly representable.
    pub fn float_precision(&self) -> Option<u32> {
        match self {
            Type::Float16 => Some(11),
            Type::BFloat16 => Some(8),
            Type::Float32 => Some(24),
            Type::Float64 => Some(53),
            Type::Float128 => Some(113),
            _ => None,
        }
    }

    /// Largest finite exponent of a floating-point type: every finite value
    /// is below `2^(max_exponent + 1)`. `None` for non-floats.
    pub fn float_max_exponent(&self) -> Option<i32> {
        match self {
            Type::Float16 => Some(15),
            Type::BFloat16 | Type::Float32 => Some(127),
            Type::Float64 => Some(1023),
            Type::Float128 => Some(16383),
            _ => None,
        }
    }
}

/// Inclusive value range of an unsigned integer of `width` bits.
pub(crate) fn unsigned_bounds(width: u32) -> (BigInt, BigInt) {
    (BigInt::ZERO, (BigInt::from(1) << width) - 1)
}

/// Inclusive value range of a two's-complement integer of `width` bits.
/// A zero-width integer has no sign bit and holds only `0`.
pub(crate) fn signed_bounds(width: u32) -> (BigInt, BigInt) {
    let Some(magnitude_width) = width.checked_sub(1) else {
        return (BigInt::ZERO, BigInt::ZERO);
    };
    let half = BigInt::from(1) << magnitude_width;
    (-half.clone(), half - 1)
}

impl FromStr for Type {
    type Err = TypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "usize" => return Ok(Type::USize),
            "isize" => return Ok(Type::ISize),
            "bool" => return Ok(Type::Bool),
            "f16" => return Ok(Type::Float16),
            "bf16" => return Ok(Type::BFloat16),
            "f32" => return Ok(Type::Float32),
            "f64" => return Ok(Type::Float64),
            "f128" => return Ok(Type::Float128),
            _ => {}
        }

        // Parameterised integer types: (i|u)<width>. Any other name, such
        // as `item` or `u`, is no built-in type.
        let (signed, digits) = if let Some(rest) = s.strip_prefix('i') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('u') {
            (false, rest)
        } else {
            return Err(TypeError::Unknown(s.to_owned()));
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(TypeError::Unknown(s.to_owned()));
        }

        let width = digits
            .parse::<u32>()
            .map_err(|_| TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width must be a non-negative integer",
            })?;

        if width > MAX_INT_WIDTH {
            return Err(TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width exceeds the supported maximum of 8388608",
            });
        }

        Ok(if signed {
            Type::Int(width)
        } else {
            Type::UInt(width)
        })
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int(w) => write!(f, "i{w}"),
            Type::UInt(w) => write!(f, "u{w}"),
            Type::USize => write!(f, "usize"),
            Type::ISize => write!(f, "isize"),
            Type::Float16 => write!(f, "f16"),
            Type::BFloat16 => write!(f, "bf16"),
            Type::Float32 => write!(f, "f32"),
            Type::Float64 => write!(f, "f64"),
            Type::Float128 => write!(f, "f128"),
            Type::Bool => write!(f, "bool"),
            Type::Pointer(inner) => write!(f, "*{inner}"),
            Type::Reference(inner) => write!(f, "&{inner}"),
            Type::Array(elem, len) => write!(f, "[{len}]{elem}"),
            Type::Tuple(elems) => {
                f.write_str("(")?;
                for (index, elem) in elems.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{elem}")?;
                }
                // A one-tuple keeps its comma: `(i32,)` is not `(i32)`.
                if elems.len() == 1 {
                    f.write_str(",")?;
                }
                f.write_str(")")
            }
            Type::Struct(struct_ty) => f.write_str(&struct_ty.name),
        }
    }
}
