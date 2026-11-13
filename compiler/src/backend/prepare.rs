//! Backend preparation shared by code generators.
//!
//! Function identity and symbol assignment belong at this boundary. Source
//! names are preserved, except that the C entry wrapper takes the `main`
//! symbol and a Xenon function named `main` is renamed out of its way.

use crate::backend::contract::BackendError;
use crate::index::IndexVec;
use crate::middle::ids::DefId;
use crate::middle::mir::{
    BodyBuilder, CastKind, Constant, FnDecl, FnSig, LocalDecl, MirPhase, MirProgram, Operand,
    Place, RETURN_PLACE, Rvalue, START_BLOCK, SourceInfo, TerminatorKind,
};
use crate::middle::typecheck::is_entry_return_type;
use crate::types::Type;

const ENTRY_WRAPPER_NAME: &str = ".xenon.entry.wrapper";

/// Adds the C entry wrapper `fn() -> i32` that calls the language entry.
///
/// The language entry returns `u8` or `()`; the wrapper turns its result
/// into the C `int` exit status: the `u8` zero-extended, or `0` for `()`.
/// The original entry identity remains the call target, and a Xenon
/// function named `main` is renamed out of the C symbol's way (see
/// [`Symbols`]).
///
/// The wrapper is ordinary Runtime MIR, so callers verify the prepared
/// program like any other.
pub fn prepare_program(program: &MirProgram) -> Result<MirProgram, BackendError> {
    let mut prepared = program.clone();
    let Some(entry) = program.entry() else {
        return Ok(prepared);
    };
    let invalid = |message: String| BackendError::InvalidMir(message);
    let entry_decl = program
        .decl(entry)
        .ok_or_else(|| invalid(format!("entry function {entry} has no declaration")))?;
    let output = entry_decl.sig.output.clone();
    if !entry_decl.sig.inputs.is_empty() || !is_entry_return_type(&output) {
        return Err(invalid(format!(
            "entry wrapper requires an `fn() -> u8` or `fn()` entry, found `fn() -> {output}`"
        )));
    }

    let span = entry_decl.span;
    let wrapper = prepared.declare(FnDecl {
        name: ENTRY_WRAPPER_NAME.to_owned(),
        sig: FnSig {
            inputs: Vec::new(),
            output: Type::Int(32),
        },
        span,
    });
    let mut builder = BodyBuilder::new(wrapper, span, Type::Int(32), []);
    let result = builder.new_local(LocalDecl::temp(output.clone(), span));
    let return_block = builder.new_block();
    let source_info = SourceInfo::outermost(span);
    builder.terminate(
        START_BLOCK,
        source_info,
        TerminatorKind::Call {
            func: entry,
            args: Vec::new(),
            destination: Place::from(result),
            target: Some(return_block),
        },
    );
    let status = if output.is_unit() {
        Rvalue::Use(Operand::constant(Constant::int(0, Type::Int(32))))
    } else {
        Rvalue::Cast(
            CastKind::IntToInt,
            Operand::Copy(Place::from(result)),
            Type::Int(32),
        )
    };
    builder.push_assign(return_block, source_info, Place::from(RETURN_PLACE), status);
    builder.terminate(return_block, source_info, TerminatorKind::Return);
    let mut body = builder
        .finish()
        .map_err(|error| invalid(error.to_string()))?;
    body.advance_phase(MirPhase::Runtime)
        .map_err(|error| invalid(error.to_string()))?;
    prepared
        .set_body(body)
        .map_err(|error| invalid(error.to_string()))?;
    prepared.set_entry(wrapper);
    Ok(prepared)
}

/// Stable backend symbols derived from the MIR declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbols {
    names: IndexVec<DefId, String>,
}

impl Symbols {
    /// Assigns symbols. When [`prepare_program`] added an entry wrapper, the
    /// wrapper takes the C `main` symbol and a Xenon function literally
    /// named `main` is renamed out of its way.
    pub fn for_program(program: &MirProgram) -> Self {
        let wrapper = program.entry().filter(|&entry| {
            program
                .decl(entry)
                .is_some_and(|decl| decl.name == ENTRY_WRAPPER_NAME)
        });
        let names = program
            .decls()
            .iter_enumerated()
            .map(|(id, decl)| match wrapper {
                Some(wrapper) if id == wrapper => "main".to_owned(),
                Some(_) if decl.name == "main" => "_xe.main".to_owned(),
                _ => decl.name.clone(),
            })
            .collect();
        Self { names }
    }

    /// Symbol assigned to `def_id`.
    pub fn name(&self, def_id: DefId) -> Option<&str> {
        self.names.get(def_id).map(String::as_str)
    }
}
