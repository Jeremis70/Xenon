//! Lexical scopes, storage markers, and loop frames.
//!
//! Every THIR block opens a lexical scope, which becomes a MIR
//! [`SourceScope`] and owns the locals declared in it. A local is marked
//! `StorageLive` at its declaration and `StorageDead` whenever control
//! leaves its scope: at the end of the block, or on a `break`, `continue`,
//! or `return` that jumps out of it. Temporaries get no storage markers.
//!
//! Loops push a [`LoopScope`] recording where `break` and `continue` jump.
//! Those blocks are created on first use, so a loop nobody breaks out of
//! has no exit block and diverges.

use crate::middle::ids::BindingId;
use crate::source::Span;

use super::super::{BasicBlock, Local, OUTERMOST_SOURCE_SCOPE, Place, SourceInfo, SourceScope};
use super::{Builder, Flow, LowerErrorKind, local_decl};

/// The scope stacks of a function being lowered.
#[derive(Debug)]
pub(super) struct Scopes {
    /// Open lexical scopes, outermost first. Never empty while lowering.
    lexical: Vec<LexicalScope>,
    /// Enclosing loops, outermost first.
    loops: Vec<LoopScope>,
}

impl Scopes {
    /// Starts with the function's outermost scope open.
    pub(super) fn new() -> Self {
        Self {
            lexical: vec![LexicalScope {
                source_scope: OUTERMOST_SOURCE_SCOPE,
                storage: Vec::new(),
            }],
            loops: Vec::new(),
        }
    }
}

/// An open lexical scope.
#[derive(Debug)]
struct LexicalScope {
    source_scope: SourceScope,
    /// Locals with storage markers declared in this scope, in order.
    storage: Vec<Local>,
}

/// An enclosing loop.
#[derive(Debug)]
pub(super) struct LoopScope {
    /// Where `break value` stores the loop's value.
    pub(super) destination: Place,
    /// Where `continue` jumps, once created.
    pub(super) continue_block: Option<BasicBlock>,
    /// Where `break` jumps, once created.
    pub(super) break_block: Option<BasicBlock>,
    /// The number of lexical scopes open outside the loop; `break` and
    /// `continue` leave every scope above this depth.
    depth: usize,
}

impl Builder<'_> {
    /// Source info for `span` in the innermost scope.
    pub(super) fn source_info(&self, span: Span) -> SourceInfo {
        let scope = self
            .scopes
            .lexical
            .last()
            .map_or(OUTERMOST_SOURCE_SCOPE, |scope| scope.source_scope);
        SourceInfo { span, scope }
    }

    /// Opens a lexical scope nested in the innermost one.
    pub(super) fn push_scope(&mut self, span: Span) {
        let parent = self.source_info(span).scope;
        let source_scope = self.cfg.new_scope(parent, span);
        self.scopes.lexical.push(LexicalScope {
            source_scope,
            storage: Vec::new(),
        });
    }

    /// Closes the innermost lexical scope. If control falls out of it, the
    /// storage of its locals ends there.
    pub(super) fn pop_scope(&mut self, flow: Flow<()>, span: Span) -> Flow<()> {
        if let Flow::Continue(block, ()) = flow {
            let depth = self.scopes.lexical.len().saturating_sub(1);
            self.exit_scopes(block, depth, span);
        }
        self.scopes.lexical.pop();
        flow
    }

    /// Ends the storage of every local in scopes deeper than `depth`,
    /// innermost first, without closing the scopes.
    pub(super) fn exit_scopes(&mut self, block: BasicBlock, depth: usize, span: Span) {
        let source_info = self.source_info(span);
        let scopes = self.scopes.lexical.get(depth..).unwrap_or_default();
        for &local in scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.storage.iter().rev())
        {
            self.cfg.storage_dead(block, source_info, local);
        }
    }

    /// The number of open lexical scopes.
    pub(super) fn scope_depth(&self) -> usize {
        self.scopes.lexical.len()
    }

    /// Declares the local of `binding` in the innermost scope and starts
    /// its storage in `block`.
    pub(super) fn declare_binding(
        &mut self,
        block: BasicBlock,
        binding: BindingId,
    ) -> Result<Local, LowerErrorKind> {
        let data = self
            .function
            .bindings
            .get(binding)
            .ok_or(LowerErrorKind::UnboundBinding(binding))?;
        let source_info = self.source_info(data.span);
        let local = self.cfg.new_local(local_decl(data, source_info));
        let slot = self
            .locals
            .get_mut(binding)
            .ok_or(LowerErrorKind::UnboundBinding(binding))?;
        *slot = Some(local);
        self.cfg.storage_live(block, source_info, local);
        if let Some(scope) = self.scopes.lexical.last_mut() {
            scope.storage.push(local);
        }
        Ok(local)
    }

    /// Enters a loop whose `break value` writes into `destination`.
    pub(super) fn push_loop(
        &mut self,
        destination: Place,
        continue_block: Option<BasicBlock>,
        break_block: Option<BasicBlock>,
    ) {
        let depth = self.scope_depth();
        self.scopes.loops.push(LoopScope {
            destination,
            continue_block,
            break_block,
            depth,
        });
    }

    /// Leaves the innermost loop.
    pub(super) fn pop_loop(&mut self) -> Result<LoopScope, LowerErrorKind> {
        self.scopes
            .loops
            .pop()
            .ok_or(LowerErrorKind::UnbalancedLoops)
    }

    /// The innermost loop, for a `break` inside it.
    pub(super) fn innermost_loop(&self) -> Result<&LoopScope, LowerErrorKind> {
        self.scopes
            .loops
            .last()
            .ok_or(LowerErrorKind::BreakOutsideLoop)
    }

    /// The scope depth of the innermost loop and the block `jump` goes to,
    /// created on first use.
    pub(super) fn loop_target(
        &mut self,
        jump: LoopJump,
    ) -> Result<(usize, BasicBlock), LowerErrorKind> {
        let frame = self.scopes.loops.last_mut().ok_or(match jump {
            LoopJump::Break => LowerErrorKind::BreakOutsideLoop,
            LoopJump::Continue => LowerErrorKind::ContinueOutsideLoop,
        })?;
        let target = match jump {
            LoopJump::Break => &mut frame.break_block,
            LoopJump::Continue => &mut frame.continue_block,
        };
        let block = *target.get_or_insert_with(|| self.cfg.new_block());
        Ok((frame.depth, block))
    }
}

/// A jump out of the current iteration of the innermost loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoopJump {
    /// `break`: leaves the loop.
    Break,
    /// `continue`: starts the next iteration.
    Continue,
}
