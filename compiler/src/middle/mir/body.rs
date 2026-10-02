//! The MIR body: the control-flow graph of a single function.

use std::cell::OnceCell;
use std::fmt;

use thiserror::Error;

use crate::index::{Idx, IndexVec, newtype_index};
use crate::middle::ids::DefId;
use crate::source::Span;
use crate::types::Type;

use super::syntax::{Statement, Terminator};

newtype_index! {
    /// A local variable slot of a [`Body`]: return place, argument,
    /// user variable, or compiler temporary.
    pub struct Local = "_{}";
}

newtype_index! {
    /// A node of the control-flow graph of a [`Body`].
    pub struct BasicBlock = "bb{}";
}

newtype_index! {
    /// A lexical scope, used to attach debug information to MIR.
    pub struct SourceScope = "scope{}";
}

/// The local holding the function's return value. It is always `_0`.
pub const RETURN_PLACE: Local = Local::from_u32(0);

/// The block where execution of every body starts. It is always `bb0`.
pub const START_BLOCK: BasicBlock = BasicBlock::from_u32(0);

/// The scope enclosing the whole function body. It is always `scope0`.
pub const OUTERMOST_SOURCE_SCOPE: SourceScope = SourceScope::from_u32(0);

/// The stage a [`Body`] has reached in the MIR pipeline.
///
/// Phases only move forward. Each phase strengthens the invariants the
/// verifier enforces, so a pass can rely on the guarantees of the phase it
/// declares it runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirPhase {
    /// Freshly lowered from THIR. May contain `EndOfBody` terminators marking
    /// paths that fall off the end of the function.
    Built,
    /// Flow-sensitive checks have run; `EndOfBody` has been eliminated.
    Checked,
    /// Lowered for code generation: runtime checks are explicit, and the
    /// body is ready for optimizations and backends.
    Runtime,
}

impl fmt::Display for MirPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MirPhase::Built => "built",
            MirPhase::Checked => "checked",
            MirPhase::Runtime => "runtime",
        })
    }
}

/// Where a MIR element came from in the source program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceInfo {
    /// Source range used for diagnostics.
    pub span: Span,
    /// Lexical scope used for debug information.
    pub scope: SourceScope,
}

impl SourceInfo {
    /// Source info in the outermost scope of the body.
    pub const fn outermost(span: Span) -> Self {
        Self {
            span,
            scope: OUTERMOST_SOURCE_SCOPE,
        }
    }
}

/// Metadata of a lexical scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScopeData {
    /// The enclosing scope, `None` only for the outermost scope.
    pub parent: Option<SourceScope>,
    /// The source range covered by the scope.
    pub span: Span,
}

/// Declaration of a local variable slot.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalDecl {
    /// The type of values stored in the local.
    pub ty: Type,
    /// Where the local was declared.
    pub source_info: SourceInfo,
    /// The source-level name, if the local corresponds to a user binding.
    pub debug_name: Option<String>,
}

impl LocalDecl {
    /// A compiler-introduced local without a source name.
    pub fn temp(ty: Type, span: Span) -> Self {
        Self {
            ty,
            source_info: SourceInfo::outermost(span),
            debug_name: None,
        }
    }

    /// A local corresponding to the user binding `name`.
    pub fn named(ty: Type, name: impl Into<String>, source_info: SourceInfo) -> Self {
        Self {
            ty,
            source_info,
            debug_name: Some(name.into()),
        }
    }
}

/// The role a [`Local`] plays in its body, derived from its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalKind {
    /// `_0`, the return value.
    ReturnPlace,
    /// `_1..=_arg_count`, the function parameters.
    Argument,
    /// A local carrying a source-level name.
    UserVariable,
    /// A compiler-introduced temporary.
    Temporary,
}

/// A program point: the statement at `statement_index` of `block`, or its
/// terminator when `statement_index == statements.len()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    /// The block containing the program point.
    pub block: BasicBlock,
    /// Index of the statement within the block.
    pub statement_index: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.block, self.statement_index)
    }
}

/// A straight-line sequence of statements ended by a single terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlockData {
    /// Statements executed in order when the block is entered.
    pub statements: Vec<Statement>,
    /// The control-flow transfer ending the block.
    pub terminator: Terminator,
}

impl BasicBlockData {
    /// Location of the terminator of `block`, given that this is its data.
    pub fn terminator_location(&self, block: BasicBlock) -> Location {
        Location {
            block,
            statement_index: self.statements.len(),
        }
    }
}

/// Predecessor lists, indexed by block.
pub type Predecessors = IndexVec<BasicBlock, Vec<BasicBlock>>;

/// The control-flow graph of a body, with cached derived analyses.
///
/// Mutable access goes through [`Body::basic_blocks_mut`], which invalidates
/// every cache, so cached results can never go stale.
#[derive(Debug, Clone)]
pub struct BasicBlocks {
    blocks: IndexVec<BasicBlock, BasicBlockData>,
    predecessors: OnceCell<Predecessors>,
}

impl BasicBlocks {
    fn new(blocks: IndexVec<BasicBlock, BasicBlockData>) -> Self {
        Self {
            blocks,
            predecessors: OnceCell::new(),
        }
    }

    /// Returns the predecessors of every block, computing them on first use.
    ///
    /// Each predecessor appears once per edge, so a block reached from both
    /// arms of a switch on the same source lists that source twice.
    pub fn predecessors(&self) -> &Predecessors {
        self.predecessors.get_or_init(|| {
            let mut preds = IndexVec::from_elem_n(Vec::new(), self.blocks.len());
            for (block, data) in self.blocks.iter_enumerated() {
                for succ in data.terminator.kind.successors() {
                    if let Some(list) = preds.get_mut(succ) {
                        list.push(block);
                    }
                }
            }
            preds
        })
    }
}

impl std::ops::Deref for BasicBlocks {
    type Target = IndexVec<BasicBlock, BasicBlockData>;

    fn deref(&self) -> &Self::Target {
        &self.blocks
    }
}

/// Error returned by [`Body::advance_phase`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("cannot move MIR from phase `{from}` back to phase `{to}`")]
pub struct PhaseError {
    /// The phase the body is in.
    pub from: MirPhase,
    /// The requested phase.
    pub to: MirPhase,
}

/// The MIR of one function.
///
/// A body owns its locals and its control-flow graph. Locals `_1` through
/// `_arg_count` are the parameters; `_0` holds the return value.
#[derive(Debug, Clone)]
pub struct Body {
    def_id: DefId,
    phase: MirPhase,
    basic_blocks: BasicBlocks,
    local_decls: IndexVec<Local, LocalDecl>,
    arg_count: usize,
    source_scopes: IndexVec<SourceScope, SourceScopeData>,
    span: Span,
}

impl Body {
    /// Assembles a body. Only the builder may do so, which guarantees every
    /// block is terminated and the local layout is well-formed.
    pub(crate) fn new(
        def_id: DefId,
        basic_blocks: IndexVec<BasicBlock, BasicBlockData>,
        local_decls: IndexVec<Local, LocalDecl>,
        arg_count: usize,
        source_scopes: IndexVec<SourceScope, SourceScopeData>,
        span: Span,
    ) -> Self {
        Self {
            def_id,
            phase: MirPhase::Built,
            basic_blocks: BasicBlocks::new(basic_blocks),
            local_decls,
            arg_count,
            source_scopes,
            span,
        }
    }

    /// The function this body defines.
    pub fn def_id(&self) -> DefId {
        self.def_id
    }

    /// The pipeline phase this body has reached.
    pub fn phase(&self) -> MirPhase {
        self.phase
    }

    /// Moves the body to a later phase.
    ///
    /// Advancing to the current phase is a no-op; moving backwards is an
    /// error, since earlier phases accept constructs later ones forbid.
    pub fn advance_phase(&mut self, to: MirPhase) -> Result<(), PhaseError> {
        if to < self.phase {
            return Err(PhaseError {
                from: self.phase,
                to,
            });
        }
        self.phase = to;
        Ok(())
    }

    /// The control-flow graph.
    pub fn basic_blocks(&self) -> &BasicBlocks {
        &self.basic_blocks
    }

    /// Mutable access to the control-flow graph; invalidates CFG caches.
    pub fn basic_blocks_mut(&mut self) -> &mut IndexVec<BasicBlock, BasicBlockData> {
        self.basic_blocks.predecessors = OnceCell::new();
        &mut self.basic_blocks.blocks
    }

    /// Declarations of every local, indexed by [`Local`].
    pub fn local_decls(&self) -> &IndexVec<Local, LocalDecl> {
        &self.local_decls
    }

    /// Mutable access to local declarations, e.g. to rewrite types.
    pub fn local_decls_mut(&mut self) -> &mut IndexVec<Local, LocalDecl> {
        &mut self.local_decls
    }

    /// Declares a fresh temporary of type `ty`.
    pub fn new_temp(&mut self, ty: Type, span: Span) -> Local {
        self.local_decls.push(LocalDecl::temp(ty, span))
    }

    /// Number of function parameters.
    pub fn arg_count(&self) -> usize {
        self.arg_count
    }

    /// The locals holding the function parameters, in order.
    pub fn args_iter(&self) -> impl ExactSizeIterator<Item = Local> + use<> {
        (1..self.arg_count + 1).map(Local::new)
    }

    /// The type of the return place `_0`.
    pub fn return_ty(&self) -> &Type {
        &self.local_decls[RETURN_PLACE].ty
    }

    /// Classifies `local` by its position and debug name.
    pub fn local_kind(&self, local: Local) -> LocalKind {
        let index = local.index();
        if index == 0 {
            LocalKind::ReturnPlace
        } else if index <= self.arg_count {
            LocalKind::Argument
        } else if self
            .local_decls
            .get(local)
            .is_some_and(|decl| decl.debug_name.is_some())
        {
            LocalKind::UserVariable
        } else {
            LocalKind::Temporary
        }
    }

    /// Lexical scopes, indexed by [`SourceScope`].
    pub fn source_scopes(&self) -> &IndexVec<SourceScope, SourceScopeData> {
        &self.source_scopes
    }

    /// The source range of the whole function.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the terminator location of `block`, if the block exists.
    pub fn terminator_loc(&self, block: BasicBlock) -> Option<Location> {
        self.basic_blocks
            .get(block)
            .map(|data| data.terminator_location(block))
    }
}
