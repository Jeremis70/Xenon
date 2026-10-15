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
use crate::middle::mir::syntax::{BinOp, Constant, Operand, Rvalue, StatementKind, TerminatorKind};
use crate::middle::mir::typing::{self, HasLocalDecls, TypingError};

/// Replaces every rvalue computing on a zero-sized integer by its result,
/// and every switch on one by a jump to the target for `0`.
pub(super) fn erase_zero_sized_computation(body: &mut Body) -> Result<(), TypingError> {
    let decls = body.local_decls().clone();
    for block in body.basic_blocks_mut().iter_mut() {
        for statement in &mut block.statements {
            if let StatementKind::Assign(assign) = &mut statement.kind
                && typing::computes_on_zero_sized(&assign.1, &decls)?
            {
                assign.1 = Rvalue::Use(zero_sized_result(&assign.1, &decls)?);
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
fn zero_sized_result(rvalue: &Rvalue, decls: &impl HasLocalDecls) -> Result<Operand, TypingError> {
    let ty = typing::rvalue_ty(rvalue, decls)?;
    let constant = match rvalue {
        // Only the amount is zero-sized: a shift by `0`.
        Rvalue::BinaryOp(op, operands) if op.is_shift() && !ty.is_zero_sized() => {
            return Ok(operands.0.clone());
        }
        // `0` compared with `0`.
        Rvalue::BinaryOp(op, _) if op.is_comparison() => {
            Constant::bool(matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge))
        }
        // `0 op 0` never overflows.
        Rvalue::Overflows(..) => Constant::bool(false),
        // Any other operation on zeros, or cast to or from a zero-sized
        // integer, yields zero. A division by zero never gets here: its
        // guard traps first.
        _ => Constant::zero(&ty).expect("operations on integers produce numbers"),
    };
    Ok(Operand::constant(constant))
}
