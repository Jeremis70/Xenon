//! CFG reachability and forward definite-initialization analysis.
//!
//! Definite initialization is a forward must-analysis. At a join, a local is
//! initialized only when it is initialized on every reachable incoming edge.
//! The fixed point starts at the lattice top so loops converge correctly.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use num_bigint::BigInt;
use thiserror::Error;

use crate::index::{Idx, IndexVec};
use crate::middle::ids::DefId;
use crate::middle::mir::body::{
    BasicBlock, Body, Local, Location, MirPhase, RETURN_PLACE, START_BLOCK,
};
use crate::middle::mir::program::MirProgram;
use crate::middle::mir::syntax::{
    ConstValue, Operand, Place, Rvalue, StatementKind, TerminatorKind,
};
use crate::middle::mir::verify::{ErrorSite, VerifyError, verify_program};
use crate::middle::target::TargetSpec;
use crate::source::Span;

/// A source-level flow error or an invalid-MIR precondition.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("flow analysis of `{function}` at {location}: {kind}")]
pub struct AnalysisError {
    /// The function containing the error.
    pub function: String,
    /// The stable function identity.
    pub def_id: DefId,
    /// The MIR program point, when the error is tied to one.
    pub location: String,
    /// Original source location.
    pub span: Span,
    /// The diagnosed condition.
    pub kind: AnalysisErrorKind,
}

/// The kinds of [`AnalysisError`].
#[derive(Debug, Clone, PartialEq, Error)]
pub enum AnalysisErrorKind {
    /// A reachable path leaves a value-returning function without returning.
    #[error("reachable path falls through without returning a value")]
    MissingReturn,

    /// A local is read before it has a value on every incoming path.
    #[error("local `{name}` may be used before it is initialized")]
    UninitializedLocal {
        /// The source name, or the MIR local name for generated locals.
        name: String,
        /// The local being read.
        local: Local,
    },

    /// A return reads `_0` before assigning the return value.
    #[error("return value may be uninitialized")]
    UninitializedReturn,

    /// The analysis was invoked on a phase it does not accept.
    #[error("expected Built MIR, found phase `{0}`")]
    InvalidPhase(MirPhase),

    /// The MIR verifier rejected the input before analysis.
    #[error("MIR verification failed: {0}")]
    InvalidMir(String),
}

/// A deterministic collection of flow-analysis errors.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisErrors(pub Vec<AnalysisError>);

impl fmt::Display for AnalysisErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MIR flow analysis found {} error(s)", self.0.len())?;
        for error in &self.0 {
            writeln!(f, "  {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AnalysisErrors {}

/// Checks reachable fallthrough and definite initialization, then advances
/// every body from Built to Checked if the whole program is valid.
///
/// The phase transition is atomic: if any body has a source error, no body
/// advances. Unreachable `EndOfBody` markers are replaced with
/// `Unreachable`, since the Checked phase cannot contain fallthrough markers.
pub fn analyze_program(
    program: &mut MirProgram,
    target: &TargetSpec,
) -> Result<(), AnalysisErrors> {
    if let Err(errors) = verify_program(program, target) {
        return Err(AnalysisErrors(
            errors
                .0
                .into_iter()
                .map(|error| verification_error(program, error))
                .collect(),
        ));
    }

    let mut errors = Vec::new();
    for body in program.bodies() {
        if body.phase() != MirPhase::Built {
            errors.push(body_error(
                program,
                body,
                body.span(),
                AnalysisErrorKind::InvalidPhase(body.phase()),
            ));
            continue;
        }
        errors.extend(analyze_body(program, body));
    }
    if !errors.is_empty() {
        return Err(AnalysisErrors(errors));
    }

    let mut checked = program.clone();
    for body in checked.bodies_mut() {
        replace_unreachable_fallthrough(body);
        if let Err(error) = body.advance_phase(MirPhase::Checked) {
            return Err(AnalysisErrors(vec![AnalysisError {
                function: body.def_id().to_string(),
                def_id: body.def_id(),
                location: "body".to_owned(),
                span: body.span(),
                kind: AnalysisErrorKind::InvalidPhase(error.from),
            }]));
        }
    }
    if let Err(errors) = verify_program(&checked, target) {
        return Err(AnalysisErrors(
            errors
                .0
                .into_iter()
                .map(|error| verification_error(&checked, error))
                .collect(),
        ));
    }
    *program = checked;
    Ok(())
}

/// Returns the set of blocks reachable from `bb0`.
pub fn reachable_blocks(body: &Body) -> BTreeSet<BasicBlock> {
    let mut reachable = BTreeSet::new();
    let mut pending = VecDeque::from([START_BLOCK]);
    while let Some(block) = pending.pop_front() {
        if !reachable.insert(block) {
            continue;
        }
        pending.extend(executable_successors(body, block));
    }
    reachable
}

fn analyze_body(program: &MirProgram, body: &Body) -> Vec<AnalysisError> {
    let reachable = reachable_blocks(body);
    let states = definite_initialization(body, &reachable);
    let mut checker = InitChecker {
        program,
        body,
        reported: BTreeSet::new(),
        errors: Vec::new(),
    };

    for block in &reachable {
        let Some(data) = body.basic_blocks().get(*block) else {
            continue;
        };
        let Some(mut initialized) = states.get(*block).cloned().flatten() else {
            continue;
        };
        for (statement_index, statement) in data.statements.iter().enumerate() {
            let location = Location {
                block: *block,
                statement_index,
            };
            match &statement.kind {
                StatementKind::Assign(assign) => {
                    let (place, rvalue) = &**assign;
                    checker.check_rvalue_reads(
                        location,
                        statement.source_info.span,
                        rvalue,
                        &initialized,
                    );
                    if !place.projection.is_empty() {
                        checker.check_place_base(
                            location,
                            statement.source_info.span,
                            place,
                            &initialized,
                        );
                    }
                    if place.projection.is_empty() {
                        initialized[place.local.index()] = true;
                    }
                }
                StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                    initialized[local.index()] = false;
                }
                StatementKind::Nop => {}
            }
        }

        let location = data.terminator_location(*block);
        let terminator = &data.terminator;
        let span = terminator.source_info.span;
        match &terminator.kind {
            TerminatorKind::SwitchInt { discr, .. }
            | TerminatorKind::Assert { cond: discr, .. } => {
                checker.check_operand_reads(location, span, discr, &initialized);
            }
            TerminatorKind::Call {
                args, destination, ..
            } => {
                for arg in args {
                    checker.check_operand_reads(location, span, arg, &initialized);
                }
                if !destination.projection.is_empty() {
                    checker.check_place_base(location, span, destination, &initialized);
                }
            }
            TerminatorKind::Return => {
                if !initialized[RETURN_PLACE.index()] {
                    checker.push_error(location, span, AnalysisErrorKind::UninitializedReturn);
                }
            }
            TerminatorKind::EndOfBody => {
                checker.push_error(location, span, AnalysisErrorKind::MissingReturn)
            }
            TerminatorKind::Goto { .. } | TerminatorKind::Unreachable => {}
        }
    }
    checker.errors
}

fn definite_initialization(
    body: &Body,
    reachable: &BTreeSet<BasicBlock>,
) -> IndexVec<BasicBlock, Option<Vec<bool>>> {
    let block_count = body.basic_blocks().len();
    let local_count = body.local_decls().len();
    let mut in_states = IndexVec::from_elem_n(None, block_count);
    let mut out_states = IndexVec::from_elem_n(None, block_count);
    let top = vec![true; local_count];
    for block in reachable {
        in_states[*block] = Some(top.clone());
        out_states[*block] = Some(top.clone());
    }

    let entry = START_BLOCK;
    let predecessors = body.basic_blocks().predecessors();
    loop {
        let mut changed = false;
        for block in reachable {
            let mut incoming: Option<Vec<bool>> = if *block == entry {
                let mut boundary = vec![false; local_count];
                for arg in body.args_iter() {
                    boundary[arg.index()] = true;
                }
                Some(boundary)
            } else {
                None
            };

            if let Some(preds) = predecessors.get(*block) {
                for pred in preds.iter().filter(|pred| {
                    reachable.contains(pred) && executable_successors(body, **pred).contains(block)
                }) {
                    let Some(state) = out_states.get(*pred).cloned().flatten() else {
                        continue;
                    };
                    let state = edge_state(body, *pred, *block, state);
                    incoming = Some(match incoming {
                        Some(mut current) => {
                            for (slot, value) in current.iter_mut().zip(state) {
                                *slot &= value;
                            }
                            current
                        }
                        None => state,
                    });
                }
            }

            let Some(new_in) = incoming else {
                continue;
            };
            let new_out = transfer_block(body, *block, new_in.clone());
            if in_states[*block].as_ref() != Some(&new_in) {
                in_states[*block] = Some(new_in);
                changed = true;
            }
            if out_states[*block].as_ref() != Some(&new_out) {
                out_states[*block] = Some(new_out);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    in_states
}

fn executable_successors(body: &Body, block: BasicBlock) -> Vec<BasicBlock> {
    let Some(data) = body.basic_blocks().get(block) else {
        return Vec::new();
    };
    match &data.terminator.kind {
        TerminatorKind::SwitchInt { discr, targets } => {
            let value = match discr {
                Operand::Constant(constant) => match &constant.value {
                    ConstValue::Bool(value) => Some(BigInt::from(u8::from(*value))),
                    ConstValue::Int(value) => Some(value.clone()),
                    ConstValue::Float(_) | ConstValue::Address(_) => None,
                },
                Operand::Copy(_) => None,
            };
            value.map_or_else(
                || data.terminator.kind.successors().collect(),
                |value| vec![targets.target_for_value(&value)],
            )
        }
        TerminatorKind::Assert {
            cond: Operand::Constant(constant),
            expected,
            target,
            ..
        } => match &constant.value {
            ConstValue::Bool(value) if *value == *expected => vec![*target],
            ConstValue::Bool(_) => Vec::new(),
            ConstValue::Int(_) | ConstValue::Float(_) | ConstValue::Address(_) => vec![*target],
        },
        _ => data.terminator.kind.successors().collect(),
    }
}

fn transfer_block(body: &Body, block: BasicBlock, mut state: Vec<bool>) -> Vec<bool> {
    let Some(data) = body.basic_blocks().get(block) else {
        return state;
    };
    for statement in &data.statements {
        match &statement.kind {
            StatementKind::Assign(assign) => {
                let (place, _) = &**assign;
                if place.projection.is_empty() {
                    state[place.local.index()] = true;
                }
            }
            StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                state[local.index()] = false;
            }
            StatementKind::Nop => {}
        }
    }
    state
}

fn edge_state(body: &Body, from: BasicBlock, to: BasicBlock, mut state: Vec<bool>) -> Vec<bool> {
    let Some(data) = body.basic_blocks().get(from) else {
        return state;
    };
    if let TerminatorKind::Call {
        destination,
        target: Some(target),
        ..
    } = &data.terminator.kind
        && *target == to
        && destination.projection.is_empty()
    {
        state[destination.local.index()] = true;
    }
    state
}

fn replace_unreachable_fallthrough(body: &mut Body) {
    let reachable = reachable_blocks(body);
    for (block, data) in body.basic_blocks_mut().iter_enumerated_mut() {
        if !reachable.contains(&block) && matches!(data.terminator.kind, TerminatorKind::EndOfBody)
        {
            data.terminator.kind = TerminatorKind::Unreachable;
        }
    }
}

struct InitChecker<'a> {
    program: &'a MirProgram,
    body: &'a Body,
    reported: BTreeSet<(Location, Local)>,
    errors: Vec<AnalysisError>,
}

impl InitChecker<'_> {
    fn check_rvalue_reads(
        &mut self,
        location: Location,
        span: Span,
        rvalue: &Rvalue,
        initialized: &[bool],
    ) {
        match rvalue {
            Rvalue::Use(operand) | Rvalue::UnaryOp(_, operand) | Rvalue::Cast(_, operand, _) => {
                self.check_operand_reads(location, span, operand, initialized);
            }
            Rvalue::BinaryOp(_, operands) | Rvalue::Overflows(_, operands) => {
                self.check_operand_reads(location, span, &operands.0, initialized);
                self.check_operand_reads(location, span, &operands.1, initialized);
            }
            Rvalue::AddressOf(_, place) if !place.projection.is_empty() => {
                self.check_local_read(location, span, place.local, initialized);
            }
            Rvalue::AddressOf(_, _) => {}
        }
    }

    fn check_operand_reads(
        &mut self,
        location: Location,
        span: Span,
        operand: &Operand,
        initialized: &[bool],
    ) {
        if let Operand::Copy(place) = operand {
            self.check_place_base(location, span, place, initialized);
        }
    }

    fn check_place_base(
        &mut self,
        location: Location,
        span: Span,
        place: &Place,
        initialized: &[bool],
    ) {
        self.check_local_read(location, span, place.local, initialized);
    }

    fn check_local_read(
        &mut self,
        location: Location,
        span: Span,
        local: Local,
        initialized: &[bool],
    ) {
        if initialized.get(local.index()).copied().unwrap_or(false)
            || !self.reported.insert((location, local))
        {
            return;
        }
        let kind = if local == RETURN_PLACE {
            AnalysisErrorKind::UninitializedReturn
        } else {
            let name = self
                .body
                .local_decls()
                .get(local)
                .and_then(|decl| decl.debug_name.clone())
                .unwrap_or_else(|| local.to_string());
            AnalysisErrorKind::UninitializedLocal { name, local }
        };
        self.push_error(location, span, kind);
    }

    fn push_error(&mut self, location: Location, span: Span, kind: AnalysisErrorKind) {
        let function = self
            .program
            .decl(self.body.def_id())
            .map_or_else(|| self.body.def_id().to_string(), |decl| decl.name.clone());
        self.errors.push(AnalysisError {
            function,
            def_id: self.body.def_id(),
            location: location.to_string(),
            span,
            kind,
        });
    }
}

fn body_error(
    program: &MirProgram,
    body: &Body,
    span: Span,
    kind: AnalysisErrorKind,
) -> AnalysisError {
    AnalysisError {
        function: program
            .decl(body.def_id())
            .map_or_else(|| body.def_id().to_string(), |decl| decl.name.clone()),
        def_id: body.def_id(),
        location: "body".to_owned(),
        span,
        kind,
    }
}

fn verification_error(program: &MirProgram, error: VerifyError) -> AnalysisError {
    let span = program
        .body(error.def_id)
        .map_or(Span::ZERO, |body| match error.site {
            ErrorSite::Body => body.span(),
            ErrorSite::LocalDecl(local) => body
                .local_decls()
                .get(local)
                .map_or(body.span(), |decl| decl.source_info.span),
            ErrorSite::Location(location) => {
                body.basic_blocks()
                    .get(location.block)
                    .map_or(body.span(), |block| {
                        if location.statement_index < block.statements.len() {
                            block.statements[location.statement_index].source_info.span
                        } else {
                            block.terminator.source_info.span
                        }
                    })
            }
        });
    AnalysisError {
        function: error.function,
        def_id: error.def_id,
        location: error.site.to_string(),
        span,
        kind: AnalysisErrorKind::InvalidMir(error.kind.to_string()),
    }
}
