//! The MIR verifier.
//!
//! The verifier checks that a body is well-formed MIR for its phase: every
//! index refers to something that exists, every operation is well-typed,
//! constants fit their types, calls match their callee's signature, and
//! phase-restricted constructs only appear where allowed.
//!
//! A verification failure is always a compiler bug (in lowering or in a
//! pass), never a user error. The driver runs the verifier after MIR
//! construction and after every pass in debug builds, so a broken pass is
//! caught right where it breaks the IR. All problems are collected rather
//! than stopping at the first one.

use std::collections::HashSet;
use std::fmt;

use num_bigint::BigInt;
use thiserror::Error;

use crate::index::Idx;
use crate::middle::ids::DefId;
use crate::middle::target::TargetSpec;
use crate::types::Type;

use super::body::{
    BasicBlock, Body, Local, LocalDecl, LocalKind, Location, MirPhase, SourceInfo, SourceScope,
};
use super::program::MirProgram;
use super::syntax::{
    ConstValue, Constant, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use super::typing::{self, TypingError};
use super::visit::{PlaceContext, Visitor};

/// Where in a body a verification error was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorSite {
    /// The body as a whole (signature, scopes, ...).
    Body,
    /// The declaration of a local.
    LocalDecl(Local),
    /// A statement or terminator.
    Location(Location),
}

impl fmt::Display for ErrorSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorSite::Body => f.write_str("body"),
            ErrorSite::LocalDecl(local) => write!(f, "declaration of `{local}`"),
            ErrorSite::Location(location) => write!(f, "{location}"),
        }
    }
}

/// A single violated MIR invariant.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum VerifyErrorKind {
    #[error("function has no declaration in the program")]
    MissingDecl,

    #[error("body has no basic blocks")]
    NoBlocks,

    #[error("body has {found} arguments but its signature has {expected}")]
    ArgCountMismatch { expected: usize, found: usize },

    #[error("return place has type `{found}` but the signature returns `{expected}`")]
    ReturnTypeMismatch { expected: Type, found: Type },

    #[error("argument `{local}` has type `{found}` but the signature expects `{expected}`")]
    ArgTypeMismatch {
        local: Local,
        expected: Type,
        found: Type,
    },

    #[error("reference to undeclared local `{0}`")]
    UnknownLocal(Local),

    #[error("jump to nonexistent block `{0}`")]
    UnknownBlock(BasicBlock),

    #[error("reference to nonexistent scope `{0}`")]
    UnknownScope(SourceScope),

    #[error("scope `{0}` has an invalid parent")]
    InvalidScopeParent(SourceScope),

    #[error(transparent)]
    Typing(TypingError),

    #[error("cannot assign a value of type `{rvalue}` to a place of type `{place}`")]
    AssignTypeMismatch { place: Type, rvalue: Type },

    #[error("storage markers are not allowed on `{0}`, which lives for the whole call")]
    StorageMarkerOnFixedLocal(Local),

    #[error("switch discriminant must be `bool` or an integer, found `{0}`")]
    InvalidSwitchDiscr(Type),

    #[error("switch value {value} does not fit the discriminant type `{ty}`")]
    SwitchValueOutOfRange { value: BigInt, ty: Type },

    #[error("switch value {0} appears more than once")]
    DuplicateSwitchValue(BigInt),

    #[error("assert condition must be `bool`, found `{0}`")]
    NonBoolAssertCond(Type),

    #[error("call to undeclared function `{0}`")]
    UnknownCallee(DefId),

    #[error("call to `{callee}` passes {found} arguments but it takes {expected}")]
    CallArgCountMismatch {
        callee: String,
        expected: usize,
        found: usize,
    },

    #[error("argument {index} of call to `{callee}` has type `{found}`, expected `{expected}`")]
    CallArgTypeMismatch {
        callee: String,
        index: usize,
        expected: Type,
        found: Type,
    },

    #[error("call to `{callee}` returns `{expected}` but its destination has type `{found}`")]
    CallDestinationMismatch {
        callee: String,
        expected: Type,
        found: Type,
    },

    #[error("constant value is not a valid `{0}`")]
    ConstantKindMismatch(Type),

    #[error("constant {value} does not fit type `{ty}`")]
    ConstantOutOfRange { value: BigInt, ty: Type },

    #[error("`{terminator}` is not allowed in phase `{phase}`")]
    InvalidInPhase {
        terminator: &'static str,
        phase: MirPhase,
    },
}

/// A violated MIR invariant, with its location.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("invalid MIR in `{function}` at {site}: {kind}")]
pub struct VerifyError {
    /// The function whose body is invalid.
    pub def_id: DefId,
    /// The function's name, for readability.
    pub function: String,
    /// Where in the body the problem is.
    pub site: ErrorSite,
    /// What is wrong.
    pub kind: VerifyErrorKind,
}

/// Every invariant violation found by a verification run.
#[derive(Debug, Clone, PartialEq, Error)]
pub struct VerifyErrors(pub Vec<VerifyError>);

impl fmt::Display for VerifyErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MIR verification found {} error(s)", self.0.len())?;
        for error in &self.0 {
            write!(f, "\n  {error}")?;
        }
        Ok(())
    }
}

/// Verifies every body of `program`.
pub fn verify_program(program: &MirProgram, target: &TargetSpec) -> Result<(), VerifyErrors> {
    let errors: Vec<_> = program
        .bodies()
        .flat_map(|body| collect_errors(program, body, target))
        .collect();
    into_result(errors)
}

/// Verifies a single body against the declarations of `program`.
pub fn verify_body(
    program: &MirProgram,
    body: &Body,
    target: &TargetSpec,
) -> Result<(), VerifyErrors> {
    into_result(collect_errors(program, body, target))
}

fn into_result(errors: Vec<VerifyError>) -> Result<(), VerifyErrors> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(VerifyErrors(errors))
    }
}

fn collect_errors(program: &MirProgram, body: &Body, target: &TargetSpec) -> Vec<VerifyError> {
    let mut checker = Checker {
        program,
        body,
        target,
        site: ErrorSite::Body,
        errors: Vec::new(),
    };
    checker.check_body_level();
    checker.visit_body(body);
    checker.errors
}

/// Errors that [`Checker::visit_place`] and [`Checker::visit_local`] report
/// for every place; other checks skip them to avoid duplicates.
fn reported_by_place_checks(error: &TypingError) -> bool {
    matches!(
        error,
        TypingError::UnknownLocal(_) | TypingError::DerefOfNonPointer(_)
    )
}

struct Checker<'a> {
    program: &'a MirProgram,
    body: &'a Body,
    target: &'a TargetSpec,
    site: ErrorSite,
    errors: Vec<VerifyError>,
}

impl Checker<'_> {
    fn report(&mut self, kind: VerifyErrorKind) {
        let def_id = self.body.def_id();
        let function = self
            .program
            .decl(def_id)
            .map_or_else(|| def_id.to_string(), |decl| decl.name.clone());
        self.errors.push(VerifyError {
            def_id,
            function,
            site: self.site,
            kind,
        });
    }

    fn report_typing(&mut self, error: TypingError) {
        if !reported_by_place_checks(&error) {
            self.report(VerifyErrorKind::Typing(error));
        }
    }

    /// Types an operand, reporting failures not already reported elsewhere.
    fn operand_ty(&mut self, operand: &Operand) -> Option<Type> {
        match typing::operand_ty(operand, self.body) {
            Ok(ty) => Some(ty.clone()),
            Err(error) => {
                self.report_typing(error);
                None
            }
        }
    }

    fn place_ty(&mut self, place: &Place) -> Option<Type> {
        match typing::place_ty(place, self.body) {
            Ok(ty) => Some(ty.clone()),
            Err(error) => {
                self.report_typing(error);
                None
            }
        }
    }

    // ---- Body-level checks ------------------------------------------------

    fn check_body_level(&mut self) {
        self.site = ErrorSite::Body;
        if self.body.basic_blocks().is_empty() {
            self.report(VerifyErrorKind::NoBlocks);
        }
        self.check_signature();
        self.check_scopes();
    }

    fn check_signature(&mut self) {
        let Some(decl) = self.program.decl(self.body.def_id()) else {
            self.report(VerifyErrorKind::MissingDecl);
            return;
        };
        let sig = &decl.sig;

        let return_ty = self.body.return_ty();
        if *return_ty != sig.output {
            self.report(VerifyErrorKind::ReturnTypeMismatch {
                expected: sig.output.clone(),
                found: return_ty.clone(),
            });
        }

        if self.body.arg_count() != sig.inputs.len() {
            self.report(VerifyErrorKind::ArgCountMismatch {
                expected: sig.inputs.len(),
                found: self.body.arg_count(),
            });
        }

        for (local, expected) in self.body.args_iter().zip(&sig.inputs) {
            let Some(arg) = self.body.local_decls().get(local) else {
                self.report(VerifyErrorKind::UnknownLocal(local));
                continue;
            };
            if arg.ty != *expected {
                self.report(VerifyErrorKind::ArgTypeMismatch {
                    local,
                    expected: expected.clone(),
                    found: arg.ty.clone(),
                });
            }
        }
    }

    fn check_scopes(&mut self) {
        for (scope, data) in self.body.source_scopes().iter_enumerated() {
            let valid = match data.parent {
                None => scope.index() == 0,
                // Parents are created before their children, which also
                // rules out cycles.
                Some(parent) => parent < scope,
            };
            if !valid {
                self.report(VerifyErrorKind::InvalidScopeParent(scope));
            }
        }
    }

    // ---- Location-level checks ---------------------------------------------

    fn check_block_exists(&mut self, block: BasicBlock) {
        if !self.body.basic_blocks().contains_index(block) {
            self.report(VerifyErrorKind::UnknownBlock(block));
        }
    }

    fn check_switch(&mut self, discr: &Operand, values: impl Iterator<Item = BigInt>) {
        let Some(ty) = self.operand_ty(discr) else {
            return;
        };
        let bounds = if ty.is_bool() {
            (BigInt::ZERO, BigInt::from(1))
        } else if let Some(bounds) = self.target.int_bounds(&ty) {
            bounds
        } else {
            self.report(VerifyErrorKind::InvalidSwitchDiscr(ty));
            return;
        };

        let mut seen = HashSet::new();
        for value in values {
            if value < bounds.0 || value > bounds.1 {
                self.report(VerifyErrorKind::SwitchValueOutOfRange {
                    value: value.clone(),
                    ty: ty.clone(),
                });
            }
            if !seen.insert(value.clone()) {
                self.report(VerifyErrorKind::DuplicateSwitchValue(value));
            }
        }
    }

    fn check_call(&mut self, func: DefId, args: &[Operand], destination: &Place) {
        let Some(decl) = self.program.decl(func) else {
            self.report(VerifyErrorKind::UnknownCallee(func));
            return;
        };
        let callee = decl.name.clone();
        let sig = decl.sig.clone();

        if args.len() != sig.inputs.len() {
            self.report(VerifyErrorKind::CallArgCountMismatch {
                callee: callee.clone(),
                expected: sig.inputs.len(),
                found: args.len(),
            });
        }
        for (index, (arg, expected)) in args.iter().zip(&sig.inputs).enumerate() {
            if let Some(found) = self.operand_ty(arg)
                && found != *expected
            {
                self.report(VerifyErrorKind::CallArgTypeMismatch {
                    callee: callee.clone(),
                    index,
                    expected: expected.clone(),
                    found,
                });
            }
        }
        if let Some(found) = self.place_ty(destination)
            && found != sig.output
        {
            self.report(VerifyErrorKind::CallDestinationMismatch {
                callee,
                expected: sig.output,
                found,
            });
        }
    }

    fn check_constant(&mut self, constant: &Constant) {
        let ty = &constant.ty;
        let range = match &constant.value {
            ConstValue::Bool(_) => {
                if !ty.is_bool() {
                    self.report(VerifyErrorKind::ConstantKindMismatch(ty.clone()));
                }
                return;
            }
            ConstValue::Float(_) => {
                if !ty.is_float() {
                    self.report(VerifyErrorKind::ConstantKindMismatch(ty.clone()));
                }
                return;
            }
            ConstValue::Int(value) => self.target.int_bounds(ty).map(|bounds| (value, bounds)),
            ConstValue::Address(value) => ty.is_indirect().then(|| {
                let width = self.target.pointer_width();
                (value, (BigInt::ZERO, (BigInt::from(1) << width) - 1))
            }),
        };
        match range {
            None => self.report(VerifyErrorKind::ConstantKindMismatch(ty.clone())),
            Some((value, (min, max))) if *value < min || *value > max => {
                self.report(VerifyErrorKind::ConstantOutOfRange {
                    value: value.clone(),
                    ty: ty.clone(),
                });
            }
            Some(_) => {}
        }
    }
}

impl Visitor for Checker<'_> {
    fn visit_local_decl(&mut self, local: Local, decl: &LocalDecl) {
        self.site = ErrorSite::LocalDecl(local);
        self.super_local_decl(local, decl);
    }

    fn visit_statement(&mut self, statement: &Statement, location: Location) {
        self.site = ErrorSite::Location(location);
        if let StatementKind::StorageLive(local) | StatementKind::StorageDead(local) =
            statement.kind
            && matches!(
                self.body.local_kind(local),
                LocalKind::ReturnPlace | LocalKind::Argument
            )
        {
            self.report(VerifyErrorKind::StorageMarkerOnFixedLocal(local));
        }
        self.super_statement(statement, location);
    }

    fn visit_assign(&mut self, place: &Place, rvalue: &Rvalue, location: Location) {
        let rvalue_ty = match typing::rvalue_ty(rvalue, self.body) {
            Ok(ty) => Some(ty),
            Err(error) => {
                self.report_typing(error);
                None
            }
        };
        // Silent: a malformed place is reported when the place is visited.
        let place_ty = typing::place_ty(place, self.body).ok();
        if let (Some(place_ty), Some(rvalue_ty)) = (place_ty, rvalue_ty)
            && *place_ty != rvalue_ty
        {
            self.report(VerifyErrorKind::AssignTypeMismatch {
                place: place_ty.clone(),
                rvalue: rvalue_ty,
            });
        }
        self.super_assign(place, rvalue, location);
    }

    fn visit_terminator(&mut self, terminator: &Terminator, location: Location) {
        self.site = ErrorSite::Location(location);
        for succ in terminator.kind.successors() {
            self.check_block_exists(succ);
        }
        match &terminator.kind {
            TerminatorKind::SwitchInt { discr, targets } => {
                self.check_switch(discr, targets.iter().map(|(value, _)| value.clone()));
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => self.check_call(*func, args, destination),
            TerminatorKind::Assert { cond, .. } => {
                if let Some(ty) = self.operand_ty(cond)
                    && !ty.is_bool()
                {
                    self.report(VerifyErrorKind::NonBoolAssertCond(ty));
                }
            }
            TerminatorKind::EndOfBody if self.body.phase() > MirPhase::Built => {
                self.report(VerifyErrorKind::InvalidInPhase {
                    terminator: "end_of_body",
                    phase: self.body.phase(),
                });
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::EndOfBody => {}
        }
        self.super_terminator(terminator, location);
    }

    fn visit_place(&mut self, place: &Place, context: PlaceContext, location: Location) {
        // Only the projection is checked here; an unknown base local is
        // reported once by `visit_local`.
        if let Err(TypingError::DerefOfNonPointer(ty)) = typing::place_ty(place, self.body) {
            self.report(VerifyErrorKind::Typing(TypingError::DerefOfNonPointer(ty)));
        }
        self.super_place(place, context, location);
    }

    fn visit_local(&mut self, local: &Local, _context: PlaceContext, _location: Location) {
        if !self.body.local_decls().contains_index(*local) {
            self.report(VerifyErrorKind::UnknownLocal(*local));
        }
    }

    fn visit_constant(&mut self, constant: &Constant, _location: Location) {
        self.check_constant(constant);
    }

    fn visit_source_info(&mut self, source_info: &SourceInfo) {
        if !self.body.source_scopes().contains_index(source_info.scope) {
            self.report(VerifyErrorKind::UnknownScope(source_info.scope));
        }
    }
}
