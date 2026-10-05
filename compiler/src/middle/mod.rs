//! The middle end: semantic analysis and target-independent IRs.

pub mod constant_fold;
pub mod ids;
pub mod mir;
pub mod ops;
pub mod target;
pub mod thir;
pub mod typecheck;
pub mod validate;
