//! Lowering of blocks and statements.

use crate::middle::thir::{Block, Expr, Stmt, StmtKind};

use super::super::{BasicBlock, Operand, Place, Rvalue};
use super::expr::has_side_effects;
use super::{Builder, Flow, LowerResult};

impl Builder<'_> {
    /// Lowers `body` as a lexical scope.
    ///
    /// Statements after one that diverges are unreachable and not lowered.
    pub(super) fn lower_block(&mut self, block: BasicBlock, body: &Block) -> LowerResult<()> {
        self.push_scope(body.span);
        let mut flow = Flow::Continue(block, ());
        for stmt in &body.stmts {
            let Flow::Continue(block, ()) = flow else {
                break;
            };
            flow = self.lower_stmt(block, stmt)?;
        }
        Ok(self.pop_scope(flow, body.span))
    }

    fn lower_stmt(&mut self, block: BasicBlock, stmt: &Stmt) -> LowerResult<()> {
        match &stmt.kind {
            StmtKind::Let { binding, init } => {
                let local = self.declare_binding(block, *binding)?;
                match init {
                    Some(init) => self.into(block, local.into(), init),
                    None => Ok(Flow::Continue(block, ())),
                }
            }
            StmtKind::Expr(expr) => {
                let (block, _) = unpack!(self.as_temp(block, expr));
                Ok(Flow::Continue(block, ()))
            }
            StmtKind::Assign { place, value } => {
                let (block, place) = unpack!(self.assignment_target(block, place, value));
                let (block, rvalue) = unpack!(self.as_rvalue(block, value));
                let source_info = self.source_info(stmt.span);
                self.cfg.push_assign(block, source_info, place, rvalue);
                Ok(Flow::Continue(block, ()))
            }
            StmtKind::CompoundAssign { op, place, value } => {
                let (block, place) = unpack!(self.assignment_target(block, place, value));
                let (block, value) = unpack!(self.as_operand(block, value));
                // The old value is read after `value` has been evaluated.
                let rvalue = Rvalue::binary(*op, Operand::Copy(place.clone()), value);
                let source_info = self.source_info(stmt.span);
                self.cfg.push_assign(block, source_info, place, rvalue);
                Ok(Flow::Continue(block, ()))
            }
            StmtKind::Return(value) => self.lower_return(block, value, stmt.span),
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => self.lower_if_stmt(block, condition, then_block, else_block.as_ref(), stmt.span),
            StmtKind::Break(value) => self.lower_break(block, value.as_ref(), stmt.span),
            StmtKind::Continue => self.lower_continue(block, stmt.span),
        }
    }

    /// Lowers the target of an assignment of `value`.
    fn assignment_target(
        &mut self,
        block: BasicBlock,
        place: &Expr,
        value: &Expr,
    ) -> LowerResult<Place> {
        if has_side_effects(value) {
            self.as_stable_place(block, place)
        } else {
            self.as_place(block, place)
        }
    }
}
