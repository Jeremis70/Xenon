//! Required runtime-check normalization.
//!
//! Potentially trapping integer operations must have explicit guards before
//! the backend sees them. Operands are snapshotted before guards so neither
//! the checks nor the operation can observe a different value. The overflow
//! mode is supplied explicitly and is independent of optimization level.

use thiserror::Error;

use num_bigint::BigInt;

use crate::index::Idx;
use crate::middle::mir::body::{BasicBlock, BasicBlockData, Body, MirPhase, SourceInfo};
use crate::middle::mir::program::MirProgram;
use crate::middle::mir::syntax::{
    AssertKind, BinOp, CastKind, Constant, Operand, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
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
    verify_program(program, target).map_err(verify_error)?;
    let mut normalized = program.clone();
    for body in normalized.bodies() {
        if body.phase() != MirPhase::Checked {
            let function = normalized
                .decl(body.def_id())
                .map_or_else(|| body.def_id().to_string(), |decl| decl.name.clone());
            return Err(RuntimeCheckError::InvalidPhase {
                function,
                phase: body.phase(),
            });
        }
    }

    for body in normalized.bodies_mut() {
        normalize_body(body, target, policy)?;
        body.advance_phase(MirPhase::Runtime)
            .map_err(|error| RuntimeCheckError::InvalidPhase {
                function: body.def_id().to_string(),
                phase: error.from,
            })?;
    }
    verify_program(&normalized, target).map_err(verify_error)?;
    *program = normalized;
    Ok(())
}

fn normalize_body(
    body: &mut Body,
    target: &TargetSpec,
    policy: RuntimeCheckPolicy,
) -> Result<(), RuntimeCheckError> {
    let original_block_count = body.basic_blocks().len();
    for block_index in 0..original_block_count {
        let block = BasicBlock::new(block_index);
        let original = body.basic_blocks()[block].clone();
        let mut current = block;
        let mut statements = Vec::new();
        let mut segments = Vec::new();

        for mut statement in original.statements {
            let checks = match &mut statement.kind {
                StatementKind::Assign(assign) => {
                    operation_checks(body, target, policy, statement.source_info, &mut assign.1)?
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => Vec::new(),
            };
            if checks.is_empty() {
                statements.push(statement);
                continue;
            }

            for check in checks {
                statements.extend(check.statements);
                let continuation = append_placeholder_block(body, statement.source_info);
                segments.push((
                    current,
                    BasicBlockData {
                        statements: std::mem::take(&mut statements),
                        terminator: Terminator {
                            source_info: statement.source_info,
                            kind: TerminatorKind::Assert {
                                cond: check.condition,
                                expected: false,
                                kind: check.kind,
                                target: continuation,
                            },
                        },
                    },
                ));
                current = continuation;
            }
            statements.push(statement);
        }

        segments.push((
            current,
            BasicBlockData {
                statements,
                terminator: original.terminator,
            },
        ));
        for (segment_block, data) in segments {
            body.basic_blocks_mut()[segment_block] = data;
        }
    }
    Ok(())
}

struct Guard {
    statements: Vec<Statement>,
    condition: Operand,
    kind: AssertKind,
}

fn operation_checks(
    body: &mut Body,
    target: &TargetSpec,
    policy: RuntimeCheckPolicy,
    source_info: SourceInfo,
    rvalue: &mut Rvalue,
) -> Result<Vec<Guard>, RuntimeCheckError> {
    match rvalue {
        Rvalue::BinaryOp(op, operands) => {
            binary_operation_checks(body, target, policy, source_info, *op, operands)
        }
        Rvalue::UnaryOp(crate::middle::ops::UnOp::Neg, operand)
            if policy.overflow == OverflowMode::Checked =>
        {
            let ty = typing::operand_ty(operand, body)?.clone();
            if !ty.is_integer() {
                return Ok(Vec::new());
            }
            let mut setup = Vec::new();
            let stable = snapshot(body, operand.clone(), &ty, source_info, &mut setup);
            *operand = stable.clone();
            let flag = body.new_temp(Type::Bool, source_info.span);
            setup.push(assign(
                source_info,
                flag.into(),
                Rvalue::Overflows(
                    BinOp::Sub,
                    Box::new((Operand::constant(Constant::int(0, ty)), stable)),
                ),
            ));
            Ok(vec![Guard {
                statements: setup,
                condition: Operand::Copy(flag.into()),
                kind: AssertKind::Overflow(BinOp::Sub),
            }])
        }
        _ => Ok(Vec::new()),
    }
}

fn binary_operation_checks(
    body: &mut Body,
    target: &TargetSpec,
    policy: RuntimeCheckPolicy,
    source_info: SourceInfo,
    op: BinOp,
    operands: &mut Box<(Operand, Operand)>,
) -> Result<Vec<Guard>, RuntimeCheckError> {
    let (lhs, rhs) = &mut **operands;
    let lhs_ty = typing::operand_ty(lhs, body)?.clone();
    let rhs_ty = typing::operand_ty(rhs, body)?.clone();
    let is_integer = lhs_ty.is_integer();
    let needs_overflow =
        policy.overflow == OverflowMode::Checked && is_integer && op.is_overflow_checkable();
    let needs_zero_check = is_integer && matches!(op, BinOp::Div | BinOp::Rem);
    let needs_shift_check = is_integer && op.is_shift();
    if !(needs_overflow || needs_zero_check || needs_shift_check) {
        return Ok(Vec::new());
    }

    let mut setup = Vec::new();
    *lhs = snapshot(body, lhs.clone(), &lhs_ty, source_info, &mut setup);
    *rhs = snapshot(body, rhs.clone(), &rhs_ty, source_info, &mut setup);
    let mut guards = Vec::new();

    if needs_overflow {
        let flag = body.new_temp(Type::Bool, source_info.span);
        setup.push(assign(
            source_info,
            flag.into(),
            Rvalue::Overflows(op, Box::new((lhs.clone(), rhs.clone()))),
        ));
        guards.push(Guard {
            statements: std::mem::take(&mut setup),
            condition: Operand::Copy(flag.into()),
            kind: AssertKind::Overflow(op),
        });
    }

    if needs_zero_check {
        let zero = Operand::constant(Constant::int(0, rhs_ty.clone()));
        let is_zero = body.new_temp(Type::Bool, source_info.span);
        let mut statements = std::mem::take(&mut setup);
        statements.push(assign(
            source_info,
            is_zero.into(),
            Rvalue::BinaryOp(BinOp::Eq, Box::new((rhs.clone(), zero))),
        ));
        guards.push(Guard {
            statements,
            condition: Operand::Copy(is_zero.into()),
            kind: if op == BinOp::Rem {
                AssertKind::RemainderByZero
            } else {
                AssertKind::DivisionByZero
            },
        });
    }

    if needs_zero_check && lhs_ty.is_signed_integer() {
        let (min, _) = target
            .int_bounds(&lhs_ty)
            .ok_or_else(|| RuntimeCheckError::MissingIntegerBounds(lhs_ty.clone()))?;
        let min_operand = Operand::constant(Constant::int(min, lhs_ty.clone()));
        let minus_one = Operand::constant(Constant::int(-1, rhs_ty.clone()));
        let is_min = body.new_temp(Type::Bool, source_info.span);
        let is_minus_one = body.new_temp(Type::Bool, source_info.span);
        let is_overflow = body.new_temp(Type::Bool, source_info.span);
        let statements = vec![
            assign(
                source_info,
                is_min.into(),
                Rvalue::BinaryOp(BinOp::Eq, Box::new((lhs.clone(), min_operand))),
            ),
            assign(
                source_info,
                is_minus_one.into(),
                Rvalue::BinaryOp(BinOp::Eq, Box::new((rhs.clone(), minus_one))),
            ),
            assign(
                source_info,
                is_overflow.into(),
                Rvalue::BinaryOp(
                    BinOp::BitAnd,
                    Box::new((
                        Operand::Copy(is_min.into()),
                        Operand::Copy(is_minus_one.into()),
                    )),
                ),
            ),
        ];
        guards.push(Guard {
            statements,
            condition: Operand::Copy(is_overflow.into()),
            kind: if op == BinOp::Rem {
                AssertKind::SignedRemainderOverflow
            } else {
                AssertKind::SignedDivisionOverflow
            },
        });
    }

    if needs_shift_check {
        let lhs_width = target
            .int_width(&lhs_ty)
            .ok_or_else(|| RuntimeCheckError::MissingIntegerWidth(lhs_ty.clone()))?;
        let rhs_width = target
            .int_width(&rhs_ty)
            .ok_or_else(|| RuntimeCheckError::MissingIntegerWidth(rhs_ty.clone()))?;
        let limit_width = u32::BITS - lhs_width.leading_zeros();
        let comparison_ty = Type::UInt(rhs_width.max(limit_width));
        let shifted = if rhs_ty == comparison_ty {
            rhs.clone()
        } else {
            let widened = body.new_temp(comparison_ty.clone(), source_info.span);
            setup.push(assign(
                source_info,
                widened.into(),
                Rvalue::Cast(CastKind::IntToInt, rhs.clone(), comparison_ty.clone()),
            ));
            Operand::Copy(widened.into())
        };
        let too_large = body.new_temp(Type::Bool, source_info.span);
        setup.push(assign(
            source_info,
            too_large.into(),
            Rvalue::BinaryOp(
                BinOp::Ge,
                Box::new((
                    shifted,
                    Operand::constant(Constant::int(BigInt::from(lhs_width), comparison_ty)),
                )),
            ),
        ));
        guards.push(Guard {
            statements: std::mem::take(&mut setup),
            condition: Operand::Copy(too_large.into()),
            kind: AssertKind::ShiftOutOfRange,
        });
    }

    Ok(guards)
}

fn snapshot(
    body: &mut Body,
    operand: Operand,
    ty: &Type,
    source_info: SourceInfo,
    statements: &mut Vec<Statement>,
) -> Operand {
    if matches!(operand, Operand::Constant(_)) {
        return operand;
    }
    let temp = body.new_temp(ty.clone(), source_info.span);
    statements.push(assign(source_info, temp.into(), Rvalue::Use(operand)));
    Operand::Copy(temp.into())
}

fn assign(source_info: SourceInfo, place: crate::middle::mir::Place, rvalue: Rvalue) -> Statement {
    Statement {
        source_info,
        kind: StatementKind::Assign(Box::new((place, rvalue))),
    }
}

fn append_placeholder_block(body: &mut Body, source_info: SourceInfo) -> BasicBlock {
    body.basic_blocks_mut().push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            source_info,
            kind: TerminatorKind::Unreachable,
        },
    })
}

fn verify_error(errors: VerifyErrors) -> RuntimeCheckError {
    RuntimeCheckError::InvalidMir(errors.to_string())
}
