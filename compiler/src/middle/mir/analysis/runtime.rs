//! Required runtime-check normalization.
//!
//! Potentially trapping integer operations must have explicit guards before
//! the backend sees them. Operands are snapshotted before guards so neither
//! the checks nor the operation can observe a different value. The overflow
//! mode is supplied explicitly and is independent of optimization level.

use num_bigint::BigInt;
use thiserror::Error;

use crate::middle::mir::body::{BasicBlock, BasicBlockData, Body, MirPhase, SourceInfo};
use crate::middle::mir::program::MirProgram;
use crate::middle::mir::syntax::{
    AssertKind, BinOp, CastKind, Constant, Operand, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind, UnOp,
};
use crate::middle::mir::typing::{self, TypingError};
use crate::middle::mir::verify::{VerifyErrors, verify_program};
use crate::middle::target::TargetSpec;
use crate::types::Type;

/// Overflow behavior for integer addition, subtraction, and multiplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverflowMode {
    /// Trap when an integer operation exceeds the range of its type.
    Checked,
    /// Keep the low bits of the mathematical result.
    Wrapping,
}

/// Runtime semantics supplied to MIR normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeCheckPolicy {
    /// Behavior of integer addition, subtraction, and multiplication.
    pub overflow: OverflowMode,
}

/// An invalid input or operation during runtime-check normalization.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RuntimeCheckError {
    /// The pointer-sized integer types have no usable width.
    #[error("target pointer width must be non-zero")]
    InvalidTargetPointerWidth,

    /// Runtime normalization only accepts Checked MIR.
    #[error("expected Checked MIR, found phase `{phase}` in `{function}`")]
    InvalidPhase { function: String, phase: MirPhase },

    /// MIR was malformed before or after normalization.
    #[error("MIR verification failed: {0}")]
    InvalidMir(String),

    /// A type could not be derived from verified MIR.
    #[error(transparent)]
    Typing(#[from] TypingError),

    /// The selected target cannot represent an operation width.
    #[error("target has no integer width for shift operand type `{0}`")]
    MissingIntegerWidth(Type),

    /// The target has no range for a signed integer type.
    #[error("target has no integer range for signed type `{0}`")]
    MissingIntegerBounds(Type),
}

/// Makes all required integer arithmetic traps explicit and advances
/// Checked MIR to Runtime.
///
/// The transformation is transactional: a failed normalization leaves
/// `program` unchanged.
///
/// Integer division/remainder by zero and invalid shifts always trap.
/// Signed minimum divided or remaindered by `-1` also traps, rather than
/// relying on backend-specific poison or undefined behavior. Checked
/// overflow guards are added only when `policy.overflow` is `Checked`.
pub fn normalize_runtime_checks(
    program: &mut MirProgram,
    target: &TargetSpec,
    policy: RuntimeCheckPolicy,
) -> Result<(), RuntimeCheckError> {
    if target.pointer_width() == 0 {
        return Err(RuntimeCheckError::InvalidTargetPointerWidth);
    }
    let invalid_mir = |errors: VerifyErrors| RuntimeCheckError::InvalidMir(errors.to_string());
    verify_program(program, target).map_err(invalid_mir)?;
    if let Some(body) = program
        .bodies()
        .find(|body| body.phase() != MirPhase::Checked)
    {
        return Err(RuntimeCheckError::InvalidPhase {
            function: program.fn_name(body.def_id()),
            phase: body.phase(),
        });
    }

    let mut normalized = program.clone();
    for body in normalized.bodies_mut() {
        normalize_body(body, target, policy)?;
    }
    normalized
        .advance_phase(MirPhase::Runtime)
        .map_err(|error| RuntimeCheckError::InvalidPhase {
            function: program.fn_name(error.def_id),
            phase: error.from,
        })?;
    verify_program(&normalized, target).map_err(invalid_mir)?;
    *program = normalized;
    Ok(())
}

fn normalize_body(
    body: &mut Body,
    target: &TargetSpec,
    policy: RuntimeCheckPolicy,
) -> Result<(), RuntimeCheckError> {
    // Blocks appended while splitting contain no unchecked operations.
    for block in body.basic_blocks().indices() {
        let BasicBlockData {
            statements,
            terminator,
        } = body.basic_blocks()[block].clone();
        let mut guards = Guards {
            body: &mut *body,
            target,
            policy,
            block,
            statements: Vec::new(),
            source_info: terminator.source_info,
        };
        for mut statement in statements {
            match &mut statement.kind {
                StatementKind::Assign(assign) => {
                    guards.source_info = statement.source_info;
                    guards.guard_rvalue(&mut assign.1)?;
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => {}
            }
            guards.statements.push(statement);
        }
        guards.finish(terminator);
    }
    Ok(())
}

/// Inserts guards in front of the operations of one block, splitting it at
/// every guard.
struct Guards<'a> {
    body: &'a mut Body,
    target: &'a TargetSpec,
    policy: RuntimeCheckPolicy,
    /// The block being filled.
    block: BasicBlock,
    /// Statements of `block` so far.
    statements: Vec<Statement>,
    /// Source info of the guarded operation.
    source_info: SourceInfo,
}

impl Guards<'_> {
    /// Adds the guards `rvalue` needs, rewriting its operands to snapshots
    /// where a guard reads them too.
    fn guard_rvalue(&mut self, rvalue: &mut Rvalue) -> Result<(), RuntimeCheckError> {
        match rvalue {
            Rvalue::BinaryOp(op, operands) => {
                let (lhs, rhs) = &mut **operands;
                self.guard_binary(*op, lhs, rhs)
            }
            Rvalue::UnaryOp(UnOp::Neg, operand)
                if self.policy.overflow == OverflowMode::Checked =>
            {
                let ty = typing::operand_ty(operand, &*self.body)?.clone();
                if ty.is_integer() {
                    self.snapshot(operand, &ty);
                    let zero = Operand::constant(Constant::int(0, ty));
                    let overflows = self.temp(
                        Type::Bool,
                        Rvalue::overflows(BinOp::Sub, zero, operand.clone()),
                    );
                    self.trap_if(overflows, AssertKind::Overflow(BinOp::Sub));
                }
                Ok(())
            }
            Rvalue::Use(_)
            | Rvalue::UnaryOp(..)
            | Rvalue::Overflows(..)
            | Rvalue::Cast(..)
            | Rvalue::AddressOf(..) => Ok(()),
        }
    }

    fn guard_binary(
        &mut self,
        op: BinOp,
        lhs: &mut Operand,
        rhs: &mut Operand,
    ) -> Result<(), RuntimeCheckError> {
        let lhs_ty = typing::operand_ty(lhs, &*self.body)?.clone();
        let rhs_ty = typing::operand_ty(rhs, &*self.body)?.clone();
        if !lhs_ty.is_integer() {
            return Ok(());
        }
        let checks_overflow =
            self.policy.overflow == OverflowMode::Checked && op.is_overflow_checkable();
        let checks_divisor = matches!(op, BinOp::Div | BinOp::Rem);
        if !(checks_overflow || checks_divisor || op.is_shift()) {
            return Ok(());
        }

        self.snapshot(lhs, &lhs_ty);
        self.snapshot(rhs, &rhs_ty);

        if checks_overflow {
            let overflows = self.temp(Type::Bool, Rvalue::overflows(op, lhs.clone(), rhs.clone()));
            self.trap_if(overflows, AssertKind::Overflow(op));
        }

        if checks_divisor {
            let is_rem = op == BinOp::Rem;
            let zero = Operand::constant(Constant::int(0, rhs_ty.clone()));
            let is_zero = self.temp(Type::Bool, Rvalue::binary(BinOp::Eq, rhs.clone(), zero));
            let kind = if is_rem {
                AssertKind::RemainderByZero
            } else {
                AssertKind::DivisionByZero
            };
            self.trap_if(is_zero, kind);

            if lhs_ty.is_signed_integer() {
                let (min, _) = self
                    .target
                    .int_bounds(&lhs_ty)
                    .ok_or_else(|| RuntimeCheckError::MissingIntegerBounds(lhs_ty.clone()))?;
                let min = Operand::constant(Constant::int(min, lhs_ty.clone()));
                let minus_one = Operand::constant(Constant::int(-1, rhs_ty.clone()));
                let is_min = self.temp(Type::Bool, Rvalue::binary(BinOp::Eq, lhs.clone(), min));
                let is_minus_one = self.temp(
                    Type::Bool,
                    Rvalue::binary(BinOp::Eq, rhs.clone(), minus_one),
                );
                let overflows = self.temp(
                    Type::Bool,
                    Rvalue::binary(BinOp::BitAnd, is_min, is_minus_one),
                );
                let kind = if is_rem {
                    AssertKind::SignedRemainderOverflow
                } else {
                    AssertKind::SignedDivisionOverflow
                };
                self.trap_if(overflows, kind);
            }
        }

        if op.is_shift() {
            self.guard_shift_amount(&lhs_ty, rhs, &rhs_ty)?;
        }
        Ok(())
    }

    /// Traps unless `0 <= amount < bit width of value_ty`.
    ///
    /// The amount is compared as an unsigned integer wide enough to hold
    /// both the amount and the bit width, so a negative amount compares as
    /// too large.
    fn guard_shift_amount(
        &mut self,
        value_ty: &Type,
        amount: &Operand,
        amount_ty: &Type,
    ) -> Result<(), RuntimeCheckError> {
        let width_of = |ty: &Type| {
            self.target
                .int_width(ty)
                .ok_or_else(|| RuntimeCheckError::MissingIntegerWidth(ty.clone()))
        };
        let value_width = width_of(value_ty)?;
        let amount_width = width_of(amount_ty)?;
        let limit_width = u32::BITS - value_width.leading_zeros();
        let comparison_ty = Type::UInt(amount_width.max(limit_width));

        let amount = if *amount_ty == comparison_ty {
            amount.clone()
        } else {
            let cast = Rvalue::Cast(CastKind::IntToInt, amount.clone(), comparison_ty.clone());
            self.temp(comparison_ty.clone(), cast)
        };
        let limit = Operand::constant(Constant::int(BigInt::from(value_width), comparison_ty));
        let too_large = self.temp(Type::Bool, Rvalue::binary(BinOp::Ge, amount, limit));
        self.trap_if(too_large, AssertKind::ShiftOutOfRange);
        Ok(())
    }

    /// Evaluates `rvalue` into a fresh temporary of type `ty`.
    fn temp(&mut self, ty: Type, rvalue: Rvalue) -> Operand {
        let temp = self.body.new_temp(ty, self.source_info.span);
        self.statements
            .push(Statement::assign(self.source_info, temp.into(), rvalue));
        Operand::Copy(temp.into())
    }

    /// Copies a place operand into a temporary, so that the guards and the
    /// operation read the same value.
    fn snapshot(&mut self, operand: &mut Operand, ty: &Type) {
        if let Operand::Copy(_) = operand {
            *operand = self.temp(ty.clone(), Rvalue::Use(operand.clone()));
        }
    }

    /// Ends the current block with an assertion that traps with `kind` when
    /// `condition` is true, and continues in a new block.
    fn trap_if(&mut self, condition: Operand, kind: AssertKind) {
        let continuation = self.body.basic_blocks_mut().push(BasicBlockData {
            statements: Vec::new(),
            terminator: Terminator {
                source_info: self.source_info,
                kind: TerminatorKind::Unreachable,
            },
        });
        self.finish(Terminator {
            source_info: self.source_info,
            kind: TerminatorKind::Assert {
                cond: condition,
                expected: false,
                kind,
                target: continuation,
            },
        });
        self.block = continuation;
    }

    /// Stores the statements collected so far into the current block, ended
    /// by `terminator`.
    fn finish(&mut self, terminator: Terminator) {
        self.body.basic_blocks_mut()[self.block] = BasicBlockData {
            statements: std::mem::take(&mut self.statements),
            terminator,
        };
    }
}
