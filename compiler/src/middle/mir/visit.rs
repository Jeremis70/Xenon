//! Visitors over MIR bodies.
//!
//! [`Visitor`] and [`MutVisitor`] are generated from a single macro so the
//! traversal logic exists exactly once. Each `visit_*` hook defaults to the
//! matching `super_*` method, which walks the children. An implementation
//! overrides the hooks it cares about and calls `super_*` to keep walking.
//!
//! The walk follows evaluation order: in `place = rvalue`, the rvalue is
//! visited before the place it is written to. Every local occurrence is
//! reported to [`Visitor::visit_local`] with a [`PlaceContext`] describing
//! how it is used, which is what liveness, initialization, and def-use
//! analyses are built on.

use super::body::{
    BasicBlock, BasicBlockData, Body, Local, LocalDecl, Location, RETURN_PLACE, SourceInfo,
};
use super::syntax::{
    Constant, Operand, Place, ProjectionElem, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};

/// How a place is used at a particular location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceContext {
    /// The value is read but not modified.
    NonMutatingUse(NonMutatingUseContext),
    /// The value may be modified.
    MutatingUse(MutatingUseContext),
    /// The local is mentioned without accessing its value.
    NonUse(NonUseContext),
}

/// Kinds of [`PlaceContext::NonMutatingUse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonMutatingUseContext {
    /// Read by an [`Operand::Copy`].
    Copy,
    /// The base local of a projection such as `(*_1)`: the pointer itself
    /// is read in order to reach the projected place.
    Projection,
    /// `_0` is read by a `return` terminator.
    Return,
}

/// Kinds of [`PlaceContext::MutatingUse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MutatingUseContext {
    /// Written by an assignment.
    Store,
    /// Written with the result of a call.
    Call,
    /// Its address is taken; it may be modified through the pointer.
    AddressOf,
}

/// Kinds of [`PlaceContext::NonUse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonUseContext {
    /// Mentioned by a `StorageLive` statement.
    StorageLive,
    /// Mentioned by a `StorageDead` statement.
    StorageDead,
}

impl PlaceContext {
    /// Returns `true` if the value of the place may be modified.
    pub fn is_mutating_use(self) -> bool {
        matches!(self, PlaceContext::MutatingUse(_))
    }

    /// Returns `true` if the value of the place is read or written.
    pub fn is_use(self) -> bool {
        !matches!(self, PlaceContext::NonUse(_))
    }

    /// Returns `true` for `StorageLive` and `StorageDead` mentions.
    pub fn is_storage_marker(self) -> bool {
        matches!(self, PlaceContext::NonUse(_))
    }
}

macro_rules! basic_blocks_iter {
    ($body:ident, mut) => {
        $body.basic_blocks_mut().iter_enumerated_mut()
    };
    ($body:ident,) => {
        $body.basic_blocks().iter_enumerated()
    };
}

macro_rules! local_decls_iter {
    ($body:ident, mut) => {
        $body.local_decls_mut().iter_enumerated_mut()
    };
    ($body:ident,) => {
        $body.local_decls().iter_enumerated()
    };
}

macro_rules! iter_items {
    ($items:expr, mut) => {
        $items.iter_mut()
    };
    ($items:expr,) => {
        $items.iter()
    };
}

macro_rules! make_mir_visitor {
    ($(#[$attr:meta])* $visitor:ident, $($mutability:ident)?) => {
        $(#[$attr])*
        pub trait $visitor {
            // ---- Hooks: override these. --------------------------------

            /// Visits a whole body: local declarations, then every block.
            fn visit_body(&mut self, body: &$($mutability)? Body) {
                self.super_body(body);
            }

            /// Visits the declaration of `local`.
            fn visit_local_decl(&mut self, local: Local, decl: &$($mutability)? LocalDecl) {
                self.super_local_decl(local, decl);
            }

            /// Visits a block's statements, then its terminator.
            fn visit_basic_block_data(
                &mut self,
                block: BasicBlock,
                data: &$($mutability)? BasicBlockData,
            ) {
                self.super_basic_block_data(block, data);
            }

            /// Visits a statement.
            fn visit_statement(
                &mut self,
                statement: &$($mutability)? Statement,
                location: Location,
            ) {
                self.super_statement(statement, location);
            }

            /// Visits `place = rvalue`.
            fn visit_assign(
                &mut self,
                place: &$($mutability)? Place,
                rvalue: &$($mutability)? Rvalue,
                location: Location,
            ) {
                self.super_assign(place, rvalue, location);
            }

            /// Visits a terminator.
            fn visit_terminator(
                &mut self,
                terminator: &$($mutability)? Terminator,
                location: Location,
            ) {
                self.super_terminator(terminator, location);
            }

            /// Visits an rvalue.
            fn visit_rvalue(&mut self, rvalue: &$($mutability)? Rvalue, location: Location) {
                self.super_rvalue(rvalue, location);
            }

            /// Visits an operand.
            fn visit_operand(&mut self, operand: &$($mutability)? Operand, location: Location) {
                self.super_operand(operand, location);
            }

            /// Visits a place used in `context`.
            fn visit_place(
                &mut self,
                place: &$($mutability)? Place,
                context: PlaceContext,
                location: Location,
            ) {
                self.super_place(place, context, location);
            }

            /// Visits one projection element of a place.
            fn visit_projection_elem(
                &mut self,
                _elem: &$($mutability)? ProjectionElem,
                _location: Location,
            ) {
            }

            /// Visits an occurrence of a local used in `context`.
            fn visit_local(
                &mut self,
                _local: &$($mutability)? Local,
                _context: PlaceContext,
                _location: Location,
            ) {
            }

            /// Visits a constant.
            fn visit_constant(&mut self, _constant: &$($mutability)? Constant, _location: Location) {
            }

            /// Visits the source info of a statement, terminator, or local.
            fn visit_source_info(&mut self, _source_info: &$($mutability)? SourceInfo) {}

            // ---- Structural walk: do not override. ----------------------

            /// Walks the children of a body.
            fn super_body(&mut self, body: &$($mutability)? Body) {
                for (local, decl) in local_decls_iter!(body, $($mutability)?) {
                    self.visit_local_decl(local, decl);
                }
                for (block, data) in basic_blocks_iter!(body, $($mutability)?) {
                    self.visit_basic_block_data(block, data);
                }
            }

            /// Walks the children of a local declaration.
            fn super_local_decl(&mut self, _local: Local, decl: &$($mutability)? LocalDecl) {
                let LocalDecl { ty: _, source_info, debug_name: _ } = decl;
                self.visit_source_info(source_info);
            }

            /// Walks the children of a block.
            fn super_basic_block_data(
                &mut self,
                block: BasicBlock,
                data: &$($mutability)? BasicBlockData,
            ) {
                let BasicBlockData { statements, terminator } = data;
                let terminator_index = statements.len();
                for (statement_index, statement) in
                    iter_items!(statements, $($mutability)?).enumerate()
                {
                    self.visit_statement(statement, Location { block, statement_index });
                }
                let location = Location { block, statement_index: terminator_index };
                self.visit_terminator(terminator, location);
            }

            /// Walks the children of a statement.
            fn super_statement(
                &mut self,
                statement: &$($mutability)? Statement,
                location: Location,
            ) {
                let Statement { source_info, kind } = statement;
                self.visit_source_info(source_info);
                match kind {
                    StatementKind::Assign(assign) => {
                        let (place, rvalue) = &$($mutability)? **assign;
                        self.visit_assign(place, rvalue, location);
                    }
                    StatementKind::StorageLive(local) => {
                        let context = PlaceContext::NonUse(NonUseContext::StorageLive);
                        self.visit_local(local, context, location);
                    }
                    StatementKind::StorageDead(local) => {
                        let context = PlaceContext::NonUse(NonUseContext::StorageDead);
                        self.visit_local(local, context, location);
                    }
                    StatementKind::Nop => {}
                }
            }

            /// Walks `place = rvalue`, the rvalue first.
            fn super_assign(
                &mut self,
                place: &$($mutability)? Place,
                rvalue: &$($mutability)? Rvalue,
                location: Location,
            ) {
                self.visit_rvalue(rvalue, location);
                let context = PlaceContext::MutatingUse(MutatingUseContext::Store);
                self.visit_place(place, context, location);
            }

            /// Walks the children of a terminator.
            fn super_terminator(
                &mut self,
                terminator: &$($mutability)? Terminator,
                location: Location,
            ) {
                let Terminator { source_info, kind } = terminator;
                self.visit_source_info(source_info);
                match kind {
                    TerminatorKind::Goto { .. }
                    | TerminatorKind::Unreachable
                    | TerminatorKind::EndOfBody => {}
                    TerminatorKind::SwitchInt { discr, targets: _ } => {
                        self.visit_operand(discr, location);
                    }
                    TerminatorKind::Call { func: _, args, destination, target: _ } => {
                        for arg in iter_items!(args, $($mutability)?) {
                            self.visit_operand(arg, location);
                        }
                        let context = PlaceContext::MutatingUse(MutatingUseContext::Call);
                        self.visit_place(destination, context, location);
                    }
                    TerminatorKind::Assert { cond, .. } => {
                        self.visit_operand(cond, location);
                    }
                    TerminatorKind::Return => {
                        // `_0` is implicit in `return`; rewriting it is meaningless,
                        // so a mutable visitor sees a scratch copy.
                        #[allow(unused_mut)]
                        let mut local = RETURN_PLACE;
                        let context = PlaceContext::NonMutatingUse(NonMutatingUseContext::Return);
                        self.visit_local(&$($mutability)? local, context, location);
                    }
                }
            }

            /// Walks the operands and places of an rvalue.
            fn super_rvalue(&mut self, rvalue: &$($mutability)? Rvalue, location: Location) {
                match rvalue {
                    Rvalue::Use(operand)
                    | Rvalue::UnaryOp(_, operand)
                    | Rvalue::Cast(_, operand, _) => self.visit_operand(operand, location),
                    Rvalue::BinaryOp(_, operands) | Rvalue::Overflows(_, operands) => {
                        let (lhs, rhs) = &$($mutability)? **operands;
                        self.visit_operand(lhs, location);
                        self.visit_operand(rhs, location);
                    }
                    Rvalue::AddressOf(_, place) => {
                        let context = PlaceContext::MutatingUse(MutatingUseContext::AddressOf);
                        self.visit_place(place, context, location);
                    }
                }
            }

            /// Walks the place or constant of an operand.
            fn super_operand(&mut self, operand: &$($mutability)? Operand, location: Location) {
                match operand {
                    Operand::Copy(place) => {
                        let context = PlaceContext::NonMutatingUse(NonMutatingUseContext::Copy);
                        self.visit_place(place, context, location);
                    }
                    Operand::Constant(constant) => self.visit_constant(constant, location),
                }
            }

            /// Walks the base local and the projections of a place.
            ///
            /// If the place goes through a pointer, the base local itself is
            /// only read, whatever happens to the pointee.
            fn super_place(
                &mut self,
                place: &$($mutability)? Place,
                context: PlaceContext,
                location: Location,
            ) {
                let local_context = if place.is_indirect() {
                    PlaceContext::NonMutatingUse(NonMutatingUseContext::Projection)
                } else {
                    context
                };
                let Place { local, projection } = place;
                self.visit_local(local, local_context, location);
                for elem in iter_items!(projection, $($mutability)?) {
                    self.visit_projection_elem(elem, location);
                }
            }
        }
    };
}

make_mir_visitor!(
    /// Read-only traversal of a MIR body.
    Visitor,
);
make_mir_visitor!(
    /// Traversal of a MIR body that may rewrite it in place.
    MutVisitor,
    mut
);
