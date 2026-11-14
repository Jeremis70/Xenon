//! Checking of explicit conversions `operand as Type`.

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast;
use crate::middle::cast::cast_kind;
use crate::middle::thir::{Expr, ExprKind};
use crate::source::Span;
use crate::types::Type;

use super::FnCtxt;
use super::literal::literal_class;

impl FnCtxt<'_, '_> {
    /// Checks `operand as ty`.
    ///
    /// An untyped literal operand takes the target type when it can, so
    /// `300 as u8` is range-checked like `let u8 x = 300;`. Any other
    /// operand is checked without expectation: the cast does not reach into
    /// it.
    pub(super) fn check_cast(
        &mut self,
        operand: &ast::Expr,
        ty: &Type,
        span: Span,
    ) -> SemanticResult<Expr> {
        let hint = literal_class(operand)
            .filter(|class| class.accepts(ty))
            .map(|_| ty);
        let operand = self.check_expr(operand, hint)?;
        let from = &operand.ty;
        let Some(kind) = cast_kind(from, ty) else {
            return Err(SemanticError::InvalidCast {
                from: from.clone(),
                to: ty.clone(),
                note: invalid_cast_note(from, ty),
                span,
            });
        };
        if from == ty {
            return Ok(operand);
        }
        Ok(Expr {
            ty: ty.clone(),
            kind: ExprKind::Cast {
                kind,
                operand: Box::new(operand),
            },
            span,
        })
    }
}

/// A hint for a cast that [`cast_kind`] rejects.
fn invalid_cast_note(from: &Type, to: &Type) -> Option<&'static str> {
    let pointer = |ty: &Type| matches!(ty, Type::Pointer(_));
    if from.contains_reference() || to.contains_reference() {
        Some("references cannot be cast")
    } else if from.is_bool() || to.is_bool() {
        Some("`bool` has no casts; compare with `==` or use a conditional expression")
    } else if (pointer(from) && to.is_integer()) || (from.is_integer() && pointer(to)) {
        Some("pointers only convert to and from `usize`; cast through `usize`")
    } else {
        None
    }
}
