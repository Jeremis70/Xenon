//! Fixed-size array literals, indexing, and broadcast assignment.

use num_bigint::BigInt;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, ExprKind as Ast};
use crate::middle::ops::{CastKind, IndirectionKind};
use crate::middle::thir::{Expr, ExprKind, Literal, Stmt, StmtKind};
use crate::source::Span;
use crate::types::Type;

use super::FnCtxt;
use super::expr::needs_context;
use super::tuple::{auto_deref, binding_expr};

impl FnCtxt<'_, '_> {
    pub(super) fn check_array(
        &mut self,
        elems: &[ast::Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        let mut inferred = None;
        let elem_ty = match expected {
            Some(Type::Array(elem, len)) => {
                if elems.len() != *len as usize {
                    return Err(SemanticError::ArrayLengthMismatch {
                        expected: *len,
                        found: elems.len(),
                        span,
                    });
                }
                (**elem).clone()
            }
            _ => {
                let (position, first) = elems
                    .iter()
                    .enumerate()
                    .find(|(_, elem)| !needs_context(elem))
                    .or_else(|| elems.iter().enumerate().next())
                    .ok_or(SemanticError::EmptyArrayWithoutType { span })?;
                let first = self.check_expr(first, None)?;
                let ty = first.ty.clone();
                inferred = Some((position, first));
                ty
            }
        };
        let elems = elems
            .iter()
            .enumerate()
            .map(|(position, elem)| match &inferred {
                Some((index, expr)) if *index == position => Ok(expr.clone()),
                _ => self.check_expr_has_type(elem, &elem_ty),
            })
            .collect::<SemanticResult<Vec<_>>>()?;
        let len = u32::try_from(elems.len()).map_err(|_| SemanticError::ArrayLengthMismatch {
            expected: u32::MAX,
            found: elems.len(),
            span,
        })?;
        Ok(Expr {
            ty: Type::Array(Box::new(elem_ty), len),
            kind: ExprKind::Array(elems),
            span,
        })
    }

    pub(super) fn check_index(
        &mut self,
        base: Expr,
        index: &ast::Expr,
        span: Span,
    ) -> SemanticResult<Expr> {
        let base = auto_deref(base);
        let Type::Array(elem, len) = &base.ty else {
            return Err(SemanticError::IndexOfNonArray { ty: base.ty, span });
        };
        let ty = (**elem).clone();
        let index = self.check_expr(index, Some(&Type::ISize))?;
        if !index.ty.is_integer() {
            return Err(SemanticError::InvalidArrayIndex {
                ty: index.ty,
                span: index.span,
            });
        }
        let value = match &index.kind {
            ExprKind::Literal(Literal::Int(value)) => Some(value.clone()),
            ExprKind::Unary {
                op: crate::middle::ops::UnOp::Neg,
                operand,
            } => match &operand.kind {
                ExprKind::Literal(Literal::Int(value)) => Some(-value),
                _ => None,
            },
            _ => None,
        };
        if let Some(value) = value
            && (value < BigInt::from(0) || value >= BigInt::from(*len))
        {
            return Err(SemanticError::ArrayIndexOutOfBounds {
                index: value,
                len: *len,
                span: index.span,
            });
        }
        // Widen before comparing with the length, even for narrow integer indices.
        let width =
            self.target()
                .int_width(&index.ty)
                .ok_or_else(|| SemanticError::InvalidArrayIndex {
                    ty: index.ty.clone(),
                    span: index.span,
                })?;
        let wide = if index.ty.is_signed_integer() {
            Type::Int(width.max(33))
        } else {
            Type::UInt(width.max(32))
        };
        let index = if index.ty == wide {
            index
        } else {
            Expr {
                span: index.span,
                ty: wide,
                kind: ExprKind::Cast {
                    kind: CastKind::IntToInt,
                    operand: Box::new(index),
                },
            }
        };
        Ok(auto_deref(Expr {
            kind: ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            ty,
            span,
        }))
    }

    pub(super) fn check_multi_index_assign(
        &mut self,
        target: &ast::Expr,
        value: &ast::Expr,
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        let Ast::Index { base, indices } = &target.kind else {
            return Err(SemanticError::NotAPlaceExpression { span });
        };
        let base = self.check_place(base)?;
        let base_ty = base.ty.clone();
        let pointer = Expr {
            ty: Type::Reference(Box::new(base_ty.clone())),
            span,
            kind: ExprKind::AddressOf {
                kind: IndirectionKind::Reference,
                place: Box::new(base),
            },
        };
        let base = self.array_temp(pointer, out);
        let base = Expr {
            kind: ExprKind::Deref(Box::new(base)),
            ty: base_ty,
            span,
        };
        let mut places = Vec::with_capacity(indices.len());
        for index in indices {
            let place = self.check_index(base.clone(), index, span)?;
            let ty = place.ty.clone();
            let pointer = Expr {
                ty: Type::Pointer(Box::new(ty.clone())),
                span,
                kind: ExprKind::AddressOf {
                    kind: IndirectionKind::Pointer,
                    place: Box::new(place),
                },
            };
            let pointer = self.array_temp(pointer, out);
            places.push(Expr {
                kind: ExprKind::Deref(Box::new(pointer)),
                ty,
                span,
            });
        }
        let first = places
            .first()
            .ok_or(SemanticError::NotAPlaceExpression { span })?;
        let value = self.check_expr_has_type(value, &first.ty)?;
        let value = self.array_temp(value, out);
        for place in places {
            out.push(Stmt {
                kind: StmtKind::Assign {
                    place,
                    value: value.clone(),
                },
                span,
            });
        }
        Ok(())
    }

    fn array_temp(&mut self, init: Expr, out: &mut Vec<Stmt>) -> Expr {
        let span = init.span;
        let binding = self.declare_temp(init.ty.clone(), span);
        let value = binding_expr(binding, init.ty.clone(), span);
        out.push(Stmt {
            kind: StmtKind::Let {
                binding,
                init: Some(init),
            },
            span,
        });
        value
    }
}
