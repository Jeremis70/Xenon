//! Checking of binary and compound-assignment operators.

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, BinOp as AstOp};
use crate::middle::ops::{BinOp, CastKind};
use crate::middle::thir::{Expr, ExprKind, LogicalOp};
use crate::source::Span;
use crate::types::Type;

use super::FnCtxt;
use super::literal::literal_class;

/// What an operator does to its operands, for typing purposes.
enum OpClass {
    /// `+ - * / %`: one integer or float type.
    Arithmetic(BinOp),
    /// `& | ^`: one integer type.
    Bitwise(BinOp),
    /// `<< >>`: integer value, integer amount of any type.
    Shift(BinOp),
    /// `== != < > <= >=`: see [`FnCtxt::comparison`].
    Comparison(BinOp),
    /// `&& || ^^`: two `bool`s.
    Logical(LogicalOp),
}

fn classify(op: &AstOp) -> OpClass {
    match op {
        AstOp::Add => OpClass::Arithmetic(BinOp::Add),
        AstOp::Sub => OpClass::Arithmetic(BinOp::Sub),
        AstOp::Mul => OpClass::Arithmetic(BinOp::Mul),
        AstOp::Div => OpClass::Arithmetic(BinOp::Div),
        AstOp::Mod => OpClass::Arithmetic(BinOp::Rem),
        AstOp::BitwiseAnd => OpClass::Bitwise(BinOp::BitAnd),
        AstOp::BitwiseOr => OpClass::Bitwise(BinOp::BitOr),
        AstOp::BitwiseXor => OpClass::Bitwise(BinOp::BitXor),
        AstOp::LShift => OpClass::Shift(BinOp::Shl),
        AstOp::RShift => OpClass::Shift(BinOp::Shr),
        AstOp::Eq => OpClass::Comparison(BinOp::Eq),
        AstOp::NotEq => OpClass::Comparison(BinOp::Ne),
        AstOp::Lt => OpClass::Comparison(BinOp::Lt),
        AstOp::Gt => OpClass::Comparison(BinOp::Gt),
        AstOp::LtEq => OpClass::Comparison(BinOp::Le),
        AstOp::GtEq => OpClass::Comparison(BinOp::Ge),
        AstOp::LogicalAnd => OpClass::Logical(LogicalOp::And),
        AstOp::LogicalOr => OpClass::Logical(LogicalOp::Or),
        AstOp::LogicalXor => OpClass::Logical(LogicalOp::Xor),
    }
}

impl FnCtxt<'_, '_> {
    /// Checks `lhs op rhs`.
    pub(super) fn check_binary(
        &mut self,
        op: &AstOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        match classify(op) {
            OpClass::Arithmetic(bin_op) | OpClass::Bitwise(bin_op) => {
                let (lhs, rhs) = self.check_pair(lhs, rhs, expected)?;
                require_same_type(op, &lhs.ty, &rhs.ty, bin_op.is_arithmetic(), span)?;
                Ok(binary(bin_op, lhs.ty.clone(), lhs, rhs, span))
            }
            OpClass::Shift(bin_op) => {
                let lhs = self.check_expr(lhs, expected)?;
                let rhs = self.check_shift_amount(rhs, &lhs.ty)?;
                require_shift_operands(op, &lhs.ty, &rhs.ty, span)?;
                Ok(binary(bin_op, lhs.ty.clone(), lhs, rhs, span))
            }
            OpClass::Comparison(bin_op) => {
                let (lhs, rhs) = self.check_pair(lhs, rhs, None)?;
                self.comparison(op, bin_op, lhs, rhs, span)
            }
            OpClass::Logical(logical_op) => {
                let lhs = self.check_expr(lhs, Some(&Type::Bool))?;
                let rhs = self.check_expr(rhs, Some(&Type::Bool))?;
                if !lhs.ty.is_bool() || !rhs.ty.is_bool() {
                    return Err(SemanticError::InvalidOperands {
                        op: format!("{op:?}"),
                        detail: format!("expected `bool`, got `{}` and `{}`", lhs.ty, rhs.ty),
                        span,
                    });
                }
                Ok(Expr {
                    kind: ExprKind::Logical {
                        op: logical_op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: Type::Bool,
                    span,
                })
            }
        }
    }

    /// Checks the operand of `place op= value` and returns the operator.
    ///
    /// The operation must produce the place's own type, so `value` is
    /// checked exactly like the right operand of `place op value`.
    pub(super) fn check_compound(
        &mut self,
        op: &AstOp,
        place: &Expr,
        value: &ast::Expr,
        span: Span,
    ) -> SemanticResult<(BinOp, Expr)> {
        match classify(op) {
            OpClass::Arithmetic(bin_op) | OpClass::Bitwise(bin_op) => {
                let value = self.check_expr(value, Some(&place.ty))?;
                require_same_type(op, &place.ty, &value.ty, bin_op.is_arithmetic(), span)?;
                Ok((bin_op, value))
            }
            OpClass::Shift(bin_op) => {
                let value = self.check_shift_amount(value, &place.ty)?;
                require_shift_operands(op, &place.ty, &value.ty, span)?;
                Ok((bin_op, value))
            }
            OpClass::Comparison(_) | OpClass::Logical(_) => Err(SemanticError::InvalidOperands {
                op: format!("{op:?}"),
                detail: "not a compound assignment operator".to_owned(),
                span,
            }),
        }
    }

    /// Checks a shift amount; an untyped literal amount takes the type of
    /// the shifted value.
    fn check_shift_amount(&mut self, amount: &ast::Expr, value_ty: &Type) -> SemanticResult<Expr> {
        let expected = literal_class(amount).map(|_| value_ty);
        self.check_expr(amount, expected)
    }

    /// Checks a comparison, making any operand conversion explicit.
    ///
    /// Integers of different types compare after widening the narrower
    /// operand to the wider type; on equal widths the unsigned type wins.
    /// Floats of different widths compare at the wider width. Pointers only
    /// support `==` and `!=` between identical types.
    fn comparison(
        &self,
        op: &AstOp,
        bin_op: BinOp,
        lhs: Expr,
        rhs: Expr,
        span: Span,
    ) -> SemanticResult<Expr> {
        let invalid = |detail: String| SemanticError::InvalidOperands {
            op: format!("{op:?}"),
            detail,
            span,
        };
        let (l, r) = (&lhs.ty, &rhs.ty);

        if l.is_indirect() || r.is_indirect() {
            if l != r || !matches!(bin_op, BinOp::Eq | BinOp::Ne) {
                return Err(invalid(format!(
                    "operator not supported for `{l}` and `{r}`"
                )));
            }
            return Ok(binary(bin_op, Type::Bool, lhs, rhs, span));
        }

        let common = if l == r && (l.is_integer() || l.is_float() || l.is_bool()) {
            l.clone()
        } else if l.is_integer() && r.is_integer() {
            self.common_int_type(l, r)
        } else if l.is_float() && r.is_float() {
            common_float_type(l, r)
                .ok_or_else(|| invalid(format!("incompatible types `{l}` and `{r}`")))?
        } else {
            return Err(invalid(format!("incompatible types `{l}` and `{r}`")));
        };

        let lhs = widen(lhs, &common);
        let rhs = widen(rhs, &common);
        Ok(binary(bin_op, Type::Bool, lhs, rhs, span))
    }

    /// The type two different integer types are compared at.
    fn common_int_type(&self, lhs: &Type, rhs: &Type) -> Type {
        let target = self.target();
        let (lw, rw) = (target.int_width(lhs), target.int_width(rhs));
        if rw > lw || (rw == lw && rhs.is_unsigned_integer() && !lhs.is_unsigned_integer()) {
            rhs.clone()
        } else {
            lhs.clone()
        }
    }
}

/// The type two different float types are compared at, `None` if neither
/// is wider (e.g. `f16` and `bf16`).
fn common_float_type(lhs: &Type, rhs: &Type) -> Option<Type> {
    let (lw, rw) = (lhs.float_width()?, rhs.float_width()?);
    match lw.cmp(&rw) {
        std::cmp::Ordering::Greater => Some(lhs.clone()),
        std::cmp::Ordering::Less => Some(rhs.clone()),
        std::cmp::Ordering::Equal => None,
    }
}

/// Converts `expr` to the numeric type `ty` with an explicit cast, if its
/// type differs.
fn widen(expr: Expr, ty: &Type) -> Expr {
    if &expr.ty == ty {
        return expr;
    }
    let kind = if ty.is_float() {
        CastKind::FloatToFloat
    } else {
        CastKind::IntToInt
    };
    Expr {
        span: expr.span,
        ty: ty.clone(),
        kind: ExprKind::Cast {
            kind,
            operand: Box::new(expr),
        },
    }
}

fn binary(op: BinOp, ty: Type, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty,
        span,
    }
}

/// Requires both operands of an arithmetic or bitwise operator to have one
/// integer (or, with `allow_float`, float) type.
fn require_same_type(
    op: &AstOp,
    lhs: &Type,
    rhs: &Type,
    allow_float: bool,
    span: Span,
) -> SemanticResult<()> {
    let numeric = |ty: &Type| ty.is_integer() || (allow_float && ty.is_float());
    let detail = if lhs == rhs && numeric(lhs) {
        return Ok(());
    } else if lhs.is_indirect() || rhs.is_indirect() {
        format!("operator not supported for `{lhs}` and `{rhs}`")
    } else if numeric(lhs) && numeric(rhs) && lhs.is_float() == rhs.is_float() {
        let kind = if lhs.is_float() { "float" } else { "integer" };
        format!("mixed {kind} types `{lhs}` and `{rhs}`")
    } else if !allow_float {
        format!("integer operands required, got `{lhs}` and `{rhs}`")
    } else {
        format!("cannot combine `{lhs}` and `{rhs}`")
    };
    Err(SemanticError::InvalidOperands {
        op: format!("{op:?}"),
        detail,
        span,
    })
}

/// Requires a shift's value and amount to both be integers.
fn require_shift_operands(
    op: &AstOp,
    value: &Type,
    amount: &Type,
    span: Span,
) -> SemanticResult<()> {
    if value.is_integer() && amount.is_integer() {
        return Ok(());
    }
    Err(SemanticError::InvalidOperands {
        op: format!("{op:?}"),
        detail: format!("expected integer operands, got `{value}` and `{amount}`"),
        span,
    })
}
