//! Lowering of expressions in each category: place, operand, rvalue, and
//! into a destination.

use crate::middle::ops::BinOp;
use crate::middle::thir::visit::{Visitor, walk_expr};
use crate::middle::thir::{Expr, ExprKind, Literal, LogicalOp};

use super::super::{BasicBlock, Constant, Local, Operand, Place, Rvalue, TerminatorKind};
use super::{Builder, Flow, LowerResult};

impl Builder<'_> {
    /// Lowers `expr` to the place it denotes.
    ///
    /// Value expressions are stored in a temporary, whose place is returned.
    pub(super) fn as_place(&mut self, block: BasicBlock, expr: &Expr) -> LowerResult<Place> {
        match &expr.kind {
            ExprKind::Binding(binding) => {
                Ok(Flow::Continue(block, self.local_of(*binding)?.into()))
            }
            ExprKind::Deref(pointer) if pointer.is_place() => {
                let (block, place) = unpack!(self.as_place(block, pointer));
                Ok(Flow::Continue(block, place.deref()))
            }
            ExprKind::Deref(pointer) => {
                let (block, temp) = unpack!(self.as_temp(block, pointer));
                Ok(Flow::Continue(block, Place::from(temp).deref()))
            }
            _ => {
                let (block, temp) = unpack!(self.as_temp(block, expr));
                Ok(Flow::Continue(block, temp.into()))
            }
        }
    }

    /// Lowers `expr` to a place that later evaluation cannot move: the
    /// pointer of a dereference is copied into a temporary first.
    ///
    /// Used for assignment targets whose value has side effects, so that
    /// `*p = f()` stores through the `p` from before the call.
    pub(super) fn as_stable_place(&mut self, block: BasicBlock, expr: &Expr) -> LowerResult<Place> {
        match &expr.kind {
            ExprKind::Deref(pointer) => {
                let (block, temp) = unpack!(self.as_temp(block, pointer));
                Ok(Flow::Continue(block, Place::from(temp).deref()))
            }
            _ => self.as_place(block, expr),
        }
    }

    /// Lowers `expr` to an operand: a constant, a copy of a place, or a
    /// copy of a temporary holding the value.
    pub(super) fn as_operand(&mut self, block: BasicBlock, expr: &Expr) -> LowerResult<Operand> {
        match &expr.kind {
            ExprKind::Literal(literal) => Ok(Flow::Continue(
                block,
                Operand::constant(constant(literal, expr)),
            )),
            ExprKind::Binding(_) | ExprKind::Deref(_) => {
                let (block, place) = unpack!(self.as_place(block, expr));
                Ok(Flow::Continue(block, Operand::Copy(place)))
            }
            _ => {
                let (block, temp) = unpack!(self.as_temp(block, expr));
                Ok(Flow::Continue(block, Operand::Copy(temp.into())))
            }
        }
    }

    /// Lowers `exprs` to operands, left to right.
    pub(super) fn as_operands<'e>(
        &mut self,
        mut block: BasicBlock,
        exprs: impl IntoIterator<Item = &'e Expr>,
    ) -> LowerResult<Vec<Operand>> {
        let exprs: Vec<&Expr> = exprs.into_iter().collect();
        // `effects_after[i]`: whether an operand after `i` has side effects.
        let mut effects_after = vec![false; exprs.len()];
        for index in (1..exprs.len()).rev() {
            effects_after[index - 1] = effects_after[index] || has_side_effects(exprs[index]);
        }

        let mut operands = Vec::with_capacity(exprs.len());
        for (expr, effects_after) in exprs.into_iter().zip(effects_after) {
            let (next, operand) = unpack!(self.as_ordered_operand(block, expr, effects_after));
            block = next;
            operands.push(operand);
        }
        Ok(Flow::Continue(block, operands))
    }

    /// Lowers `expr` to an operand evaluated before other operands.
    ///
    /// If `effects_after` (an operand evaluated later has side effects), a
    /// place is read into a temporary immediately, so that it observes the
    /// state of its own turn rather than the state after those effects.
    fn as_ordered_operand(
        &mut self,
        block: BasicBlock,
        expr: &Expr,
        effects_after: bool,
    ) -> LowerResult<Operand> {
        if effects_after && expr.is_place() {
            let (block, temp) = unpack!(self.as_temp(block, expr));
            return Ok(Flow::Continue(block, Operand::Copy(temp.into())));
        }
        self.as_operand(block, expr)
    }

    /// Lowers `expr` to a single computation.
    pub(super) fn as_rvalue(&mut self, block: BasicBlock, expr: &Expr) -> LowerResult<Rvalue> {
        let (block, rvalue) = match &expr.kind {
            ExprKind::Literal(_) | ExprKind::Binding(_) | ExprKind::Deref(_) => {
                let (block, operand) = unpack!(self.as_operand(block, expr));
                (block, Rvalue::Use(operand))
            }
            ExprKind::Unary { op, operand } => {
                let (block, operand) = unpack!(self.as_operand(block, operand));
                (block, Rvalue::UnaryOp(*op, operand))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                unpack!(self.binary_rvalue(block, *op, lhs, rhs))
            }
            ExprKind::Logical { op, lhs, rhs } => {
                unpack!(self.binary_rvalue(block, eager_logical_op(*op), lhs, rhs))
            }
            ExprKind::Cast { kind, operand } => {
                let (block, operand) = unpack!(self.as_operand(block, operand));
                (block, Rvalue::Cast(*kind, operand, expr.ty.clone()))
            }
            ExprKind::AddressOf { kind, place } => {
                let (block, place) = unpack!(self.as_place(block, place));
                (block, Rvalue::AddressOf(*kind, place))
            }
            ExprKind::Call { .. } | ExprKind::If { .. } | ExprKind::Loop { .. } => {
                let (block, temp) = unpack!(self.as_temp(block, expr));
                (block, Rvalue::Use(Operand::Copy(temp.into())))
            }
        };
        Ok(Flow::Continue(block, rvalue))
    }

    fn binary_rvalue(
        &mut self,
        block: BasicBlock,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
    ) -> LowerResult<Rvalue> {
        let (block, lhs) = unpack!(self.as_ordered_operand(block, lhs, has_side_effects(rhs)));
        let (block, rhs) = unpack!(self.as_operand(block, rhs));
        Ok(Flow::Continue(
            block,
            Rvalue::BinaryOp(op, Box::new((lhs, rhs))),
        ))
    }

    /// Lowers `expr` into a new temporary.
    pub(super) fn as_temp(&mut self, block: BasicBlock, expr: &Expr) -> LowerResult<Local> {
        let temp = self.cfg.new_temp(expr.ty.clone(), expr.span);
        let (block, ()) = unpack!(self.into(block, temp.into(), expr));
        Ok(Flow::Continue(block, temp))
    }

    /// Lowers `expr`, writing its value into `destination`.
    ///
    /// `destination` must be fresh: nothing in `expr` may read it, because
    /// conditional expressions and loops write it before they finish.
    pub(super) fn into(
        &mut self,
        block: BasicBlock,
        destination: Place,
        expr: &Expr,
    ) -> LowerResult<()> {
        match &expr.kind {
            ExprKind::Call { callee, args } => {
                let (block, args) = unpack!(self.as_operands(block, args));
                let next = self.cfg.new_block();
                let source_info = self.source_info(expr.span);
                self.cfg.terminate(
                    block,
                    source_info,
                    TerminatorKind::Call {
                        func: *callee,
                        args,
                        destination,
                        target: Some(next),
                    },
                );
                Ok(Flow::Continue(next, ()))
            }
            ExprKind::If {
                condition,
                then_expr,
                else_expr,
            } => self.lower_if_expr(
                block,
                destination,
                condition,
                then_expr,
                else_expr,
                expr.span,
            ),
            ExprKind::Loop { condition, body } => self.lower_loop(
                block,
                destination,
                &expr.ty,
                condition.as_deref(),
                body,
                expr.span,
            ),
            _ => {
                let (block, rvalue) = unpack!(self.as_rvalue(block, expr));
                let source_info = self.source_info(expr.span);
                self.cfg
                    .push_assign(block, source_info, destination, rvalue);
                Ok(Flow::Continue(block, ()))
            }
        }
    }
}

/// Returns `true` if evaluating `expr` may change the value of a place:
/// it contains a call or a loop (whose body may assign).
pub(super) fn has_side_effects(expr: &Expr) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'thir> Visitor<'thir> for Finder {
        fn visit_expr(&mut self, expr: &'thir Expr) {
            if matches!(expr.kind, ExprKind::Call { .. } | ExprKind::Loop { .. }) {
                self.found = true;
            } else if !self.found {
                walk_expr(self, expr);
            }
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

/// The MIR operator of an eager logical operator on `bool`.
fn eager_logical_op(op: LogicalOp) -> BinOp {
    match op {
        LogicalOp::And => BinOp::BitAnd,
        LogicalOp::Or => BinOp::BitOr,
        LogicalOp::Xor => BinOp::BitXor,
    }
}

/// The MIR constant of a literal expression.
fn constant(literal: &Literal, expr: &Expr) -> Constant {
    match literal {
        Literal::Bool(value) => Constant::bool(*value),
        Literal::Int(value) => Constant::int(value.clone(), expr.ty.clone()),
        Literal::Float(value) => Constant::float(*value, expr.ty.clone()),
        Literal::Address(value) => Constant::address(value.clone(), expr.ty.clone()),
    }
}
