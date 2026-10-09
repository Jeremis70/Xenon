//! CFG reachability and forward definite-initialization analysis.
//!
//! Definite initialization is a forward must-analysis. At a join, a local is
//! initialized only when it is initialized on every reachable incoming edge.
//! A block's entry state starts at the lattice top (unvisited) and only
//! shrinks, so loops converge to the greatest fixed point.

use std::collections::BTreeSet;
use std::fmt;

use num_bigint::BigInt;
use thiserror::Error;

use crate::index::IndexVec;
use crate::middle::ids::DefId;
use crate::middle::mir::body::{
    BasicBlock, Body, Local, Location, MirPhase, PhaseError, RETURN_PLACE, START_BLOCK,
};
use crate::middle::mir::program::MirProgram;
use crate::middle::mir::syntax::{ConstValue, Operand, TerminatorKind};
use crate::middle::mir::verify::{ErrorSite, VerifyErrors, verify_program};
use crate::middle::mir::visit::{MutatingUseContext, PlaceContext, Visitor};
use crate::middle::target::TargetSpec;
use crate::source::Span;

use super::edge_definition;

/// A source-level flow error or an invalid-MIR precondition.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("flow analysis of `{function}` at {site}: {kind}")]
pub struct AnalysisError {
    /// The function containing the error.
    pub function: String,
    /// The stable function identity.
    pub def_id: DefId,
    /// Where in the body the error is.
    pub site: ErrorSite,
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

impl AnalysisError {
    fn new(program: &MirProgram, def_id: DefId, site: ErrorSite, kind: AnalysisErrorKind) -> Self {
        let span = program
            .body(def_id)
            .map_or(Span::ZERO, |body| site_span(body, site));
        Self {
            function: program.fn_name(def_id),
            def_id,
            site,
            span,
            kind,
        }
    }
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
    verify_program(program, target).map_err(|errors| invalid_mir(program, errors))?;

    let mut errors = Vec::new();
    for body in program.bodies() {
        if body.phase() == MirPhase::Built {
            errors.extend(check_body(program, body));
        } else {
            let kind = AnalysisErrorKind::InvalidPhase(body.phase());
            errors.push(AnalysisError::new(
                program,
                body.def_id(),
                ErrorSite::Body,
                kind,
            ));
        }
    }
    if !errors.is_empty() {
        return Err(AnalysisErrors(errors));
    }

    for body in program.bodies_mut() {
        replace_unreachable_fallthrough(body);
    }
    program
        .advance_phase(MirPhase::Checked)
        .map_err(|error: PhaseError| {
            let kind = AnalysisErrorKind::InvalidPhase(error.from);
            AnalysisErrors(vec![AnalysisError::new(
                program,
                error.def_id,
                ErrorSite::Body,
                kind,
            )])
        })?;
    verify_program(program, target).map_err(|errors| invalid_mir(program, errors))
}

/// Returns the blocks reachable from `bb0` along executable edges: a switch
/// or assert on a constant only follows the edge that constant selects.
///
/// Unlike [`traversal::reachable_set`](crate::middle::mir::traversal::reachable_set),
/// which follows every CFG edge, this is the reachability source-level
/// diagnostics are based on.
pub fn reachable_blocks(body: &Body) -> BTreeSet<BasicBlock> {
    let blocks = body.basic_blocks();
    let mut reachable = BTreeSet::new();
    let mut pending = vec![START_BLOCK];
    while let Some(block) = pending.pop() {
        if let Some(data) = blocks.get(block)
            && reachable.insert(block)
        {
            pending.extend(executable_successors(&data.terminator.kind));
        }
    }
    reachable
}

/// The successors control can actually reach from a terminator.
fn executable_successors(terminator: &TerminatorKind) -> Vec<BasicBlock> {
    match terminator {
        TerminatorKind::SwitchInt {
            discr: Operand::Constant(constant),
            targets,
        } => {
            let value = match &constant.value {
                ConstValue::Bool(value) => Some(BigInt::from(u8::from(*value))),
                ConstValue::Int(value) => Some(value.clone()),
                ConstValue::Float(_) | ConstValue::Address(_) => None,
            };
            match value {
                Some(value) => vec![targets.target_for_value(&value)],
                None => terminator.successors().collect(),
            }
        }
        TerminatorKind::Assert {
            cond: Operand::Constant(constant),
            expected,
            ..
        } if constant.value == ConstValue::Bool(!*expected) => Vec::new(),
        _ => terminator.successors().collect(),
    }
}

/// Which locals hold a value, indexed by local.
type InitState = IndexVec<Local, bool>;

/// Reports every read of a possibly uninitialized local and every
/// reachable fallthrough in `body`.
fn check_body(program: &MirProgram, body: &Body) -> Vec<AnalysisError> {
    let mut findings = BTreeSet::new();
    for (block, state) in definite_initialization(body).into_iter_enumerated() {
        // Blocks without an entry state are unreachable.
        let Some(state) = state else {
            continue;
        };
        let data = &body.basic_blocks()[block];
        let mut walker = InitWalker {
            state,
            uninitialized_reads: Some(&mut findings),
        };
        walker.visit_basic_block_data(block, data);
        if matches!(data.terminator.kind, TerminatorKind::EndOfBody) {
            findings.insert((data.terminator_location(block), None));
        }
    }

    findings
        .into_iter()
        .map(|(location, local)| {
            let kind = match local {
                None => AnalysisErrorKind::MissingReturn,
                Some(RETURN_PLACE) => AnalysisErrorKind::UninitializedReturn,
                Some(local) => AnalysisErrorKind::UninitializedLocal {
                    name: body.local_decls()[local]
                        .debug_name
                        .clone()
                        .unwrap_or_else(|| local.to_string()),
                    local,
                },
            };
            AnalysisError::new(program, body.def_id(), ErrorSite::Location(location), kind)
        })
        .collect()
}

/// The initialized locals on entry to each block, `None` for blocks that no
/// executable path reaches.
fn definite_initialization(body: &Body) -> IndexVec<BasicBlock, Option<InitState>> {
    let blocks = body.basic_blocks();
    let mut entry_states = IndexVec::from_elem_n(None, blocks.len());
    let mut arguments = IndexVec::from_elem_n(false, body.local_decls().len());
    for arg in body.args_iter() {
        arguments[arg] = true;
    }
    entry_states[START_BLOCK] = Some(arguments);

    let mut pending = vec![START_BLOCK];
    while let Some(block) = pending.pop() {
        let Some(state) = entry_states[block].clone() else {
            continue;
        };
        let data = &blocks[block];
        let mut walker = InitWalker {
            state,
            uninitialized_reads: None,
        };
        walker.visit_basic_block_data(block, data);

        let terminator = &data.terminator.kind;
        for successor in executable_successors(terminator) {
            let mut state = walker.state.clone();
            if let Some(local) = edge_definition(terminator, successor) {
                state[local] = true;
            }
            let changed = match &mut entry_states[successor] {
                Some(current) => meet(current, &state),
                slot @ None => {
                    *slot = Some(state);
                    true
                }
            };
            if changed {
                pending.push(successor);
            }
        }
    }
    entry_states
}

/// Intersects `state` into `current`; returns whether `current` changed.
fn meet(current: &mut InitState, state: &InitState) -> bool {
    let mut changed = false;
    for (slot, &initialized) in current.iter_mut().zip(state) {
        changed |= *slot && !initialized;
        *slot &= initialized;
    }
    changed
}

/// Applies a block's statements to an initialization state, optionally
/// recording the reads of locals that are not initialized.
struct InitWalker<'a> {
    state: InitState,
    /// Where `(location, Some(local))` reads of uninitialized locals are
    /// recorded; `None` while the fixed point is being computed.
    uninitialized_reads: Option<&'a mut BTreeSet<(Location, Option<Local>)>>,
}

impl Visitor for InitWalker<'_> {
    fn visit_local(&mut self, local: &Local, context: PlaceContext, location: Location) {
        match context {
            PlaceContext::NonMutatingUse(_) => {
                if !self.state[*local]
                    && let Some(reads) = &mut self.uninitialized_reads
                {
                    reads.insert((location, Some(*local)));
                }
            }
            PlaceContext::MutatingUse(MutatingUseContext::Store) => self.state[*local] = true,
            // A fresh or dead storage slot holds no value.
            PlaceContext::NonUse(_) => self.state[*local] = false,
            // Call destinations are initialized on the return edge, see
            // `edge_definition`; taking an address neither reads nor writes.
            PlaceContext::MutatingUse(MutatingUseContext::Call | MutatingUseContext::AddressOf) => {
            }
        }
    }
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

/// The source span of `site` in `body`, or the body's span if the site
/// does not exist.
fn site_span(body: &Body, site: ErrorSite) -> Span {
    let span = match site {
        ErrorSite::Body => None,
        ErrorSite::LocalDecl(local) => body
            .local_decls()
            .get(local)
            .map(|decl| decl.source_info.span),
        ErrorSite::Location(location) => body.source_info(location).map(|info| info.span),
    };
    span.unwrap_or(body.span())
}

fn invalid_mir(program: &MirProgram, errors: VerifyErrors) -> AnalysisErrors {
    AnalysisErrors(
        errors
            .0
            .into_iter()
            .map(|error| {
                let kind = AnalysisErrorKind::InvalidMir(error.kind.to_string());
                AnalysisError::new(program, error.def_id, error.site, kind)
            })
            .collect(),
    )
}
