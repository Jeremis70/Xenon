//! Checking of `for` and `repeat`.
//!
//! Both desugar to hidden temporaries and one counted THIR loop: a
//! condition tested before each iteration, and an increment that runs after
//! it, also on `continue`. Nothing below THIR knows about them.

use num_bigint::BigInt;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{self, ExprKind as Ast};
use crate::middle::ids::BindingId;
use crate::middle::ops::BinOp;
use crate::middle::thir::{
    Block, ConditionPlacement, Expr, ExprKind, LoopCondition, Stmt, StmtKind,
};
use crate::source::Span;
use crate::types::Type;

use super::expr::needs_context;
use super::operator::binary;
use super::{Expectation, FnCtxt, LoopFrame};

impl FnCtxt<'_, '_> {
    /// Checks `repeat count { body }`, appending what it desugars to:
    ///
    /// ```text
    /// let limit = count; let counter = 0;
    /// while counter < limit { body } step { counter += 1; }
    /// ```
    pub(super) fn check_repeat(
        &mut self,
        count: &ast::Expr,
        body: &[ast::Stmt],
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        if literal_value(count).is_some_and(|value| value < BigInt::ZERO) {
            return Err(SemanticError::NegativeRepeatCount { span: count.span });
        }
        let expected = needs_context(count).then_some(Type::USize);
        let limit = self.check_expr(count, expected.as_ref())?;
        if !limit.ty.is_integer() {
            return Err(SemanticError::NonIntegerRepeatCount {
                ty: limit.ty,
                span: count.span,
            });
        }
        let ty = limit.ty.clone();
        let limit = self.let_temp(limit, out);
        let zero = self.int_literal(BigInt::ZERO, ty, span)?;
        let counter = self.let_temp(zero, out);

        let condition = self.compare(BinOp::Lt, counter, limit, span);
        let increment = vec![self.advance(counter, &BigInt::from(1), false, span)];
        let stmt = self.counted_loop(condition, increment, body, span, |_, _| Ok(()))?;
        out.push(stmt);
        Ok(())
    }

    /// Builds the statement
    /// `while condition { prelude; body } step { increment }`.
    ///
    /// `prelude` appends the statements that open every iteration; it runs
    /// inside the scope of the body, so the names it declares are visible
    /// to `body` and to nothing after the loop.
    pub(super) fn counted_loop(
        &mut self,
        condition: Expr,
        increment: Vec<Stmt>,
        body: &[ast::Stmt],
        span: Span,
        prelude: impl FnOnce(&mut Self, &mut Vec<Stmt>) -> SemanticResult<()>,
    ) -> SemanticResult<Stmt> {
        self.loops.push(LoopFrame {
            contextual: Expectation::Unknown,
            break_ty: None,
        });
        self.scopes.push();
        let mut stmts = Vec::with_capacity(body.len());
        let result = prelude(self, &mut stmts).and_then(|()| {
            body.iter()
                .try_for_each(|stmt| self.check_stmt(stmt, &mut stmts))
        });
        self.scopes.pop();
        let frame = self.loops.pop();
        result?;

        // As for `while`: the loop's value is that of its first `break`.
        let ty = frame
            .and_then(|frame| frame.break_ty)
            .unwrap_or(Type::Int(64));
        if ty.contains_reference() {
            return Err(SemanticError::ReferenceLoop { ty, span });
        }
        let condition = Box::new(LoopCondition {
            expr: condition,
            placement: ConditionPlacement::Before,
            continue_if: true,
        });
        let kind = ExprKind::Loop {
            condition: Some(condition),
            increment: Some(Block {
                stmts: increment,
                span,
            }),
            body: Block { stmts, span },
        };
        Ok(Stmt {
            kind: StmtKind::Expr(Expr { kind, ty, span }),
            span,
        })
    }

    /// Declares a hidden temporary initialized with `init`.
    pub(super) fn let_temp(&mut self, init: Expr, out: &mut Vec<Stmt>) -> BindingId {
        let span = init.span;
        let binding = self.declare_temp(init.ty.clone(), span);
        out.push(Stmt {
            kind: StmtKind::Let {
                binding,
                init: Some(init),
            },
            span,
        });
        binding
    }

    /// The value of `binding`.
    pub(super) fn read(&self, binding: BindingId, span: Span) -> Expr {
        Expr {
            kind: ExprKind::Binding(binding),
            ty: self.bindings[binding].ty.clone(),
            span,
        }
    }

    /// `lhs op rhs` for two bindings of one integer type.
    pub(super) fn compare(&self, op: BinOp, lhs: BindingId, rhs: BindingId, span: Span) -> Expr {
        binary(
            op,
            Type::Bool,
            self.read(lhs, span),
            self.read(rhs, span),
            span,
        )
    }

    /// The statement moving `counter` by `magnitude`, down if `descending`.
    ///
    /// Callers only run it when the result fits the counter's type. In a
    /// type too narrow for `magnitude` itself (`1` in an `i1`) the step is
    /// written with the negated literal, and where even that does not fit
    /// no step can ever be taken, so the statement leaves the loop.
    pub(super) fn advance(
        &self,
        counter: BindingId,
        magnitude: &BigInt,
        descending: bool,
        span: Span,
    ) -> Stmt {
        let ty = &self.bindings[counter].ty;
        let (forward, backward) = if descending {
            (BinOp::Sub, BinOp::Add)
        } else {
            (BinOp::Add, BinOp::Sub)
        };
        let step = self
            .int_literal(magnitude.clone(), ty.clone(), span)
            .map(|value| (forward, value))
            .or_else(|_| {
                self.int_literal(-magnitude, ty.clone(), span)
                    .map(|value| (backward, value))
            });
        let kind = match step {
            Ok((op, value)) => StmtKind::CompoundAssign {
                op,
                place: self.read(counter, span),
                value,
            },
            Err(_) => StmtKind::Break(None),
        };
        Stmt { kind, span }
    }
}

/// The value of an integer literal, possibly negated: `5` or `-5`.
pub(super) fn literal_value(expr: &ast::Expr) -> Option<BigInt> {
    match &expr.kind {
        Ast::Int(value) => Some(value.clone()),
        Ast::UnaryOp {
            op: ast::UnaryOp::Neg,
            operand,
        } => match &operand.kind {
            Ast::Int(value) => Some(-value),
            _ => None,
        },
        _ => None,
    }
}
