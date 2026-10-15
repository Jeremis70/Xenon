//! Compile-time detection of operations that always trap.
//!
//! An integer operation whose operands are known at compile time, and which
//! would fail its runtime check, is reported as an error instead of being
//! left to trap when the program runs: division or remainder by zero,
//! `MIN / -1`, arithmetic overflow, and out-of-range shifts.
//!
//! The pass propagates constants through locals that are defined exactly
//! once and never have their address taken. Zero-sized integers are always
//! known to be `0`. Blocks that known branch conditions rule out are not
//! checked. Results come from [`interpret`](crate::middle::mir::interpret),
//! so they agree with what the program computes at runtime.
//!
//! Overflow is reported whatever the overflow mode, so that diagnostics do
//! not depend on the optimization level.

use std::fmt;

use thiserror::Error;

use crate::index::IndexVec;
use crate::middle::ids::DefId;
use crate::middle::mir::body::{BasicBlock, Body, Local, Location, MirPhase, START_BLOCK};
use crate::middle::mir::interpret::{EvalResult, eval_rvalue, known_operand};
use crate::middle::mir::program::MirProgram;
use crate::middle::mir::syntax::{AssertKind, Constant, StatementKind};
use crate::middle::mir::traversal::reverse_postorder;
use crate::middle::mir::typing::TypingError;
use crate::middle::mir::visit::{MutatingUseContext, PlaceContext, Visitor};
use crate::middle::target::TargetSpec;
use crate::source::Span;

use super::flow::executable_successors_with;

/// An operation that traps whenever it executes.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownPanic {
    /// The function containing the operation.
    pub function: String,
    /// The stable function identity.
    pub def_id: DefId,
    /// The statement computing the operation.
    pub location: Location,
    /// Original source location.
    pub span: Span,
    /// The runtime check the operation always fails.
    pub kind: AssertKind,
}

impl fmt::Display for KnownPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "this operation will panic at runtime: {}",
            self.kind.description()
        )
    }
}

/// An invalid input to [`check_known_panics`].
#[derive(Debug, Clone, PartialEq, Error)]
pub enum KnownPanicsError {
    /// The pass only accepts Checked MIR.
    #[error("expected Checked MIR, found phase `{phase}` in `{function}`")]
    InvalidPhase { function: String, phase: MirPhase },

    /// A type could not be derived from the MIR.
    #[error(transparent)]
    Typing(#[from] TypingError),
}

/// Returns every reachable operation of `program` that always traps, in
/// function then location order.
///
/// Reads Checked MIR, whose locals are definitely initialized before use,
/// and leaves it unchanged.
pub fn check_known_panics(
    program: &MirProgram,
    target: &TargetSpec,
) -> Result<Vec<KnownPanic>, KnownPanicsError> {
    let mut panics = Vec::new();
    for body in program.bodies() {
        if body.phase() != MirPhase::Checked {
            return Err(KnownPanicsError::InvalidPhase {
                function: program.fn_name(body.def_id()),
                phase: body.phase(),
            });
        }
        let mut found = check_body(body, target)?;
        found.sort_by_key(|(location, _, _)| *location);
        panics.extend(found.into_iter().map(|(location, span, kind)| KnownPanic {
            function: program.fn_name(body.def_id()),
            def_id: body.def_id(),
            location,
            span,
            kind,
        }));
    }
    Ok(panics)
}

fn check_body(
    body: &Body,
    target: &TargetSpec,
) -> Result<Vec<(Location, Span, AssertKind)>, TypingError> {
    let propagatable = propagatable_locals(body);
    let mut known: IndexVec<Local, Option<Constant>> =
        IndexVec::from_elem_n(None, body.local_decls().len());
    let mut live = IndexVec::from_elem_n(false, body.basic_blocks().len());
    live[START_BLOCK] = true;
    let mut found = Vec::new();

    // Reverse postorder visits the single definition of a propagatable
    // local before its uses: definite initialization makes it dominate them.
    for block in reverse_postorder(body) {
        if !live[block] {
            continue;
        }
        let data = &body.basic_blocks()[block];
        for (statement_index, statement) in data.statements.iter().enumerate() {
            let StatementKind::Assign(assign) = &statement.kind else {
                continue;
            };
            let (place, rvalue) = &**assign;
            let value_of = |operand: &_| known_operand(operand, body, |local| known[local].clone());
            match eval_rvalue(rvalue, body, target, value_of)? {
                EvalResult::Trap(kind) => {
                    let location = Location {
                        block,
                        statement_index,
                    };
                    found.push((location, statement.source_info.span, kind));
                }
                EvalResult::Value(constant) => {
                    if let Some(local) = place.as_local()
                        && propagatable[local]
                    {
                        known[local] = Some(constant);
                    }
                }
                EvalResult::Unknown => {}
            }
        }
        let value_of = |operand: &_| {
            known_operand(operand, body, |local| known[local].clone()).map(|c| c.value)
        };
        for successor in executable_successors_with(&data.terminator.kind, value_of) {
            mark_live(&mut live, successor);
        }
    }
    Ok(found)
}

fn mark_live(live: &mut IndexVec<BasicBlock, bool>, block: BasicBlock) {
    if let Some(slot) = live.get_mut(block) {
        *slot = true;
    }
}

/// The locals whose value is fixed by their only definition: defined once
/// and never modified through a pointer.
fn propagatable_locals(body: &Body) -> IndexVec<Local, bool> {
    let mut counter = DefCounter {
        defs: IndexVec::from_elem_n(0, body.local_decls().len()),
        address_taken: IndexVec::from_elem_n(false, body.local_decls().len()),
    };
    counter.visit_body(body);
    counter
        .defs
        .iter()
        .zip(counter.address_taken.iter())
        .map(|(&defs, &address_taken)| defs == 1 && !address_taken)
        .collect()
}

struct DefCounter {
    defs: IndexVec<Local, u32>,
    address_taken: IndexVec<Local, bool>,
}

impl Visitor for DefCounter {
    fn visit_local(&mut self, local: &Local, context: PlaceContext, _location: Location) {
        match context {
            PlaceContext::MutatingUse(MutatingUseContext::AddressOf) => {
                self.address_taken[*local] = true;
            }
            PlaceContext::MutatingUse(MutatingUseContext::Store | MutatingUseContext::Call) => {
                self.defs[*local] += 1;
            }
            PlaceContext::NonMutatingUse(_) | PlaceContext::NonUse(_) => {}
        }
    }
}
