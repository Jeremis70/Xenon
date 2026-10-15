//! Evaluation of MIR operations on compile-time constants.
//!
//! This module is the single definition of what an operation computes, and
//! when it traps, given constant operands: compile-time diagnostics and
//! passes that replace operations by their results all evaluate through
//! [`eval_rvalue`] instead of re-deriving arithmetic rules.
//!
//! Integer results follow the operand's bit width on the target. Traps are
//! reported with the [`AssertKind`] the runtime guard of
//! [`normalize_runtime_checks`](super::analysis::normalize_runtime_checks)
//! would fail with; those guards are the symbolic counterpart of the
//! concrete rules here, and the two must stay in sync.
//!
//! Evaluation is partial: floats, addresses, and pointer operations are
//! [`EvalResult::Unknown`], as is anything depending on an unknown operand.
//! An operation that traps whatever its unknown operand holds, such as a
//! division by a known `0`, is still reported as a trap.

use num_bigint::{BigInt, Sign};
use num_traits::{One, Zero};

use crate::middle::target::TargetSpec;
use crate::types::Type;

use super::body::Local;
use super::syntax::{AssertKind, BinOp, CastKind, ConstValue, Constant, Operand, Rvalue, UnOp};
use super::typing::{self, HasLocalDecls, TypingError};

/// The outcome of evaluating an rvalue at compile time.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalResult {
    /// The rvalue always produces this value.
    Value(Constant),
    /// The rvalue always traps with this assertion.
    Trap(AssertKind),
    /// The result depends on values unknown at compile time.
    Unknown,
}

/// The compile-time value of `operand`, if known.
///
/// Constants are known, zero-sized integers always hold `0`, and
/// `local_value` supplies what the caller knows about plain locals.
pub fn known_operand(
    operand: &Operand,
    decls: &impl HasLocalDecls,
    local_value: impl Fn(Local) -> Option<Constant>,
) -> Option<Constant> {
    match operand {
        Operand::Constant(constant) => Some((**constant).clone()),
        Operand::Copy(place) => place.as_local().and_then(&local_value).or_else(|| {
            let ty = typing::place_ty(place, decls).ok()?;
            ty.is_zero_sized().then(|| Constant::zero(ty)).flatten()
        }),
    }
}

/// Evaluates `rvalue`, reading operand values through `value_of`.
///
/// Fails only if `rvalue` is ill-typed.
pub fn eval_rvalue(
    rvalue: &Rvalue,
    decls: &impl HasLocalDecls,
    target: &TargetSpec,
    value_of: impl Fn(&Operand) -> Option<Constant>,
) -> Result<EvalResult, TypingError> {
    let ty = typing::rvalue_ty(rvalue, decls)?;
    let result = match rvalue {
        Rvalue::Use(operand) => value_of(operand).map_or(EvalResult::Unknown, EvalResult::Value),
        Rvalue::UnaryOp(op, operand) => match value_of(operand) {
            Some(operand) => eval_unary(*op, &operand.value, &ty, target),
            None => EvalResult::Unknown,
        },
        Rvalue::BinaryOp(op, operands) => {
            let lhs = value_of(&operands.0);
            let rhs = value_of(&operands.1);
            let operand_ty = typing::operand_ty(&operands.0, decls)?;
            eval_binary(
                *op,
                lhs.as_ref().map(|c| &c.value),
                rhs.as_ref().map(|c| &c.value),
                operand_ty,
                target,
            )
        }
        Rvalue::Overflows(op, operands) => match (value_of(&operands.0), value_of(&operands.1)) {
            (Some(lhs), Some(rhs)) => {
                match eval_binary(*op, Some(&lhs.value), Some(&rhs.value), &lhs.ty, target) {
                    EvalResult::Value(_) => EvalResult::Value(Constant::bool(false)),
                    EvalResult::Trap(AssertKind::Overflow(_)) => {
                        EvalResult::Value(Constant::bool(true))
                    }
                    _ => EvalResult::Unknown,
                }
            }
            _ => EvalResult::Unknown,
        },
        Rvalue::Cast(CastKind::IntToInt, operand, to) => {
            match value_of(operand).map(|operand| operand.value) {
                Some(ConstValue::Int(value)) => int_value(wrap(value, to, target), to),
                _ => EvalResult::Unknown,
            }
        }
        Rvalue::Cast(..) | Rvalue::AddressOf(..) => EvalResult::Unknown,
    };
    Ok(result)
}

fn eval_unary(op: UnOp, operand: &ConstValue, ty: &Type, target: &TargetSpec) -> EvalResult {
    match (op, operand) {
        (UnOp::Not, ConstValue::Bool(value)) => EvalResult::Value(Constant::bool(!value)),
        // Two's complement: `!x == -x - 1`, within the width of `ty`.
        (UnOp::Not, ConstValue::Int(value)) => int_value(wrap(-value - 1, ty, target), ty),
        // Runtime checks guard negation as `0 - x`.
        (UnOp::Neg, ConstValue::Int(value)) => {
            checked(-value, ty, target, AssertKind::Overflow(BinOp::Sub))
        }
        _ => EvalResult::Unknown,
    }
}

/// Evaluates `lhs op rhs` on operands of type `ty` (the shift amount may
/// have another type).
fn eval_binary(
    op: BinOp,
    lhs: Option<&ConstValue>,
    rhs: Option<&ConstValue>,
    ty: &Type,
    target: &TargetSpec,
) -> EvalResult {
    if !ty.is_integer() {
        return match (op, lhs, rhs) {
            (_, Some(ConstValue::Bool(a)), Some(ConstValue::Bool(b))) => eval_bool(op, *a, *b),
            _ => EvalResult::Unknown,
        };
    }
    let as_int = |value: Option<&ConstValue>| match value {
        Some(ConstValue::Int(value)) => Some(value.clone()),
        _ => None,
    };
    let (lhs, rhs) = (as_int(lhs), as_int(rhs));

    // Traps decided by one operand alone.
    match op {
        BinOp::Div if rhs.as_ref().is_some_and(Zero::is_zero) => {
            return EvalResult::Trap(AssertKind::DivisionByZero);
        }
        BinOp::Rem if rhs.as_ref().is_some_and(Zero::is_zero) => {
            return EvalResult::Trap(AssertKind::RemainderByZero);
        }
        BinOp::Shl | BinOp::Shr => {
            let Some(width) = target.int_width(ty) else {
                return EvalResult::Unknown;
            };
            // No amount is smaller than a width of `0`.
            let out_of_range = width == 0
                || rhs.as_ref().is_some_and(|amount| {
                    amount.sign() == Sign::Minus || *amount >= BigInt::from(width)
                });
            if out_of_range {
                return EvalResult::Trap(AssertKind::ShiftOutOfRange);
            }
        }
        _ => {}
    }

    let (Some(a), Some(b)) = (lhs, rhs) else {
        return EvalResult::Unknown;
    };
    match op {
        BinOp::Add => checked(a + b, ty, target, AssertKind::Overflow(op)),
        BinOp::Sub => checked(a - b, ty, target, AssertKind::Overflow(op)),
        BinOp::Mul => checked(a * b, ty, target, AssertKind::Overflow(op)),
        // Division truncates toward zero, and the remainder takes the sign
        // of the dividend. Only `MIN / -1` leaves the range.
        BinOp::Div => checked(a / b, ty, target, AssertKind::SignedDivisionOverflow),
        // `MIN % -1` is `0`, but traps like `MIN / -1` does.
        BinOp::Rem if b == -BigInt::one() && is_min(&a, ty, target) => {
            EvalResult::Trap(AssertKind::SignedRemainderOverflow)
        }
        BinOp::Rem => int_value(Some(a % b), ty),
        // Bitwise operations on in-range two's complement values stay in
        // range.
        BinOp::BitAnd => int_value(Some(a & b), ty),
        BinOp::BitOr => int_value(Some(a | b), ty),
        BinOp::BitXor => int_value(Some(a ^ b), ty),
        // The amount was checked above.
        BinOp::Shl => int_value(wrap(a << usize::try_from(&b).unwrap_or(0), ty, target), ty),
        // Signed values shift arithmetically; unsigned values are never
        // negative, so the same rounding applies to both.
        BinOp::Shr => int_value(Some(a >> usize::try_from(&b).unwrap_or(0)), ty),
        BinOp::Eq => compare(a == b),
        BinOp::Ne => compare(a != b),
        BinOp::Lt => compare(a < b),
        BinOp::Le => compare(a <= b),
        BinOp::Gt => compare(a > b),
        BinOp::Ge => compare(a >= b),
    }
}

fn eval_bool(op: BinOp, a: bool, b: bool) -> EvalResult {
    let value = match op {
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        BinOp::Eq => a == b,
        BinOp::Ne => a != b,
        BinOp::Lt => !a & b,
        BinOp::Le => a <= b,
        BinOp::Gt => a & !b,
        BinOp::Ge => a >= b,
        _ => return EvalResult::Unknown,
    };
    compare(value)
}

fn compare(value: bool) -> EvalResult {
    EvalResult::Value(Constant::bool(value))
}

/// `value` if it fits `ty`, else a trap with `kind`.
fn checked(value: BigInt, ty: &Type, target: &TargetSpec, kind: AssertKind) -> EvalResult {
    match target.int_bounds(ty) {
        Some((min, max)) if min <= value && value <= max => int_value(Some(value), ty),
        Some(_) => EvalResult::Trap(kind),
        None => EvalResult::Unknown,
    }
}

fn is_min(value: &BigInt, ty: &Type, target: &TargetSpec) -> bool {
    target.int_bounds(ty).is_some_and(|(min, _)| *value == min)
}

/// The value of `ty` congruent to `value` modulo `2^width`: the low bits
/// of `value`, read with the signedness of `ty`.
fn wrap(value: BigInt, ty: &Type, target: &TargetSpec) -> Option<BigInt> {
    let (min, max) = target.int_bounds(ty)?;
    let modulus: BigInt = &max - &min + 1;
    let offset: BigInt = (value - &min) % &modulus;
    let offset = if offset.sign() == Sign::Minus {
        offset + modulus
    } else {
        offset
    };
    Some(offset + min)
}

fn int_value(value: Option<BigInt>, ty: &Type) -> EvalResult {
    value.map_or(EvalResult::Unknown, |value| {
        EvalResult::Value(Constant::int(value, ty.clone()))
    })
}
