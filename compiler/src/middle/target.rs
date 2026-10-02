//! Target properties the middle end needs, independent of any backend.
//!
//! The middle end must reason about target-dependent facts (for example the
//! range of `usize`) without linking against LLVM or any other backend. A
//! backend is responsible for constructing a [`TargetSpec`] that agrees with
//! the machine it generates code for.

use crate::types::{Type, signed_bounds, unsigned_bounds};
use num_bigint::BigInt;

/// Backend-agnostic description of the compilation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetSpec {
    pointer_width: u32,
}

impl TargetSpec {
    /// Describes a target whose pointers are `pointer_width` bits wide.
    pub const fn new(pointer_width: u32) -> Self {
        Self { pointer_width }
    }

    /// Describes the machine the compiler itself is running on.
    pub const fn host() -> Self {
        Self::new(usize::BITS)
    }

    /// Width in bits of pointers, `usize`, and `isize`.
    pub const fn pointer_width(&self) -> u32 {
        self.pointer_width
    }

    /// Bit width of an integer type on this target, `None` for non-integers.
    pub fn int_width(&self, ty: &Type) -> Option<u32> {
        match ty {
            Type::Int(width) | Type::UInt(width) => Some(*width),
            Type::USize | Type::ISize => Some(self.pointer_width),
            _ => None,
        }
    }

    /// Inclusive `(min, max)` range of an integer type on this target.
    ///
    /// Unlike [`Type::bounds`], this also resolves `usize` and `isize`.
    pub fn int_bounds(&self, ty: &Type) -> Option<(BigInt, BigInt)> {
        let width = self.int_width(ty)?;
        Some(if ty.is_signed_integer() {
            signed_bounds(width)
        } else {
            unsigned_bounds(width)
        })
    }
}
