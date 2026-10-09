//! Flow-sensitive analyses and required runtime normalization for MIR.
//!
//! The analysis boundary is deliberately independent of any backend:
//! [`analyze_program`] checks reachable paths and advances Built MIR to the
//! Checked phase; [`normalize_runtime_checks`] turns trapping arithmetic
//! contracts into explicit MIR assertions and advances Checked MIR to
//! Runtime. Optional optimizations run only after these correctness stages.
//!
//! # Module map
//!
//! - `flow`: reachability and definite initialization.
//! - `liveness`: backward local liveness.
//! - `runtime`: explicit arithmetic and shift checks.

mod flow;
mod liveness;
mod runtime;

pub use flow::{
    AnalysisError, AnalysisErrorKind, AnalysisErrors, analyze_program, reachable_blocks,
};
pub use liveness::{Liveness, analyze_liveness};
pub use runtime::{OverflowMode, RuntimeCheckError, RuntimeCheckPolicy, normalize_runtime_checks};
