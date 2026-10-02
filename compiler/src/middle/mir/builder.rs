//! Incremental construction of MIR bodies.
//!
//! [`BodyBuilder`] is the only way to create a [`Body`]. It hands out
//! blocks, locals, and scopes, and guarantees that a finished body has a
//! terminator on every block. Misuse (pushing into a terminated block,
//! terminating twice, referencing a block that does not exist) is recorded
//! rather than panicking and is reported by [`BodyBuilder::finish`]; such
//! misuse is always a bug in the lowering code, never in the user program.

use thiserror::Error;

use crate::index::IndexVec;
use crate::middle::ids::DefId;
use crate::source::Span;
use crate::types::Type;

use super::body::{
    BasicBlock, BasicBlockData, Body, Local, LocalDecl, OUTERMOST_SOURCE_SCOPE, SourceInfo,
    SourceScope, SourceScopeData,
};
use super::syntax::{Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind};
use super::typing::HasLocalDecls;

/// A misuse of [`BodyBuilder`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    #[error("block `{0}` does not exist")]
    UnknownBlock(BasicBlock),

    #[error("block `{0}` is already terminated")]
    AlreadyTerminated(BasicBlock),

    #[error("block `{0}` was never terminated")]
    Unterminated(BasicBlock),

    #[error("scope `{0}` does not exist")]
    UnknownScope(SourceScope),
}

/// A block whose terminator may not be known yet.
#[derive(Debug, Clone, Default)]
struct PartialBlock {
    statements: Vec<Statement>,
    terminator: Option<Terminator>,
}

/// Builds a [`Body`] block by block.
#[derive(Debug)]
pub struct BodyBuilder {
    def_id: DefId,
    span: Span,
    blocks: IndexVec<BasicBlock, PartialBlock>,
    local_decls: IndexVec<Local, LocalDecl>,
    arg_count: usize,
    source_scopes: IndexVec<SourceScope, SourceScopeData>,
    error: Option<BuildError>,
}

impl BodyBuilder {
    /// Starts a body for `def_id`.
    ///
    /// Declares the return place `_0` and the arguments `_1..=_n` from
    /// `args`, the outermost scope, and the start block `bb0`.
    pub fn new(
        def_id: DefId,
        span: Span,
        return_ty: Type,
        args: impl IntoIterator<Item = LocalDecl>,
    ) -> Self {
        let mut local_decls = IndexVec::new();
        local_decls.push(LocalDecl::temp(return_ty, span));
        local_decls.extend(args);
        let arg_count = local_decls.len() - 1;

        let mut source_scopes = IndexVec::new();
        source_scopes.push(SourceScopeData { parent: None, span });

        let mut blocks = IndexVec::new();
        blocks.push(PartialBlock::default());

        Self {
            def_id,
            span,
            blocks,
            local_decls,
            arg_count,
            source_scopes,
            error: None,
        }
    }

    /// The function the body is built for.
    pub fn def_id(&self) -> DefId {
        self.def_id
    }

    /// Opens a lexical scope nested in `parent`.
    pub fn new_scope(&mut self, parent: SourceScope, span: Span) -> SourceScope {
        if !self.source_scopes.contains_index(parent) {
            self.record(BuildError::UnknownScope(parent));
        }
        self.source_scopes.push(SourceScopeData {
            parent: Some(parent),
            span,
        })
    }

    /// Declares a local.
    pub fn new_local(&mut self, decl: LocalDecl) -> Local {
        self.local_decls.push(decl)
    }

    /// Declares an unnamed temporary in the outermost scope.
    pub fn new_temp(&mut self, ty: Type, span: Span) -> Local {
        self.local_decls.push(LocalDecl {
            ty,
            source_info: SourceInfo {
                span,
                scope: OUTERMOST_SOURCE_SCOPE,
            },
            debug_name: None,
        })
    }

    /// Creates an empty, unterminated block.
    pub fn new_block(&mut self) -> BasicBlock {
        self.blocks.push(PartialBlock::default())
    }

    /// Returns `true` if `block` exists and already has a terminator.
    pub fn is_terminated(&self, block: BasicBlock) -> bool {
        self.blocks
            .get(block)
            .is_some_and(|data| data.terminator.is_some())
    }

    /// Appends a statement to `block`.
    pub fn push(&mut self, block: BasicBlock, statement: Statement) {
        if let Some(data) = self.open_block(block) {
            data.statements.push(statement);
        }
    }

    /// Appends `place = rvalue` to `block`.
    pub fn push_assign(
        &mut self,
        block: BasicBlock,
        source_info: SourceInfo,
        place: Place,
        rvalue: Rvalue,
    ) {
        self.push_kind(
            block,
            source_info,
            StatementKind::Assign(Box::new((place, rvalue))),
        );
    }

    /// Appends `StorageLive(local)` to `block`.
    pub fn storage_live(&mut self, block: BasicBlock, source_info: SourceInfo, local: Local) {
        self.push_kind(block, source_info, StatementKind::StorageLive(local));
    }

    /// Appends `StorageDead(local)` to `block`.
    pub fn storage_dead(&mut self, block: BasicBlock, source_info: SourceInfo, local: Local) {
        self.push_kind(block, source_info, StatementKind::StorageDead(local));
    }

    /// Ends `block` with a terminator of `kind`.
    pub fn terminate(&mut self, block: BasicBlock, source_info: SourceInfo, kind: TerminatorKind) {
        if let Some(data) = self.open_block(block) {
            data.terminator = Some(Terminator { source_info, kind });
        }
    }

    /// Ends `block` with `goto -> target`.
    pub fn goto(&mut self, block: BasicBlock, source_info: SourceInfo, target: BasicBlock) {
        self.terminate(block, source_info, TerminatorKind::Goto { target });
    }

    /// Completes the body.
    ///
    /// Fails with the first misuse recorded during construction, or if any
    /// block is still unterminated.
    pub fn finish(self) -> Result<Body, BuildError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut blocks = IndexVec::with_capacity(self.blocks.len());
        for (block, data) in self.blocks.into_iter_enumerated() {
            let terminator = data.terminator.ok_or(BuildError::Unterminated(block))?;
            blocks.push(BasicBlockData {
                statements: data.statements,
                terminator,
            });
        }
        Ok(Body::new(
            self.def_id,
            blocks,
            self.local_decls,
            self.arg_count,
            self.source_scopes,
            self.span,
        ))
    }

    fn push_kind(&mut self, block: BasicBlock, source_info: SourceInfo, kind: StatementKind) {
        self.push(block, Statement { source_info, kind });
    }

    /// Returns `block` if it exists and can still receive statements.
    fn open_block(&mut self, block: BasicBlock) -> Option<&mut PartialBlock> {
        let error = match self.blocks.get(block) {
            None => BuildError::UnknownBlock(block),
            Some(data) if data.terminator.is_some() => BuildError::AlreadyTerminated(block),
            Some(_) => return self.blocks.get_mut(block),
        };
        self.record(error);
        None
    }

    fn record(&mut self, error: BuildError) {
        self.error.get_or_insert(error);
    }
}

impl HasLocalDecls for BodyBuilder {
    fn local_decls(&self) -> &IndexVec<Local, LocalDecl> {
        &self.local_decls
    }
}
