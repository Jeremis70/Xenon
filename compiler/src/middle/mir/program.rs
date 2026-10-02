//! A whole program in MIR form: function declarations and their bodies.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::index::IndexVec;
use crate::middle::ids::DefId;
use crate::source::Span;
use crate::types::Type;

use super::body::Body;

/// The type signature of a function.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnSig {
    /// Parameter types, in order.
    pub inputs: Vec<Type>,
    /// Return type.
    pub output: Type,
}

/// A function known to the program, whether or not it has a MIR body.
///
/// Declarations exist independently of bodies so that calls can be typed
/// and verified before (or without) the callee being lowered, e.g. for
/// external functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    /// Source-level name, used for symbols and diagnostics.
    pub name: String,
    /// The function's signature.
    pub sig: FnSig,
    /// Where the function was declared.
    pub span: Span,
}

/// Error raised when a body is attached to the program inconsistently.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProgramError {
    #[error("MIR body for `{0}` has no matching function declaration")]
    UndeclaredFunction(DefId),

    #[error("function `{0}` already has a MIR body")]
    DuplicateBody(DefId),
}

/// The MIR of a whole program.
///
/// Bodies are stored in [`DefId`] order, so iteration (and therefore
/// pretty-printing and code generation) is deterministic.
#[derive(Debug, Clone, Default)]
pub struct MirProgram {
    decls: IndexVec<DefId, FnDecl>,
    bodies: BTreeMap<DefId, Body>,
    entry: Option<DefId>,
}

impl MirProgram {
    /// Creates an empty program.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a function and returns its identifier.
    pub fn declare(&mut self, decl: FnDecl) -> DefId {
        self.decls.push(decl)
    }

    /// All function declarations, indexed by [`DefId`].
    pub fn decls(&self) -> &IndexVec<DefId, FnDecl> {
        &self.decls
    }

    /// The declaration of `def_id`, if it exists.
    pub fn decl(&self, def_id: DefId) -> Option<&FnDecl> {
        self.decls.get(def_id)
    }

    /// Attaches the body of an already declared function.
    pub fn set_body(&mut self, body: Body) -> Result<(), ProgramError> {
        let def_id = body.def_id();
        if !self.decls.contains_index(def_id) {
            return Err(ProgramError::UndeclaredFunction(def_id));
        }
        if self.bodies.contains_key(&def_id) {
            return Err(ProgramError::DuplicateBody(def_id));
        }
        self.bodies.insert(def_id, body);
        Ok(())
    }

    /// The body of `def_id`, if it has one.
    pub fn body(&self, def_id: DefId) -> Option<&Body> {
        self.bodies.get(&def_id)
    }

    /// Mutable access to the body of `def_id`, if it has one.
    pub fn body_mut(&mut self, def_id: DefId) -> Option<&mut Body> {
        self.bodies.get_mut(&def_id)
    }

    /// Every body, in [`DefId`] order.
    pub fn bodies(&self) -> impl ExactSizeIterator<Item = &Body> + '_ {
        self.bodies.values()
    }

    /// Every body mutably, in [`DefId`] order.
    pub fn bodies_mut(&mut self) -> impl ExactSizeIterator<Item = &mut Body> + '_ {
        self.bodies.values_mut()
    }

    /// Splits the program into read-only declarations and mutable bodies,
    /// so a pass can rewrite bodies while consulting callee signatures.
    pub fn decls_and_bodies_mut(
        &mut self,
    ) -> (
        &IndexVec<DefId, FnDecl>,
        impl ExactSizeIterator<Item = &mut Body> + '_,
    ) {
        (&self.decls, self.bodies.values_mut())
    }

    /// Marks `def_id` as the program entry point.
    pub fn set_entry(&mut self, def_id: DefId) {
        self.entry = Some(def_id);
    }

    /// The program entry point, if one was set.
    pub fn entry(&self) -> Option<DefId> {
        self.entry
    }
}
