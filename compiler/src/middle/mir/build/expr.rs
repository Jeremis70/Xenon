//! Lowering of expressions in each category: place, operand, rvalue, and
//! into a destination.

use crate::middle::ops::{BinOp, UnOp};
use crate::middle::thir::visit::{Visitor, walk_expr};
use crate::middle::thir::{Expr, ExprKind, Literal, LogicalOp};
use crate::source::Span;
use crate::types::Type;

use super::super::{
    AggregateKind, AssertKind, BasicBlock, Constant, Local, Operand, Place, Rvalue, TerminatorKind,
};
use super::control_flow::LoopParts;
use super::{Builder, Flow, LowerErrorKind, LowerResult};

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
            // An element of a place is a place; an element of a value is
            // read from a temporary holding the value.
            ExprKind::Field { base, index } => {
                let (block, base) = unpack!(self.as_place(block, base));
                Ok(Flow::Continue(block, base.field(*index)))
            }
            ExprKind::Index { base, index } => self.index_place(block, base, index, expr.span),
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
            ExprKind::Field { base, index } => {
                let (block, base) = unpack!(self.as_stable_place(block, base));
                Ok(Flow::Continue(block, base.field(*index)))
            }
            ExprKind::Index { base, index } => self.index_place(block, base, index, expr.span),
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
            ExprKind::Binding(_)
            | ExprKind::Deref(_)
            | ExprKind::Field { .. }
            | ExprKind::Index { .. } => {
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
            ExprKind::Literal(_)
            | ExprKind::Binding(_)
            | ExprKind::Deref(_)
            | ExprKind::Field { .. }
            | ExprKind::Index { .. } => {
                let (block, operand) = unpack!(self.as_operand(block, expr));
                (block, Rvalue::Use(operand))
            }
            ExprKind::Tuple(elems) => {
                let (block, operands) = unpack!(self.as_operands(block, elems));
                (block, Rvalue::Aggregate(AggregateKind::Tuple, operands))
            }
            ExprKind::Array(elems) => {
                let (block, operands) = unpack!(self.as_operands(block, elems));
                let Type::Array(elem, len) = &expr.ty else {
                    return Err(LowerErrorKind::MalformedArray(expr.ty.clone()));
                };
                (
                    block,
                    Rvalue::Aggregate(AggregateKind::Array(elem.clone(), *len), operands),
                )
            }
            ExprKind::Struct { fields } => {
                // Evaluated as written, stored as declared.
                let values = fields.iter().map(|(_, value)| value);
                let (block, operands) = unpack!(self.as_operands(block, values));
                let mut ordered: Vec<Option<Operand>> = vec![None; operands.len()];
                for ((index, _), operand) in fields.iter().zip(operands) {
                    let slot = ordered
                        .get_mut(*index as usize)
                        .ok_or_else(|| LowerErrorKind::MalformedStruct(expr.ty.clone()))?;
                    *slot = Some(operand);
                }
                let operands = ordered
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| LowerErrorKind::MalformedStruct(expr.ty.clone()))?;
                let Type::Struct(struct_ty) = &expr.ty else {
                    return Err(LowerErrorKind::MalformedStruct(expr.ty.clone()));
                };
                (
                    block,
                    Rvalue::Aggregate(AggregateKind::Struct(struct_ty.clone()), operands),
                )
            }
            ExprKind::Binary { op, lhs, rhs } if lhs.ty.is_aggregate() => {
                unpack!(self.tuple_comparison(block, *op, lhs, rhs, expr.span))
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
        Ok(Flow::Continue(block, Rvalue::binary(op, lhs, rhs)))
    }

    fn index_place(
        &mut self,
        block: BasicBlock,
        base: &Expr,
        index: &Expr,
        span: Span,
    ) -> LowerResult<Place> {
        let Type::Array(_, len) = &base.ty else {
            return Err(LowerErrorKind::MalformedArray(base.ty.clone()));
        };
        let (block, base) = unpack!(self.as_stable_place(block, base));
        let (block, local) = unpack!(self.as_temp(block, index));
        let operand = Operand::Copy(local.into());
        let source_info = self.source_info(span);
        let upper = self.cfg.new_temp(Type::Bool, span);
        self.cfg.push_assign(
            block,
            source_info,
            upper.into(),
            Rvalue::binary(
                BinOp::Lt,
                operand.clone(),
                Operand::constant(Constant::int(*len, index.ty.clone())),
            ),
        );
        let mut valid = Operand::Copy(upper.into());
        if index.ty.is_signed_integer() {
            let lower = self.cfg.new_temp(Type::Bool, span);
            self.cfg.push_assign(
                block,
                source_info,
                lower.into(),
                Rvalue::binary(
                    BinOp::Ge,
                    operand,
                    Operand::constant(Constant::int(0, index.ty.clone())),
                ),
            );
            let both = self.cfg.new_temp(Type::Bool, span);
            self.cfg.push_assign(
                block,
                source_info,
                both.into(),
                Rvalue::binary(BinOp::BitAnd, valid, Operand::Copy(lower.into())),
            );
            valid = Operand::Copy(both.into());
        }
        if *len == 0 {
            valid = Operand::constant(Constant::bool(false));
        }
        let next = self.cfg.new_block();
        self.cfg.terminate(
            block,
            source_info,
            TerminatorKind::Assert {
                cond: valid,
                expected: true,
                kind: AssertKind::BoundsCheck,
                target: next,
            },
        );
        Ok(Flow::Continue(next, base.index(local)))
    }

    /// Lowers `lhs == rhs` or `lhs != rhs` on aggregates of the same type.
    ///
    /// Both sides are evaluated completely, left to right, before any
    /// element is compared: every call in them runs exactly once and in
    /// source order. The elements are then compared with plain reads of the
    /// two stored tuples, and the results combined with a non-short-circuit
    /// `&`; since those reads have no side effects, switching to a
    /// short-circuit combination later would not change what is evaluated.
    fn tuple_comparison(
        &mut self,
        block: BasicBlock,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
    ) -> LowerResult<Rvalue> {
        let (block, lhs_place) = unpack!(self.as_stored_tuple(block, lhs, has_side_effects(rhs)));
        let (block, rhs_place) = unpack!(self.as_stored_tuple(block, rhs, false));
        let (block, equal) =
            unpack!(self.elements_equal(block, lhs_place, rhs_place, &lhs.ty, span));
        Ok(Flow::Continue(
            block,
            match op {
                BinOp::Ne => Rvalue::UnaryOp(UnOp::Not, equal),
                _ => Rvalue::Use(equal),
            },
        ))
    }

    /// A place holding the fully evaluated aggregate `expr`: the place itself
    /// if it is one and no later evaluation can change it (`snapshot` is
    /// false), otherwise a temporary.
    ///
    /// A zero-sized value such as `()` is always copied: comparing it reads
    /// no element, so the copy is the only read definite-initialization
    /// analysis can see.
    fn as_stored_tuple(
        &mut self,
        block: BasicBlock,
        expr: &Expr,
        snapshot: bool,
    ) -> LowerResult<Place> {
        if expr.is_place() && !snapshot && !expr.ty.is_zero_sized(self.structs) {
            return self.as_place(block, expr);
        }
        let (block, temp) = unpack!(self.as_temp(block, expr));
        Ok(Flow::Continue(block, temp.into()))
    }

    /// Compares the values of type `ty` at `lhs` and `rhs`, element by
    /// element for tuples, structs, and arrays, and returns the `bool` result.
    fn elements_equal(
        &mut self,
        mut block: BasicBlock,
        lhs: Place,
        rhs: Place,
        ty: &Type,
        span: Span,
    ) -> LowerResult<Operand> {
        let source_info = self.source_info(span);
        // Values without data are always equal. Their fields are not
        // compared one by one: there can be an enormous tree of them.
        if ty.is_aggregate() && ty.is_zero_sized(self.structs) {
            return Ok(Flow::Continue(
                block,
                Operand::constant(Constant::bool(true)),
            ));
        }
        if let Type::Array(elem, len) = ty {
            return self.array_elements_equal(block, lhs, rhs, elem, *len, span);
        }
        let Some(elems) = self.structs.field_tys(ty) else {
            let equal = self.cfg.new_temp(Type::Bool, span);
            let rvalue = Rvalue::binary(BinOp::Eq, Operand::Copy(lhs), Operand::Copy(rhs));
            self.cfg
                .push_assign(block, source_info, equal.into(), rvalue);
            return Ok(Flow::Continue(block, Operand::Copy(equal.into())));
        };
        let mut all_equal: Option<Operand> = None;
        for (index, elem_ty) in elems.iter().enumerate() {
            let index = index as u32;
            let (next, equal) = unpack!(self.elements_equal(
                block,
                lhs.clone().field(index),
                rhs.clone().field(index),
                elem_ty,
                span,
            ));
            block = next;
            all_equal = Some(match all_equal {
                None => equal,
                Some(previous) => {
                    let both = self.cfg.new_temp(Type::Bool, span);
                    let rvalue = Rvalue::binary(BinOp::BitAnd, previous, equal);
                    self.cfg
                        .push_assign(block, source_info, both.into(), rvalue);
                    Operand::Copy(both.into())
                }
            });
        }
        // `()` has a single value, so two units are always equal.
        Ok(Flow::Continue(
            block,
            all_equal.unwrap_or_else(|| Operand::constant(Constant::bool(true))),
        ))
    }

    fn array_elements_equal(
        &mut self,
        block: BasicBlock,
        lhs: Place,
        rhs: Place,
        elem: &Type,
        len: u32,
        span: Span,
    ) -> LowerResult<Operand> {
        let source_info = self.source_info(span);
        let index = self.cfg.new_temp(Type::UInt(32), span);
        let all_equal = self.cfg.new_temp(Type::Bool, span);
        self.cfg.push_assign(
            block,
            source_info,
            index.into(),
            Rvalue::Use(Operand::constant(Constant::int(0, Type::UInt(32)))),
        );
        self.cfg.push_assign(
            block,
            source_info,
            all_equal.into(),
            Rvalue::Use(Operand::constant(Constant::bool(true))),
        );
        let head = self.cfg.new_block();
        let body = self.cfg.new_block();
        let exit = self.cfg.new_block();
        self.cfg
            .terminate(block, source_info, TerminatorKind::Goto { target: head });
        let in_bounds = self.cfg.new_temp(Type::Bool, span);
        self.cfg.push_assign(
            head,
            source_info,
            in_bounds.into(),
            Rvalue::binary(
                BinOp::Lt,
                Operand::Copy(index.into()),
                Operand::constant(Constant::int(len, Type::UInt(32))),
            ),
        );
        self.switch_bool(head, Operand::Copy(in_bounds.into()), body, exit, span);
        let (body, equal) =
            unpack!(self.elements_equal(body, lhs.index(index), rhs.index(index), elem, span));
        self.cfg.push_assign(
            body,
            source_info,
            all_equal.into(),
            Rvalue::binary(BinOp::BitAnd, Operand::Copy(all_equal.into()), equal),
        );
        // The body only runs for index < len, so even len == u32::MAX cannot overflow.
        self.cfg.push_assign(
            body,
            source_info,
            index.into(),
            Rvalue::binary(
                BinOp::Add,
                Operand::Copy(index.into()),
                Operand::constant(Constant::int(1, Type::UInt(32))),
            ),
        );
        self.cfg
            .terminate(body, source_info, TerminatorKind::Goto { target: head });
        Ok(Flow::Continue(exit, Operand::Copy(all_equal.into())))
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
            ExprKind::Loop {
                condition,
                increment,
                body,
            } => self.lower_loop(
                block,
                destination,
                &expr.ty,
                LoopParts {
                    condition: condition.as_deref(),
                    increment: increment.as_ref(),
                    body,
                },
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
