//! Checking of expressions and places.

use num_bigint::BigInt;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, ExprKind as Ast};
use crate::middle::ops::{IndirectionKind, UnOp};
use crate::middle::thir::{Expr, ExprKind, Literal};
use crate::source::Span;
use crate::types::Type;

use super::FnCtxt;
use super::literal::{LiteralClass, literal_class};

impl FnCtxt<'_, '_> {
    /// Checks `expr`, using `expected` to type untyped literals and to pick
    /// the flavor of `@place`.
    ///
    /// The expectation is only a hint: the result may have another type,
    /// which callers that require `expected` reject with [`Self::coerce`].
    pub(super) fn check_expr(
        &mut self,
        expr: &ast::Expr,
        expected: Option<&Type>,
    ) -> SemanticResult<Expr> {
        let span = expr.span;
        match &expr.kind {
            Ast::Int(value) => {
                self.int_literal(value.clone(), LiteralClass::Int.ty_for(expected), span)
            }
            Ast::Float(value) => Ok(float_literal(*value, expected, span)),
            Ast::Bool(value) => Ok(Expr {
                kind: ExprKind::Literal(Literal::Bool(*value)),
                ty: Type::Bool,
                span,
            }),
            Ast::Address(value) => self.address_literal(value, expected, span),
            Ast::Ident(_) | Ast::Deref(_) => self.check_place(expr),
            Ast::AddressOf(operand) => {
                let place = self.check_place(operand)?;
                // The context picks the flavor: `&T` where a reference is
                // expected, `*T` everywhere else.
                let kind = match expected {
                    Some(Type::Reference(_)) => IndirectionKind::Reference,
                    _ => IndirectionKind::Pointer,
                };
                Ok(Expr {
                    ty: kind.pointer_to(place.ty.clone()),
                    kind: ExprKind::AddressOf {
                        kind,
                        place: Box::new(place),
                    },
                    span,
                })
            }
            Ast::UnaryOp { op, operand } => self.check_unary(op, operand, expected, span),
            Ast::BinOp { lhs, op, rhs } => self.check_binary(op, lhs, rhs, expected, span),
            Ast::Call { name, args } => self.check_call(name, args, span),
            Ast::IfElse {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.check_condition(condition)?;
                let (then_expr, else_expr) = self.check_pair(then_branch, else_branch, expected)?;
                if then_expr.ty != else_expr.ty {
                    return Err(SemanticError::TypeMismatch {
                        expected: then_expr.ty.to_string(),
                        found: else_expr.ty.to_string(),
                        span: else_expr.span,
                    });
                }
                Ok(Expr {
                    ty: then_expr.ty.clone(),
                    kind: ExprKind::If {
                        condition: Box::new(condition),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                    },
                    span,
                })
            }
            Ast::Loop { body } => self.check_loop(None, body, expected, span),
            Ast::CondLoop {
                post,
                inverted,
                condition,
                body,
            } => self.check_loop(Some((condition, *post, *inverted)), body, expected, span),
        }
    }

    /// Checks `expr` and requires it to have type `expected`.
    pub(super) fn check_expr_has_type(
        &mut self,
        expr: &ast::Expr,
        expected: &Type,
    ) -> SemanticResult<Expr> {
        let checked = self.check_expr(expr, Some(expected))?;
        self.coerce(checked, expected)
    }

    /// Checks a `bool` condition.
    pub(super) fn check_condition(&mut self, expr: &ast::Expr) -> SemanticResult<Expr> {
        let condition = self.check_expr(expr, Some(&Type::Bool))?;
        if !condition.ty.is_bool() {
            return Err(SemanticError::ConditionNotBool {
                found: condition.ty.to_string(),
                span: expr.span,
            });
        }
        Ok(condition)
    }

    /// Checks two expressions that must end up with the same type, such as
    /// the operands of `+` or the branches of an `if`.
    ///
    /// If exactly one side is an untyped literal expression, it adopts the
    /// type of the other side. Otherwise both sides are checked against
    /// `expected`. The caller decides whether the resulting types agree.
    pub(super) fn check_pair(
        &mut self,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        expected: Option<&Type>,
    ) -> SemanticResult<(Expr, Expr)> {
        match (literal_class(lhs).is_some(), literal_class(rhs).is_some()) {
            (false, true) => {
                let lhs = self.check_expr(lhs, expected)?;
                let rhs = self.check_expr(rhs, Some(&lhs.ty))?;
                Ok((lhs, rhs))
            }
            (true, false) => {
                let rhs = self.check_expr(rhs, expected)?;
                let lhs = self.check_expr(lhs, Some(&rhs.ty))?;
                Ok((lhs, rhs))
            }
            _ => Ok((
                self.check_expr(lhs, expected)?,
                self.check_expr(rhs, expected)?,
            )),
        }
    }

    /// Checks an expression that must denote a memory location.
    ///
    /// A variable of type `&T` denotes the `T` it refers to, so it is
    /// wrapped in an explicit dereference. A `*T` must be dereferenced with
    /// an explicit `*p`.
    pub(super) fn check_place(&mut self, expr: &ast::Expr) -> SemanticResult<Expr> {
        let span = expr.span;
        match &expr.kind {
            Ast::Ident(name) => {
                let binding =
                    self.scopes
                        .lookup(name)
                        .ok_or_else(|| SemanticError::UndefinedVariable {
                            name: name.clone(),
                            span,
                        })?;
                let storage = Expr {
                    kind: ExprKind::Binding(binding),
                    ty: self.bindings[binding].ty.clone(),
                    span,
                };
                Ok(match &storage.ty {
                    Type::Reference(pointee) => Expr {
                        ty: (**pointee).clone(),
                        kind: ExprKind::Deref(Box::new(storage)),
                        span,
                    },
                    _ => storage,
                })
            }
            Ast::Deref(operand) => {
                let pointer = self.check_expr(operand, None)?;
                let pointee = pointer.ty.pointee().cloned().ok_or_else(|| {
                    SemanticError::CannotDereference {
                        found: pointer.ty.to_string(),
                        span,
                    }
                })?;
                Ok(Expr {
                    kind: ExprKind::Deref(Box::new(pointer)),
                    ty: pointee,
                    span,
                })
            }
            _ => Err(SemanticError::NotAPlaceExpression { span }),
        }
    }

    /// Builds an integer literal of type `ty`, checking that it fits.
    pub(super) fn int_literal(&self, value: BigInt, ty: Type, span: Span) -> SemanticResult<Expr> {
        let fits = self
            .target()
            .int_bounds(&ty)
            .is_some_and(|(min, max)| min <= value && value <= max);
        if !fits {
            return Err(SemanticError::LiteralOutOfRange { value, ty, span });
        }
        Ok(Expr {
            kind: ExprKind::Literal(Literal::Int(value)),
            ty,
            span,
        })
    }

    fn address_literal(
        &self,
        value: &BigInt,
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        let ty = match expected {
            Some(ty) if ty.is_indirect() => ty.clone(),
            _ => return Err(SemanticError::AddressLiteralWithoutPointerType { span }),
        };
        let limit = BigInt::from(1u8) << self.target().pointer_width();
        if value.sign() == num_bigint::Sign::Minus || value >= &limit {
            return Err(SemanticError::LiteralOutOfRange {
                value: value.clone(),
                ty,
                span,
            });
        }
        Ok(Expr {
            kind: ExprKind::Literal(Literal::Address(value.clone())),
            ty,
            span,
        })
    }

    fn check_unary(
        &mut self,
        op: &ast::UnaryOp,
        operand: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        // `-literal` is a negative literal, so that e.g. `-128` fits `i8`.
        if *op == ast::UnaryOp::Neg {
            match &operand.kind {
                Ast::Int(value) => {
                    return self.int_literal(-value, LiteralClass::Int.ty_for(expected), span);
                }
                Ast::Float(value) => return Ok(float_literal(-value, expected, span)),
                _ => {}
            }
        }

        let operand = self.check_expr(operand, expected)?;
        let ty = &operand.ty;
        let (mir_op, symbol, requirement, valid) = match op {
            ast::UnaryOp::Neg => (
                UnOp::Neg,
                "-",
                "numeric type",
                ty.is_integer() || ty.is_float(),
            ),
            ast::UnaryOp::Not => (UnOp::Not, "!", "`bool`", ty.is_bool()),
            ast::UnaryOp::BitwiseNot => (UnOp::Not, "~", "integer type", ty.is_integer()),
        };
        if !valid {
            return Err(SemanticError::InvalidOperands {
                op: symbol.to_owned(),
                detail: format!("expected {requirement}, found `{}`", operand.ty),
                span,
            });
        }
        Ok(Expr {
            ty: operand.ty.clone(),
            kind: ExprKind::Unary {
                op: mir_op,
                operand: Box::new(operand),
            },
            span,
        })
    }

    fn check_call(&mut self, name: &str, args: &[ast::Expr], span: Span) -> SemanticResult<Expr> {
        let globals = self.globals;
        let callee =
            *globals
                .by_name
                .get(name)
                .ok_or_else(|| SemanticError::UndefinedFunction {
                    name: name.to_owned(),
                    span,
                })?;
        let signature = &globals.signatures[callee];
        if args.len() != signature.params.len() {
            return Err(SemanticError::ArgumentCountMismatch {
                name: name.to_owned(),
                expected: signature.params.len(),
                got: args.len(),
                span,
            });
        }
        let args = args
            .iter()
            .zip(&signature.params)
            .map(|(arg, param_ty)| self.check_expr_has_type(arg, param_ty))
            .collect::<SemanticResult<_>>()?;
        Ok(Expr {
            kind: ExprKind::Call { callee, args },
            ty: signature.return_ty.clone(),
            span,
        })
    }
}

fn float_literal(value: f64, expected: Option<&Type>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Float(value)),
        ty: LiteralClass::Float.ty_for(expected),
        span,
    }
}
