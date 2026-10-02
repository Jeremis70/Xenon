//! Type derivation for MIR places, operands, and rvalues.
//!
//! MIR stores types only on locals and constants; every other type is
//! derived from those by the rules in this module. These rules are the
//! single source of truth for MIR typing: the builder, the verifier, passes,
//! and backends all call them instead of re-implementing type inference.
//!
//! MIR typing is strict. Operands of a binary operation must already have
//! identical types (except shift amounts): any implicit conversions of the
//! source language are made explicit as casts before MIR is built.

use thiserror::Error;

use crate::index::IndexVec;
use crate::types::Type;

use super::body::{Body, Local, LocalDecl};
use super::syntax::{BinOp, CastKind, Operand, Place, ProjectionElem, Rvalue, UnOp};

/// Anything that owns MIR local declarations and can therefore type places.
pub trait HasLocalDecls {
    /// The declarations of every local, indexed by [`Local`].
    fn local_decls(&self) -> &IndexVec<Local, LocalDecl>;
}

impl HasLocalDecls for IndexVec<Local, LocalDecl> {
    fn local_decls(&self) -> &IndexVec<Local, LocalDecl> {
        self
    }
}

impl HasLocalDecls for Body {
    fn local_decls(&self) -> &IndexVec<Local, LocalDecl> {
        Body::local_decls(self)
    }
}

/// A MIR construct whose type cannot be derived.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TypingError {
    #[error("use of undeclared local `{0}`")]
    UnknownLocal(Local),

    #[error("cannot dereference a value of non-pointer type `{0}`")]
    DerefOfNonPointer(Type),

    #[error("`{op:?}` operands have different types `{lhs}` and `{rhs}`")]
    OperandMismatch { op: BinOp, lhs: Type, rhs: Type },

    #[error("`{op:?}` is not defined for operands of type `{ty}`")]
    InvalidBinaryOperand { op: BinOp, ty: Type },

    #[error("shift amount must be an integer, found `{0}`")]
    InvalidShiftAmount(Type),

    #[error("`{op:?}` is not defined for an operand of type `{ty}`")]
    InvalidUnaryOperand { op: UnOp, ty: Type },

    #[error("overflow checks are not defined for `{0:?}`")]
    UncheckableOverflowOp(BinOp),

    #[error("`{kind:?}` cast from `{from}` to `{to}` is invalid")]
    InvalidCast {
        kind: CastKind,
        from: Type,
        to: Type,
    },
}

/// The type of the value stored at `place`.
pub fn place_ty<'a>(place: &Place, decls: &'a impl HasLocalDecls) -> Result<&'a Type, TypingError> {
    let mut ty = &decls
        .local_decls()
        .get(place.local)
        .ok_or(TypingError::UnknownLocal(place.local))?
        .ty;
    for elem in &place.projection {
        ty = match elem {
            ProjectionElem::Deref => ty
                .pointee()
                .ok_or_else(|| TypingError::DerefOfNonPointer(ty.clone()))?,
        };
    }
    Ok(ty)
}

/// The type of the value produced by `operand`.
pub fn operand_ty<'a>(
    operand: &'a Operand,
    decls: &'a impl HasLocalDecls,
) -> Result<&'a Type, TypingError> {
    match operand {
        Operand::Copy(place) => place_ty(place, decls),
        Operand::Constant(constant) => Ok(&constant.ty),
    }
}

/// The type of the value produced by `rvalue`.
pub fn rvalue_ty(rvalue: &Rvalue, decls: &impl HasLocalDecls) -> Result<Type, TypingError> {
    match rvalue {
        Rvalue::Use(operand) => operand_ty(operand, decls).cloned(),
        Rvalue::UnaryOp(op, operand) => unary_op_ty(*op, operand_ty(operand, decls)?),
        Rvalue::BinaryOp(op, operands) => binary_op_ty(
            *op,
            operand_ty(&operands.0, decls)?,
            operand_ty(&operands.1, decls)?,
        ),
        Rvalue::Overflows(op, operands) => {
            let lhs = operand_ty(&operands.0, decls)?;
            let rhs = operand_ty(&operands.1, decls)?;
            if !op.is_overflow_checkable() {
                return Err(TypingError::UncheckableOverflowOp(*op));
            }
            binary_op_ty(*op, lhs, rhs)?;
            if !lhs.is_integer() {
                return Err(TypingError::InvalidBinaryOperand {
                    op: *op,
                    ty: lhs.clone(),
                });
            }
            Ok(Type::Bool)
        }
        Rvalue::Cast(kind, operand, target) => {
            let source = operand_ty(operand, decls)?;
            check_cast(*kind, source, target)?;
            Ok(target.clone())
        }
        Rvalue::AddressOf(kind, place) => Ok(kind.pointer_to(place_ty(place, decls)?.clone())),
    }
}

/// The result type of `op` applied to an operand of type `ty`.
pub fn unary_op_ty(op: UnOp, ty: &Type) -> Result<Type, TypingError> {
    let valid = match op {
        UnOp::Neg => ty.is_integer() || ty.is_float(),
        UnOp::Not => ty.is_integer() || ty.is_bool(),
    };
    if valid {
        Ok(ty.clone())
    } else {
        Err(TypingError::InvalidUnaryOperand { op, ty: ty.clone() })
    }
}

/// The result type of `op` applied to operands of types `lhs` and `rhs`.
pub fn binary_op_ty(op: BinOp, lhs: &Type, rhs: &Type) -> Result<Type, TypingError> {
    if op.is_shift() {
        if !lhs.is_integer() {
            return Err(TypingError::InvalidBinaryOperand {
                op,
                ty: lhs.clone(),
            });
        }
        if !rhs.is_integer() {
            return Err(TypingError::InvalidShiftAmount(rhs.clone()));
        }
        return Ok(lhs.clone());
    }

    if lhs != rhs {
        return Err(TypingError::OperandMismatch {
            op,
            lhs: lhs.clone(),
            rhs: rhs.clone(),
        });
    }

    let valid = match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            lhs.is_integer() || lhs.is_float()
        }
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => lhs.is_integer() || lhs.is_bool(),
        BinOp::Eq | BinOp::Ne => true,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            lhs.is_integer() || lhs.is_float() || lhs.is_bool()
        }
        // Shifts are fully handled above.
        BinOp::Shl | BinOp::Shr => false,
    };
    if !valid {
        return Err(TypingError::InvalidBinaryOperand {
            op,
            ty: lhs.clone(),
        });
    }

    Ok(if op.is_comparison() {
        Type::Bool
    } else {
        lhs.clone()
    })
}

/// Checks that a cast of `kind` may convert `from` into `to`.
pub fn check_cast(kind: CastKind, from: &Type, to: &Type) -> Result<(), TypingError> {
    let valid = match kind {
        CastKind::IntToInt => from.is_integer() && to.is_integer(),
        CastKind::FloatToFloat => from.is_float() && to.is_float(),
        CastKind::IntToFloat => from.is_integer() && to.is_float(),
        CastKind::FloatToInt => from.is_float() && to.is_integer(),
        CastKind::PtrToPtr => from.is_indirect() && to.is_indirect(),
    };
    if valid {
        Ok(())
    } else {
        Err(TypingError::InvalidCast {
            kind,
            from: from.clone(),
            to: to.clone(),
        })
    }
}
