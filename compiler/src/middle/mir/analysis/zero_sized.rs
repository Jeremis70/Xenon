//! Erasure of computation on zero-sized integers.
//!
//! `i0` and `u0` hold only `0` and have no bits to compute on, so every
//! operation reading or producing one has a result known in advance. This
//! pass replaces each such operation by that result, so Runtime MIR only
//! stores, copies, passes, and returns zero-sized values.
//!
//! It runs after runtime checks are inserted, so the guards those checks add
//! are folded too: a division by a zero-sized divisor always traps, and so
//! does any shift of a zero-sized value.

use num_bigint::BigInt;

use crate::middle::mir::body::Body;
use crate::middle::mir::interpret::{EvalResult, eval_rvalue, known_operand};
use crate::middle::mir::syntax::{Constant, Operand, Rvalue, StatementKind, TerminatorKind};
use crate::middle::mir::typing::{self, HasLocalDecls, TypingError};
use crate::middle::target::TargetSpec;

/// Replaces every rvalue computing on a zero-sized integer by its result,
/// and every switch on one by a jump to the target for `0`.
pub(super) fn erase_zero_sized_computation(
    body: &mut Body,
    target: &TargetSpec,
) -> Result<(), TypingError> {
    let decls = body.local_decls().clone();
    for block in body.basic_blocks_mut().iter_mut() {
        for statement in &mut block.statements {
            if let StatementKind::Assign(assign) = &mut statement.kind
                && typing::computes_on_zero_sized(&assign.1, &decls)?
            {
                assign.1 = Rvalue::Use(zero_sized_result(&assign.1, &decls, target)?);
            }
        }
        if let TerminatorKind::SwitchInt { discr, targets } = &block.terminator.kind
            && typing::operand_ty(discr, &decls)?.is_zero_sized()
        {
            block.terminator.kind = TerminatorKind::Goto {
                target: targets.target_for(&BigInt::ZERO),
            };
        }
    }
    Ok(())
}

/// The value of an rvalue computing on zero-sized integers, all of which
/// hold `0`.
fn zero_sized_result(
    rvalue: &Rvalue,
    decls: &impl HasLocalDecls,
    target: &TargetSpec,
) -> Result<Operand, TypingError> {
    let ty = typing::rvalue_ty(rvalue, decls)?;
    // Only the amount is zero-sized: a shift by `0`.
    if let Rvalue::BinaryOp(op, operands) = rvalue
        && op.is_shift()
        && !ty.is_zero_sized()
    {
        return Ok(operands.0.clone());
    }
    let value_of = |operand: &Operand| known_operand(operand, decls, |_| None);
    let constant = match eval_rvalue(rvalue, decls, target, value_of)? {
        EvalResult::Value(constant) => constant,
        // A trapping operation never completes: its guard traps first, so
        // any value of its type will do. An unknown result is a zero-sized
        // value computed from unknown operands, or a conversion of `0`
        // the evaluator does not model, such as `u0` to a float.
        EvalResult::Trap(_) | EvalResult::Unknown => {
            Constant::zero(&ty).expect("operations on integers produce numbers")
        }
    };
    Ok(Operand::constant(constant))
}
