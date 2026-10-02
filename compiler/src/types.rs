//! Semantic types shared by every compiler stage.
//!
//! The AST currently spells types with this same representation, but the
//! type itself is a semantic concept: the type checker, MIR, and backends
//! all consume it. It therefore lives below the frontend in the dependency
//! graph so that the middle end never has to depend on syntax modules.

use crate::error::TypeError;
use num_bigint::BigInt;
use std::str::FromStr;

/// A fully resolved Xenon type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Signed integer of any non-zero bit width.
    Int(u32),
    /// Unsigned integer of any non-zero bit width.
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
}

/// Inclusive value range of an unsigned integer of `width` bits.
pub(crate) fn unsigned_bounds(width: u32) -> (BigInt, BigInt) {
    (BigInt::ZERO, (BigInt::from(1) << width) - 1)
}

/// Inclusive value range of a two's-complement integer of `width` bits.
///
/// `width` must be non-zero; the type parser rejects zero-width integers.
pub(crate) fn signed_bounds(width: u32) -> (BigInt, BigInt) {
    let half = BigInt::from(1) << width.saturating_sub(1);
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

        // Parameterised integer types: (i|u)<width>
        let (signed, digits) = if let Some(rest) = s.strip_prefix('i') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('u') {
            (false, rest)
        } else {
            return Err(TypeError::Unknown(s.to_owned()));
        };

        if digits.is_empty() {
            return Err(TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "missing bit width",
            });
        }

        let width = digits
            .parse::<u32>()
            .map_err(|_| TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width must be a positive integer",
            })?;

        if width == 0 {
            return Err(TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width must be non-zero",
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
        }
    }
}
