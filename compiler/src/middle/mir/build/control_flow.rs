//! Lowering of control flow: `if`, loops, `break`, `continue`, and
//! `return`.
//!
//! Block layout of the three loop forms, after the destination has been
//! zero-initialized:
//!
//! ```text
//! loop { B }           while c { B }               do { B } while c
//!
//!   body: B              header: c                   body: B
//!     goto body            switch -> body | exit       goto latch
//!                        body: B                     latch: c
//!                          goto header                 switch -> body | exit
//! ```
//!
//! `continue` jumps to `body`, `header`, and `latch` respectively; `until`
//! loops swap the switch targets. Exit and latch blocks are only created
//! when something jumps to them, so a loop that is never left diverges.

use crate::middle::thir::{Block, ConditionPlacement, Expr, LoopCondition};
use crate::source::Span;
use crate::types::Type;

use super::super::{BasicBlock, Operand, Place, RETURN_PLACE, SwitchTargets, TerminatorKind};
use super::scope::LoopScope;
use super::{Builder, Flow, LowerErrorKind, LowerResult};

impl Builder<'_> {
    /// Lowers an `if` statement.
    pub(super) fn lower_if_stmt(
        &mut self,
        block: BasicBlock,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
        span: Span,
    ) -> LowerResult<()> {
        let Some((then_start, else_start)) = self.branch(block, condition, span)? else {
            return Ok(Flow::Diverge);
        };
        let then_flow = self.lower_block(then_start, then_block)?;
        let else_flow = match else_block {
            Some(else_block) => self.lower_block(else_start, else_block)?,
            None => Flow::Continue(else_start, ()),
        };
        Ok(self.join([then_flow, else_flow], span))
    }

    /// Lowers a conditional expression into `destination`.
    pub(super) fn lower_if_expr(
        &mut self,
        block: BasicBlock,
        destination: Place,
        condition: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
        span: Span,
    ) -> LowerResult<()> {
        let Some((then_start, else_start)) = self.branch(block, condition, span)? else {
            return Ok(Flow::Diverge);
        };
        let then_flow = self.into(then_start, destination.clone(), then_expr)?;
        let else_flow = self.into(else_start, destination, else_expr)?;
        Ok(self.join([then_flow, else_flow], span))
    }

    /// Evaluates `condition` and branches on it to two new blocks, returned
    /// as `(taken if true, taken if false)`; `None` if `condition` diverges.
    fn branch(
        &mut self,
        block: BasicBlock,
        condition: &Expr,
        span: Span,
    ) -> Result<Option<(BasicBlock, BasicBlock)>, LowerErrorKind> {
        let Flow::Continue(block, discr) = self.as_operand(block, condition)? else {
            return Ok(None);
        };
        let if_true = self.cfg.new_block();
        let if_false = self.cfg.new_block();
        self.switch_bool(block, discr, if_true, if_false, span);
        Ok(Some((if_true, if_false)))
    }

    /// Ends `block` with a branch on the `bool` operand `discr`.
    fn switch_bool(
        &mut self,
        block: BasicBlock,
        discr: Operand,
        if_true: BasicBlock,
        if_false: BasicBlock,
        span: Span,
    ) {
        let source_info = self.source_info(span);
        self.cfg.terminate(
            block,
            source_info,
            TerminatorKind::SwitchInt {
                discr,
                targets: SwitchTargets::bool(if_true, if_false),
            },
        );
    }

    /// Merges the paths that continue into one block.
    ///
    /// With a single continuing path no join block is needed; with none,
    /// the construct diverges.
    fn join<const N: usize>(&mut self, flows: [Flow<()>; N], span: Span) -> Flow<()> {
        let ends: Vec<BasicBlock> = flows
            .into_iter()
            .filter_map(|flow| match flow {
                Flow::Continue(block, ()) => Some(block),
                Flow::Diverge => None,
            })
            .collect();
        match ends.as_slice() {
            [] => Flow::Diverge,
            [end] => Flow::Continue(*end, ()),
            ends => {
                let join = self.cfg.new_block();
                let source_info = self.source_info(span);
                for &end in ends {
                    self.cfg.goto(end, source_info, join);
                }
                Flow::Continue(join, ())
            }
        }
    }

    /// Lowers a loop into `destination`, which holds the value of the
    /// `break` that leaves it, or zero.
    pub(super) fn lower_loop(
        &mut self,
        block: BasicBlock,
        destination: Place,
        ty: &Type,
        condition: Option<&LoopCondition>,
        body: &Block,
        span: Span,
    ) -> LowerResult<()> {
        let source_info = self.source_info(span);
        let zero = self.zero_rvalue(ty)?;
        self.cfg
            .push_assign(block, source_info, destination.clone(), zero);

        let start = self.cfg.new_block();
        self.cfg.goto(block, source_info, start);

        let frame = match condition {
            None => {
                self.push_loop(destination, Some(start), None);
                self.lower_loop_body(start, start, body)?
            }
            Some(condition) if condition.placement == ConditionPlacement::Before => {
                // `start` is the header testing the condition.
                let (test_end, discr) = unpack!(self.as_operand(start, &condition.expr));
                let body_start = self.cfg.new_block();
                let exit = self.cfg.new_block();
                self.switch_loop(test_end, discr, condition, body_start, exit, span);
                self.push_loop(destination, Some(start), Some(exit));
                self.lower_loop_body(body_start, start, body)?
            }
            Some(condition) => {
                // `start` is the body; the latch testing the condition is
                // the continue target, created if anything reaches it.
                self.push_loop(destination, None, None);
                let body_flow = self.lower_block(start, body)?;
                if let Flow::Continue(end, ()) = body_flow {
                    let (_, latch) = self.continue_target()?;
                    self.cfg.goto(end, source_info, latch);
                }
                let mut frame = self.pop_loop()?;
                if let Some(latch) = frame.continue_block
                    && let Flow::Continue(test_end, discr) =
                        self.as_operand(latch, &condition.expr)?
                {
                    let exit = *frame
                        .break_block
                        .get_or_insert_with(|| self.cfg.new_block());
                    self.switch_loop(test_end, discr, condition, start, exit, span);
                }
                frame
            }
        };

        Ok(match frame.break_block {
            Some(exit) => Flow::Continue(exit, ()),
            None => Flow::Diverge,
        })
    }

    /// Lowers the body of the innermost loop, which jumps back to `back_edge`
    /// when it completes, then leaves the loop.
    fn lower_loop_body(
        &mut self,
        start: BasicBlock,
        back_edge: BasicBlock,
        body: &Block,
    ) -> Result<LoopScope, LowerErrorKind> {
        if let Flow::Continue(end, ()) = self.lower_block(start, body)? {
            let source_info = self.source_info(body.span);
            self.cfg.goto(end, source_info, back_edge);
        }
        self.pop_loop()
    }

    /// Ends `block` with the test of a loop condition.
    fn switch_loop(
        &mut self,
        block: BasicBlock,
        discr: Operand,
        condition: &LoopCondition,
        body: BasicBlock,
        exit: BasicBlock,
        span: Span,
    ) {
        let (if_true, if_false) = if condition.continue_if {
            (body, exit)
        } else {
            (exit, body)
        };
        self.switch_bool(block, discr, if_true, if_false, span);
    }

    /// Lowers `break`, storing `value` as the loop's value.
    pub(super) fn lower_break(
        &mut self,
        block: BasicBlock,
        value: Option<&Expr>,
        span: Span,
    ) -> LowerResult<()> {
        let block = match value {
            Some(value) => {
                let destination = self
                    .innermost_loop(LowerErrorKind::BreakOutsideLoop)?
                    .destination
                    .clone();
                unpack!(self.into(block, destination, value)).0
            }
            None => block,
        };
        let (depth, target) = self.break_target()?;
        self.exit_scopes(block, depth, span);
        let source_info = self.source_info(span);
        self.cfg.goto(block, source_info, target);
        Ok(Flow::Diverge)
    }

    /// Lowers `continue`.
    pub(super) fn lower_continue(&mut self, block: BasicBlock, span: Span) -> LowerResult<()> {
        let (depth, target) = self.continue_target()?;
        self.exit_scopes(block, depth, span);
        let source_info = self.source_info(span);
        self.cfg.goto(block, source_info, target);
        Ok(Flow::Diverge)
    }

    /// Lowers `return value`.
    pub(super) fn lower_return(
        &mut self,
        block: BasicBlock,
        value: &Expr,
        span: Span,
    ) -> LowerResult<()> {
        let (block, ()) = unpack!(self.into(block, RETURN_PLACE.into(), value));
        self.exit_scopes(block, 0, span);
        let source_info = self.source_info(span);
        self.cfg
            .terminate(block, source_info, TerminatorKind::Return);
        Ok(Flow::Diverge)
    }
}
