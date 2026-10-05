//! Checking of statements, blocks, and loops.

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, ExprKind as Ast, StmtKind as AstStmt};
use crate::middle::ops::BinOp;
use crate::middle::thir::{
    BindingKind, Block, ConditionPlacement, Expr, ExprKind, Literal, LoopCondition, Stmt, StmtKind,
};
use crate::source::Span;
use crate::types::Type;

use super::{FnCtxt, LoopFrame};

impl FnCtxt<'_, '_> {
    /// Checks `stmts` as one lexical scope.
    ///
    /// `fallback` is the span used if the block has no statements.
    pub(super) fn check_block(
        &mut self,
        stmts: &[ast::Stmt],
        fallback: Span,
    ) -> SemanticResult<Block> {
        self.scopes.push();
        let result = stmts
            .iter()
            .map(|stmt| self.check_stmt(stmt))
            .collect::<SemanticResult<_>>();
        self.scopes.pop();
        Ok(Block {
            stmts: result?,
            span: Self::block_span(stmts, fallback),
        })
    }

    fn check_stmt(&mut self, stmt: &ast::Stmt) -> SemanticResult<Stmt> {
        let span = stmt.span;
        let kind = match &stmt.kind {
            AstStmt::VarDecl(binding) => {
                let init = match &binding.default {
                    Some(init) => Some(self.check_initializer(binding, init)?),
                    None => None,
                };
                // Declared only after the initializer, which therefore
                // still sees any shadowed binding of the same name.
                let binding = self.declare(binding, BindingKind::Local);
                StmtKind::Let { binding, init }
            }
            AstStmt::Expr(expr) => StmtKind::Expr(self.check_expr(expr, None)?),
            AstStmt::Assign { target, value } => {
                let place = self.check_place(target)?;
                let value = self.check_expr_has_type(value, &place.ty)?;
                StmtKind::Assign { place, value }
            }
            AstStmt::CompoundAssign { target, op, value } => {
                let place = self.check_place(target)?;
                let (op, value) = self.check_compound(op, &place, value, span)?;
                StmtKind::CompoundAssign { op, place, value }
            }
            AstStmt::IncDec { target, op } => {
                let place = self.check_place(target)?;
                if !place.ty.is_integer() && !place.ty.is_float() {
                    return Err(SemanticError::InvalidOperands {
                        op: format!("{op:?}"),
                        detail: format!("expected numeric type, found `{}`", place.ty),
                        span,
                    });
                }
                let one = self.one(&place.ty, span)?;
                let op = match op {
                    ast::IncDecOp::Increment => BinOp::Add,
                    ast::IncDecOp::Decrement => BinOp::Sub,
                };
                StmtKind::CompoundAssign {
                    op,
                    place,
                    value: one,
                }
            }
            AstStmt::Return(value) => {
                let return_ty = self.return_ty.clone();
                StmtKind::Return(self.check_expr_has_type(value, &return_ty)?)
            }
            AstStmt::If {
                condition,
                then_branch,
                else_branch,
            } => StmtKind::If {
                condition: self.check_condition(condition)?,
                then_block: self.check_block(then_branch, span)?,
                else_block: match else_branch {
                    Some(else_branch) => Some(self.check_block(else_branch, span)?),
                    None => None,
                },
            },
            AstStmt::Break(value) => StmtKind::Break(self.check_break(value.as_deref(), span)?),
            AstStmt::Continue => {
                if self.loops.is_empty() {
                    return Err(SemanticError::ContinueOutsideLoop { span });
                }
                StmtKind::Continue
            }
        };
        Ok(Stmt { kind, span })
    }

    /// Checks the initializer of a variable declaration.
    fn check_initializer(
        &mut self,
        binding: &ast::Binding,
        init: &ast::Expr,
    ) -> SemanticResult<Expr> {
        let is_literal = match &init.kind {
            Ast::Int(_) => true,
            Ast::UnaryOp {
                op: ast::UnaryOp::Neg,
                operand,
            } => matches!(operand.kind, Ast::Int(_)),
            _ => false,
        };
        let init = self.check_expr_has_type(init, &binding.ty);
        // A literal initializer that does not fit names the variable.
        if let (true, Err(SemanticError::LiteralOutOfRange { value, ty, .. })) = (is_literal, &init)
        {
            return Err(SemanticError::ConstantOutOfRange {
                name: binding.name.clone().unwrap_or_else(|| "_".to_owned()),
                value: value.clone(),
                ty: ty.clone(),
                span: binding.span,
            });
        }
        init
    }

    /// The value `1` of the numeric type `ty`, for `++` and `--`.
    fn one(&self, ty: &Type, span: Span) -> SemanticResult<Expr> {
        if ty.is_float() {
            return Ok(Expr {
                kind: ExprKind::Literal(Literal::Float(1.0)),
                ty: ty.clone(),
                span,
            });
        }
        self.int_literal(1.into(), ty.clone(), span)
    }

    /// Checks `break value`, unifying its type with the innermost loop.
    ///
    /// The first `break` fixes the loop's type: from its value, or for a
    /// bare `break` from the context of the loop (`i64` without one).
    fn check_break(
        &mut self,
        value: Option<&ast::Expr>,
        span: Span,
    ) -> SemanticResult<Option<Expr>> {
        let Some(frame) = self.loops.last() else {
            return Err(SemanticError::BreakOutsideLoop { span });
        };
        let expected = frame.break_ty.clone().or_else(|| frame.contextual.clone());

        let value = match value {
            Some(value) => Some(self.check_expr(value, expected.as_ref())?),
            None => None,
        };
        let (ty, ty_span) = match &value {
            Some(value) => (value.ty.clone(), value.span),
            None => (expected.unwrap_or(Type::Int(64)), span),
        };

        let Some(frame) = self.loops.last_mut() else {
            return Err(SemanticError::BreakOutsideLoop { span });
        };
        match &frame.break_ty {
            None => frame.break_ty = Some(ty),
            Some(earlier) if *earlier == ty => {}
            Some(earlier) => {
                return Err(SemanticError::BreakTypeConflict {
                    earlier: earlier.to_string(),
                    found: ty.to_string(),
                    span: ty_span,
                });
            }
        }
        Ok(value)
    }

    /// Checks a loop expression. `condition` is `(condition, post, inverted)`
    /// for `while`/`until` loops and their `do` forms.
    ///
    /// The loop's type is fixed by its first `break`; without one, it is
    /// the type its context expects, or `i64`.
    pub(super) fn check_loop(
        &mut self,
        condition: Option<(&ast::Expr, bool, bool)>,
        body: &[ast::Stmt],
        expected: Option<&Type>,
        span: Span,
    ) -> SemanticResult<Expr> {
        let condition = match condition {
            Some((expr, post, inverted)) => Some(Box::new(LoopCondition {
                expr: self.check_condition(expr)?,
                placement: if post {
                    ConditionPlacement::After
                } else {
                    ConditionPlacement::Before
                },
                continue_if: !inverted,
            })),
            None => None,
        };

        self.loops.push(LoopFrame {
            contextual: expected.cloned(),
            break_ty: None,
        });
        let body = self.check_block(body, span);
        let frame = self.loops.pop();
        let body = body?;

        let ty = frame
            .and_then(|frame| frame.break_ty.or(frame.contextual))
            .unwrap_or(Type::Int(64));
        Ok(Expr {
            kind: ExprKind::Loop { condition, body },
            ty,
            span,
        })
    }
}
