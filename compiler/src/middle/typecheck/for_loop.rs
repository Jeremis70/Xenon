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
    BindingKind, Block, ConditionPlacement, Expr, ExprKind, LoopCondition, Stmt, StmtKind,
};
use crate::source::Span;
use crate::types::{MAX_INT_WIDTH, Type};

use super::expr::needs_context;
use super::operator::{binary, widen};
use super::{Expectation, FnCtxt, LoopFrame};

impl FnCtxt<'_, '_> {
    /// Checks a `for` loop, appending what it desugars to.
    pub(super) fn check_for(
        &mut self,
        header: &ast::ForHeader,
        body: &[ast::Stmt],
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        if !header.clauses.is_empty() {
            return Err(SemanticError::NotYetSupported {
                what: "`with` and `where`",
                span,
            });
        }
        match &header.iterable {
            ast::ForIterable::Range(range) => self.check_for_range(header, range, body, span, out),
            ast::ForIterable::Value(value) => Err(SemanticError::NotYetSupported {
                what: "`for` over a value",
                span: value.span,
            }),
        }
    }

    /// Checks a `for` loop over a range:
    ///
    /// ```text
    /// let counter = start; let end = end;
    /// while counter < end { let x = counter; body } step { counter += 1; }
    /// ```
    ///
    /// The condition and the increment depend on the direction and on
    /// whether the end is included (see [`Self::range_increment`]). The
    /// increment never computes a value past the end, so no valid range can
    /// overflow its type.
    fn check_for_range(
        &mut self,
        header: &ast::ForHeader,
        range: &ast::RangeExpr,
        body: &[ast::Stmt],
        span: Span,
        out: &mut Vec<Stmt>,
    ) -> SemanticResult<()> {
        let direction = range_direction(range)?;
        let (start, end) = self.check_range_bounds(header, range)?;
        let ty = start.ty.clone();
        let counter = self.let_temp(start, out);
        let end = self.let_temp(end, out);

        let op = match (direction.descending, range.inclusive) {
            (false, false) => BinOp::Lt,
            (false, true) => BinOp::Le,
            (true, false) => BinOp::Gt,
            (true, true) => BinOp::Ge,
        };
        let condition = self.compare(op, counter, end, span);
        let increment = self.range_increment(counter, end, range, &direction, span)?;
        let stmt = self.counted_loop(condition, increment, body, span, |this, stmts| {
            this.declare_loop_variable(header, ty, counter, stmts);
            Ok(())
        })?;
        out.push(stmt);
        Ok(())
    }

    /// Checks the bounds of `range`, which share one integer type: the
    /// written type of the loop variable, else the type of whichever bound
    /// has one of its own.
    fn check_range_bounds(
        &mut self,
        header: &ast::ForHeader,
        range: &ast::RangeExpr,
    ) -> SemanticResult<(Expr, Expr)> {
        let (start, end) = (&*range.start, &*range.end);
        let non_integer = |ty: Type, span: Span| {
            if ty.is_integer() {
                Ok(ty)
            } else {
                Err(SemanticError::NonIntegerRange { ty, span })
            }
        };
        if let Some(written) = &header.ty {
            let ty = non_integer(self.globals.resolve(written)?, written.span)?;
            let start = self.check_expr_has_type(start, &ty)?;
            let end = self.check_expr_has_type(end, &ty)?;
            return Ok((start, end));
        }
        match (needs_context(start), needs_context(end)) {
            (true, true) => Err(SemanticError::RangeTypeUnknown {
                span: start.span.to(end.span),
            }),
            (true, false) => {
                let end_expr = self.check_expr(end, None)?;
                let ty = non_integer(end_expr.ty.clone(), end.span)?;
                let start = self.check_expr_has_type(start, &ty)?;
                Ok((start, end_expr))
            }
            (false, _) => {
                let start_expr = self.check_expr(start, None)?;
                let ty = non_integer(start_expr.ty.clone(), start.span)?;
                let end = self.check_expr_has_type(end, &ty)?;
                Ok((start_expr, end))
            }
        }
    }

    /// The statements that move `counter` to the next value of `range`, or
    /// leave the loop when there is none.
    ///
    /// | end      | increment                                   |
    /// |----------|---------------------------------------------|
    /// | `until`  | `counter += 1`                              |
    /// | `to`     | `if counter == end { break; } counter += 1` |
    ///
    /// The loop condition already keeps an `until` counter short of the
    /// end. A `to` counter may sit on the last value of its type, so the
    /// step is only taken when another value follows.
    ///
    /// A step `m` larger than one could jump past the end and out of the
    /// type, so the distance left is tested first (`end - counter` going
    /// up, `counter - end` going down):
    ///
    /// | end      | increment                                          |
    /// |----------|----------------------------------------------------|
    /// | `until`  | `if distance <= m { break; } counter += m`         |
    /// | `to`     | `if distance < m { break; } counter += m`          |
    ///
    /// The distance is never negative, because the loop condition held. In
    /// a signed type it may still not fit (`127 - -128`), so there it is
    /// computed one bit wider.
    fn range_increment(
        &mut self,
        counter: BindingId,
        end: BindingId,
        range: &ast::RangeExpr,
        direction: &Direction,
        span: Span,
    ) -> SemanticResult<Vec<Stmt>> {
        let mut increment = Vec::with_capacity(2);
        if direction.magnitude != BigInt::from(1) {
            let step_span = range.step.as_ref().map_or(span, |step| step.span);
            let ty = self.bindings[counter].ty.clone();
            // The step is also added to the counter, so it must fit.
            self.int_literal(direction.magnitude.clone(), ty.clone(), step_span)?;
            let wide = if ty.is_signed_integer() {
                match self.target().int_width(&ty) {
                    Some(width) if width < MAX_INT_WIDTH => Type::Int(width + 1),
                    _ => {
                        return Err(SemanticError::RangeStepUnsupported {
                            ty,
                            span: step_span,
                        });
                    }
                }
            } else {
                ty
            };
            let (far, near) = if direction.descending {
                (counter, end)
            } else {
                (end, counter)
            };
            let distance = binary(
                BinOp::Sub,
                wide.clone(),
                widen(self.read(far, span), &wide),
                widen(self.read(near, span), &wide),
                span,
            );
            let step = self.int_literal(direction.magnitude.clone(), wide, step_span)?;
            let op = if range.inclusive {
                BinOp::Lt
            } else {
                BinOp::Le
            };
            increment.push(break_if(binary(op, Type::Bool, distance, step, span), span));
        } else if range.inclusive {
            increment.push(break_if(self.compare(BinOp::Eq, counter, end, span), span));
        }
        increment.push(self.advance(counter, &direction.magnitude, direction.descending, span));
        Ok(increment)
    }

    /// Declares the loop variable of `header`, unless it is `_`, as a copy
    /// of `value` made at the start of each iteration.
    fn declare_loop_variable(
        &mut self,
        header: &ast::ForHeader,
        ty: Type,
        value: BindingId,
        stmts: &mut Vec<Stmt>,
    ) {
        if header.name.is_none() {
            return;
        }
        let span = header.name_span;
        let init = self.read(value, span);
        let binding = self.declare_named(header.name.clone(), ty, BindingKind::LoopName, span);
        stmts.push(Stmt {
            kind: StmtKind::Let {
                binding,
                init: Some(init),
            },
            span,
        });
    }

    /// Checks the target of an assignment, which cannot be a loop variable
    /// or a `with` counter, nor a part of one.
    pub(super) fn check_assign_target(&mut self, target: &ast::Expr) -> SemanticResult<Expr> {
        let place = self.check_place(target)?;
        if let Some(binding) = root_binding(&place)
            && self.bindings[binding].kind == BindingKind::LoopName
        {
            return Err(SemanticError::AssignToLoopName {
                name: self.bindings[binding].name.clone().unwrap_or_default(),
                span: target.span,
            });
        }
        Ok(place)
    }

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

/// Which way a range runs, and by how much each step.
#[derive(Debug)]
pub(super) struct Direction {
    pub(super) descending: bool,
    /// The absolute value of the step; never zero.
    pub(super) magnitude: BigInt,
}

/// The direction of `range`, which is always known at compile time.
///
/// A written `by` gives it. Without one the range counts by one: down when
/// both bounds are literals and the start is past the end, up otherwise.
pub(super) fn range_direction(range: &ast::RangeExpr) -> SemanticResult<Direction> {
    if let Some(step) = &range.step {
        if step.magnitude == BigInt::ZERO {
            return Err(SemanticError::ZeroRangeStep { span: step.span });
        }
        return Ok(Direction {
            descending: step.negative,
            magnitude: step.magnitude.clone(),
        });
    }
    let descending = match (literal_value(&range.start), literal_value(&range.end)) {
        (Some(start), Some(end)) => start > end,
        _ => false,
    };
    Ok(Direction {
        descending,
        magnitude: BigInt::from(1),
    })
}

/// `if condition { break; }`
fn break_if(condition: Expr, span: Span) -> Stmt {
    let leave = Stmt {
        kind: StmtKind::Break(None),
        span,
    };
    Stmt {
        kind: StmtKind::If {
            condition,
            then_block: Block {
                stmts: vec![leave],
                span,
            },
            else_block: None,
        },
        span,
    }
}

/// The binding a place is, or is a part of. A place reached through a
/// pointer or a reference is not a part of the binding holding it.
fn root_binding(place: &Expr) -> Option<BindingId> {
    match &place.kind {
        ExprKind::Binding(binding) => Some(*binding),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => root_binding(base),
        _ => None,
    }
}
