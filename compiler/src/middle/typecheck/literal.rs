//! Classification of untyped literal expressions.
//!
//! An untyped literal expression is one whose type is decided entirely by
//! its context: a numeric literal, or an operator applied only to such
//! expressions. When it meets an expression of known type (the other
//! operand of a binary operator, the other branch of an `if`), it adopts
//! that type instead of the context-free default.

use crate::frontend::ast::{BinOp, Expr, ExprKind, UnaryOp};
use crate::types::Type;

/// The kind of value an untyped literal expression produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiteralClass {
    /// An integer; any integer type fits.
    Int,
    /// A float; any float type fits.
    Float,
}

impl LiteralClass {
    /// Returns `true` if a literal of this class may take type `ty`.
    pub(super) fn accepts(self, ty: &Type) -> bool {
        match self {
            LiteralClass::Int => ty.is_integer(),
            LiteralClass::Float => ty.is_float(),
        }
    }

    /// The type of a literal of this class without a usable context.
    pub(super) fn default_ty(self) -> Type {
        match self {
            LiteralClass::Int => Type::Int(64),
            LiteralClass::Float => Type::Float64,
        }
    }

    /// The type for a literal of this class in a context expecting
    /// `expected`.
    pub(super) fn ty_for(self, expected: Option<&Type>) -> Type {
        match expected {
            Some(ty) if self.accepts(ty) => ty.clone(),
            _ => self.default_ty(),
        }
    }
}

/// Returns the class of `expr` if it is an untyped literal expression.
pub(super) fn literal_class(expr: &Expr) -> Option<LiteralClass> {
    match &expr.kind {
        ExprKind::Int(_) => Some(LiteralClass::Int),
        ExprKind::Float(_) => Some(LiteralClass::Float),
        ExprKind::UnaryOp {
            op: UnaryOp::Neg,
            operand,
        } => literal_class(operand),
        ExprKind::UnaryOp {
            op: UnaryOp::BitwiseNot,
            operand,
        } => literal_class(operand).filter(|class| *class == LiteralClass::Int),
        ExprKind::BinOp { lhs, op, rhs } => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => same_class(lhs, rhs),
            BinOp::BitwiseAnd | BinOp::BitwiseOr | BinOp::BitwiseXor => {
                same_class(lhs, rhs).filter(|class| *class == LiteralClass::Int)
            }
            // A shift has the type of its left operand.
            BinOp::LShift | BinOp::RShift => {
                literal_class(lhs).filter(|class| *class == LiteralClass::Int)
            }
            _ => None,
        },
        ExprKind::IfElse {
            then_branch,
            else_branch,
            ..
        } => same_class(then_branch, else_branch),
        _ => None,
    }
}

fn same_class(lhs: &Expr, rhs: &Expr) -> Option<LiteralClass> {
    let class = literal_class(lhs)?;
    (literal_class(rhs)? == class).then_some(class)
}
