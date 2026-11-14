//! The rules of explicit conversions (`x as T`).
//!
//! A cast is *exact or it traps*: it never changes a value silently. A
//! conversion between two types is either lossless, so every source value
//! survives it and no check is needed, or checked at runtime, where a value
//! the target type cannot represent exactly fails the cast:
//!
//! - integer to integer: the value must be within the target's bounds;
//! - integer to float: the float must hold the value exactly;
//! - float to integer: the value must be finite, integral, and in range;
//! - float to float: narrowing must not round (NaN stays NaN);
//! - pointers and `usize`: the bits are copied, nothing can be lost.
//!
//! The type checker, the runtime-check pass, and the constant interpreter
//! share these rules, so they always agree.

use crate::middle::ops::CastKind;
use crate::middle::target::TargetSpec;
use crate::types::Type;

/// The conversion `from as to` performs, `None` if no cast converts `from`
/// to `to`.
///
/// Integers and floats convert into each other; pointers convert into
/// pointers and to and from `usize`. References, `bool`, and tuples have no
/// casts.
pub fn cast_kind(from: &Type, to: &Type) -> Option<CastKind> {
    let pointer = |ty: &Type| matches!(ty, Type::Pointer(_));
    Some(match (from, to) {
        _ if from.is_integer() && to.is_integer() => CastKind::IntToInt,
        _ if from.is_integer() && to.is_float() => CastKind::IntToFloat,
        _ if from.is_float() && to.is_integer() => CastKind::FloatToInt,
        _ if from.is_float() && to.is_float() => CastKind::FloatToFloat,
        _ if pointer(from) && pointer(to) => CastKind::PtrToPtr,
        (_, Type::USize) if pointer(from) => CastKind::PtrToInt,
        (Type::USize, _) if pointer(to) => CastKind::IntToPtr,
        _ => return None,
    })
}

/// Returns `true` if every value of `from` survives the conversion `kind`
/// to `to` on `target`, so the cast needs no runtime check.
///
/// Returns `false` when some value could be lost, including when the target
/// cannot tell (an integer width it does not resolve).
pub fn is_lossless(kind: CastKind, from: &Type, to: &Type, target: &TargetSpec) -> bool {
    match kind {
        CastKind::IntToInt => match (target.int_bounds(from), target.int_bounds(to)) {
            (Some((from_min, from_max)), Some((to_min, to_max))) => {
                to_min <= from_min && from_max <= to_max
            }
            _ => false,
        },
        CastKind::IntToFloat => {
            let (Some(width), Some(precision)) = (target.int_width(from), to.float_precision())
            else {
                return false;
            };
            // `iN` spans `-2^(N-1) ..= 2^(N-1) - 1`: N - 1 magnitude bits,
            // and `-2^(N-1)` is a power of two, exact in any float.
            let magnitude = if from.is_signed_integer() {
                width.saturating_sub(1)
            } else {
                width
            };
            magnitude <= precision
        }
        // `0.5` fits no integer type.
        CastKind::FloatToInt => false,
        CastKind::FloatToFloat => match (
            from.float_precision(),
            to.float_precision(),
            from.float_max_exponent(),
            to.float_max_exponent(),
        ) {
            (Some(from_precision), Some(to_precision), Some(from_exp), Some(to_exp)) => {
                from_precision <= to_precision && from_exp <= to_exp
            }
            _ => false,
        },
        CastKind::PtrToPtr | CastKind::PtrToInt | CastKind::IntToPtr => true,
    }
}
