//! MIR construction: lowering [THIR](crate::middle::thir) to built MIR.
//!
//! THIR is fully resolved and typed, so construction never reports user
//! errors: every [`LowerError`] is a compiler bug. The output is in
//! [`MirPhase::Built`](super::MirPhase::Built) and contains no runtime
//! checks; flow checking and check insertion are later passes.
//!
//! # Strategy
//!
//! Like rustc, expressions are lowered in one of four *categories*, chosen
//! by what the consumer needs:
//!
//! - `as_place`: the memory location an expression denotes (`x`, `*p`);
//! - `as_operand`: a value that can be read later (a constant or a copy of
//!   a place);
//! - `as_rvalue`: a single computation to store somewhere (`a + b`,
//!   `x as i64`, `@x`);
//! - `into`: writes the value into a *fresh* destination, which is how
//!   calls, conditional expressions, and loops are lowered.
//!
//! Every lowering function takes the block to continue in and returns a
//! `Flow`: the block where control continues together with the result, or
//! `Flow::Diverge` when control never gets past the construct (`return`,
//! `break`, an infinite loop). Statements after a diverging one are not
//! lowered, so the CFG contains no dead code.
//!
//! # Evaluation order
//!
//! Operands are evaluated left to right. An operand that is a place is read
//! where the operation executes, which would observe side effects of the
//! operands after it; such places are copied into a temporary first when a
//! later operand has side effects (see `expr::has_side_effects`). The base
//! pointer of an assignment target is snapshotted the same way.
//!
//! # Module map
//!
//! - this module: entry points, the `Builder`, and `Flow`;
//! - `scope`: lexical scopes, storage markers, and loop frames;
//! - `expr`: the expression categories;
//! - `stmt`: blocks and statements;
//! - `control_flow`: `if`, loops, `break`, `continue`, and `return`.

/// Unwraps a [`LowerResult`], returning [`Flow::Diverge`] from the enclosing
/// function if the lowered construct diverges.
macro_rules! unpack {
    ($flow:expr) => {
        match $flow? {
            Flow::Continue(block, value) => (block, value),
            Flow::Diverge => return Ok(Flow::Diverge),
        }
    };
}

mod control_flow;
mod expr;
mod scope;
mod stmt;

use thiserror::Error;

use crate::index::{Idx, IndexVec};
use crate::middle::ids::{BindingId, DefId};
use crate::middle::thir::{self, ThirProgram};
use crate::types::Type;

use super::{
    BasicBlock, Body, BodyBuilder, BuildError, FnDecl, FnSig, Local, LocalDecl, MirProgram,
    ProgramError, START_BLOCK, SourceInfo, TerminatorKind,
};
use scope::Scopes;

/// An internal error raised while lowering a function to MIR.
///
/// THIR has been fully checked, so this always indicates a compiler bug.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("internal error while building MIR for `{function}`: {kind}")]
pub struct LowerError {
    /// The function being lowered.
    pub function: String,
    /// What went wrong.
    pub kind: LowerErrorKind,
}

/// The kinds of [`LowerError`].
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LowerErrorKind {
    #[error(transparent)]
    Build(#[from] BuildError),

    #[error(transparent)]
    Program(#[from] ProgramError),

    #[error("THIR function `{thir}` was declared as MIR function `{mir}`")]
    DefIdMismatch { thir: DefId, mir: DefId },

    #[error("binding `{0}` is used before its declaration")]
    UnboundBinding(BindingId),

    #[error("`break` outside of a loop")]
    BreakOutsideLoop,

    #[error("`continue` outside of a loop")]
    ContinueOutsideLoop,

    #[error("left more loops than were entered")]
    UnbalancedLoops,

    #[error("type `{0}` has no zero value")]
    NoZeroValue(Type),
}

/// Lowers a whole program.
///
/// Functions keep their THIR [`DefId`]s, so identifiers can be shared
/// between diagnostics of both IRs.
pub fn build_mir(program: &ThirProgram) -> Result<MirProgram, LowerError> {
    let mut mir = MirProgram::new();
    for (def_id, function) in program.functions.iter_enumerated() {
        let declared = mir.declare(FnDecl {
            name: function.name.clone(),
            sig: FnSig {
                inputs: function.param_tys().cloned().collect(),
                output: function.return_ty.clone(),
            },
            span: function.span,
        });
        if declared != def_id {
            return Err(LowerError::new(
                function,
                LowerErrorKind::DefIdMismatch {
                    thir: def_id,
                    mir: declared,
                },
            ));
        }
    }
    for (def_id, function) in program.functions.iter_enumerated() {
        let body = build_body(def_id, function)?;
        mir.set_body(body)
            .map_err(|error| LowerError::new(function, error.into()))?;
    }
    if let Some(entry) = program.entry {
        mir.set_entry(entry);
    }
    Ok(mir)
}

/// Lowers the body of one function.
pub fn build_body(def_id: DefId, function: &thir::Function) -> Result<Body, LowerError> {
    Builder::new(def_id, function)
        .build()
        .map_err(|kind| LowerError::new(function, kind))
}

impl LowerError {
    fn new(function: &thir::Function, kind: LowerErrorKind) -> Self {
        Self {
            function: function.name.clone(),
            kind,
        }
    }
}

/// The result of lowering one construct.
#[must_use]
#[derive(Debug)]
enum Flow<T> {
    /// Control continues in the block, and the construct produced `T`.
    Continue(BasicBlock, T),
    /// Control never continues past the construct.
    Diverge,
}

/// The result of a lowering function; errors are compiler bugs.
type LowerResult<T> = Result<Flow<T>, LowerErrorKind>;

/// The state of lowering one function body.
#[derive(Debug)]
struct Builder<'thir> {
    function: &'thir thir::Function,
    cfg: BodyBuilder,
    /// The MIR local of every binding whose declaration has been lowered.
    locals: IndexVec<BindingId, Option<Local>>,
    scopes: Scopes,
}

impl<'thir> Builder<'thir> {
    fn new(def_id: DefId, function: &'thir thir::Function) -> Self {
        let args = function.params.iter().map(|&binding| {
            let binding = &function.bindings[binding];
            local_decl(binding, SourceInfo::outermost(binding.span))
        });
        let cfg = BodyBuilder::new(def_id, function.span, function.return_ty.clone(), args);

        let mut locals = IndexVec::from_elem_n(None, function.bindings.len());
        for (index, &binding) in function.params.iter().enumerate() {
            locals[binding] = Some(Local::new(index + 1));
        }

        Self {
            function,
            cfg,
            locals,
            scopes: Scopes::new(),
        }
    }

    fn build(mut self) -> Result<Body, LowerErrorKind> {
        let function = self.function;
        let source_info = SourceInfo::outermost(function.span);

        if let Some(binding) = function.named_return {
            let local = self.declare_binding(START_BLOCK, binding)?;
            let zero = self.zero_rvalue(&function.bindings[binding].ty)?;
            self.cfg
                .push_assign(START_BLOCK, source_info, local.into(), zero);
        }

        let flow = self.lower_block(START_BLOCK, &function.body)?;
        // Leaves the outermost scope, which holds the named return value.
        if let Flow::Continue(end, ()) = self.pop_scope(flow, function.span) {
            self.cfg
                .terminate(end, source_info, TerminatorKind::EndOfBody);
        }
        Ok(self.cfg.finish()?)
    }

    /// The MIR local of `binding`.
    fn local_of(&self, binding: BindingId) -> Result<Local, LowerErrorKind> {
        self.locals
            .get(binding)
            .copied()
            .flatten()
            .ok_or(LowerErrorKind::UnboundBinding(binding))
    }
}

/// The declaration of the MIR local of `binding`.
fn local_decl(binding: &thir::Binding, source_info: SourceInfo) -> LocalDecl {
    LocalDecl {
        ty: binding.ty.clone(),
        source_info,
        debug_name: binding.name.clone(),
    }
}
