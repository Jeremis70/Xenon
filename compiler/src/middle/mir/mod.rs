//! MIR: the mid-level intermediate representation.
//!
//! MIR is a control-flow graph of basic blocks per function, operating on
//! explicitly typed locals. It sits between the type-checked tree IR and the
//! code generators, and is where flow-sensitive checks, runtime-check
//! insertion, and optimizations happen. Backends consume MIR only, so a new
//! backend (Cranelift, a custom one, ...) needs no knowledge of the AST.
//!
//! Module map:
//! - [`body`]: [`Body`], locals, blocks, scopes, and phases.
//! - [`analysis`]: reachability, dataflow, and runtime-check normalization.
//! - [`build`]: construction of built MIR from THIR ([`build_mir`]).
//! - [`syntax`]: statements, terminators, places, operands, and rvalues.
//! - [`program`]: [`MirProgram`], function declarations and bodies.
//! - [`typing`]: the single source of truth for MIR typing rules.
//! - [`builder`]: [`BodyBuilder`], the only way to create a body.
//! - [`visit`]: [`Visitor`] and [`MutVisitor`] traversals.
//! - [`traversal`]: CFG orders (preorder, postorder, reverse postorder).
//! - [`verify`]: the invariant checker run after construction and passes.
//! - [`pretty`]: deterministic textual dumps.
//!
//! See `docs/internals/mir.md` for the design and its invariants.

pub mod analysis;
pub mod body;
pub mod build;
pub mod builder;
pub mod pretty;
pub mod program;
pub mod syntax;
pub mod traversal;
pub mod typing;
pub mod verify;
pub mod visit;

pub use body::{
    BasicBlock, BasicBlockData, BasicBlocks, Body, Local, LocalDecl, LocalKind, Location, MirPhase,
    OUTERMOST_SOURCE_SCOPE, PhaseError, RETURN_PLACE, START_BLOCK, SourceInfo, SourceScope,
    SourceScopeData,
};
pub use build::{LowerError, LowerErrorKind, build_body, build_mir};
pub use builder::{BodyBuilder, BuildError};
pub use program::{FnDecl, FnSig, MirProgram, ProgramError};
pub use syntax::{
    AssertKind, BinOp, CastKind, ConstValue, Constant, IndirectionKind, Operand, Place,
    ProjectionElem, Rvalue, Statement, StatementKind, SwitchTargets, Terminator, TerminatorKind,
    UnOp,
};
pub use verify::{
    ErrorSite, VerifyError, VerifyErrorKind, VerifyErrors, verify_body, verify_program,
};
pub use visit::{MutVisitor, PlaceContext, Visitor};
