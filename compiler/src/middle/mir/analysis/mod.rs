//! Flow-sensitive analyses and required runtime normalization for MIR.
//!
//! The analysis boundary is deliberately independent of any backend:
//! [`analyze_program`] checks reachable paths and advances Built MIR to the
//! Checked phase; [`normalize_runtime_checks`] turns trapping arithmetic
//! contracts into explicit MIR assertions and advances Checked MIR to
//! Runtime. Optional optimizations run only after these correctness stages.
//!
//! Analyses classify local accesses through [`Visitor::visit_local`] and its
//! [`PlaceContext`](super::PlaceContext), so a new MIR construct only has to
//! be taught to the visitor to be understood by every analysis.
//!
//! # Module map
//!
//! - `flow`: reachability and definite initialization.
//! - `liveness`: backward local liveness.
//! - `runtime`: explicit arithmetic and shift checks.
//!
//! [`Visitor::visit_local`]: super::Visitor::visit_local

mod flow;
mod liveness;
mod runtime;

pub use flow::{
    AnalysisError, AnalysisErrorKind, AnalysisErrors, analyze_program, reachable_blocks,
};
pub use liveness::{Liveness, analyze_liveness};
pub use runtime::{OverflowMode, RuntimeCheckError, RuntimeCheckPolicy, normalize_runtime_checks};

use super::body::{BasicBlock, Local};
use super::syntax::TerminatorKind;

/// The local a terminator defines on its edge to `successor`.
///
/// A call writes its destination only when it returns normally, so the
/// definition belongs to the edge to its return target rather than to the
/// block. Writes through a pointer define no local.
fn edge_definition(terminator: &TerminatorKind, successor: BasicBlock) -> Option<Local> {
    match terminator {
        TerminatorKind::Call {
            destination,
            target: Some(target),
            ..
        } if *target == successor => destination.as_local(),
        _ => None,
    }
}
