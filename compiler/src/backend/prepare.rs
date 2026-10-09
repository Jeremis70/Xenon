//! Backend preparation shared by code generators.
//!
//! Function identity and symbol assignment belong at this boundary. Current
//! source names are preserved except for the reserved C entry wrapper.

use std::collections::HashMap;

use crate::middle::ids::DefId;
use crate::middle::mir::{
    BodyBuilder, FnDecl, FnSig, MirPhase, MirProgram, Place, RETURN_PLACE, START_BLOCK, SourceInfo,
    TerminatorKind,
};
use crate::middle::target::TargetSpec;
use crate::source::Span;
use crate::types::Type;

const ENTRY_WRAPPER_NAME: &str = ".xenon.entry.wrapper";

/// Adds a verified-MIR C entry wrapper when the language entry has another
/// name. The original entry identity remains the call target.
pub fn prepare_program(program: &MirProgram, target: &TargetSpec) -> Result<MirProgram, String> {
    let Some(entry) = program.entry() else {
        return Ok(program.clone());
    };
    let entry_decl = program
        .decl(entry)
        .ok_or_else(|| format!("entry function {entry:?} has no declaration"))?;
    if entry_decl.name == "main" {
        return Ok(program.clone());
    }
    if !entry_decl.sig.inputs.is_empty() || entry_decl.sig.output != Type::Int(32) {
        return Err("entry wrapper requires an `fn() -> i32` entry".to_owned());
    }
    let span = program
        .body(entry)
        .map(|body| body.span())
        .unwrap_or(Span::ZERO);
    let mut prepared = program.clone();
    let wrapper = prepared.declare(FnDecl {
        name: ENTRY_WRAPPER_NAME.to_owned(),
        sig: FnSig {
            inputs: Vec::new(),
            output: Type::Int(32),
        },
        span,
    });
    let mut builder = BodyBuilder::new(wrapper, span, Type::Int(32), []);
    let return_block = builder.new_block();
    let source_info = SourceInfo::outermost(span);
    builder.terminate(
        START_BLOCK,
        source_info,
        TerminatorKind::Call {
            func: entry,
            args: Vec::new(),
            destination: Place::from(RETURN_PLACE),
            target: Some(return_block),
        },
    );
    builder.terminate(return_block, source_info, TerminatorKind::Return);
    let mut body = builder.finish().map_err(|error| error.to_string())?;
    body.advance_phase(MirPhase::Runtime)
        .map_err(|error| error.to_string())?;
    prepared.set_body(body).map_err(|error| error.to_string())?;
    prepared.set_entry(wrapper);
    crate::middle::mir::verify_program(&prepared, target).map_err(|errors| errors.to_string())?;
    Ok(prepared)
}

/// Stable backend symbols derived from the MIR declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbols {
    names: HashMap<DefId, String>,
    /// The language entry function, when one is marked.
    pub entry: Option<DefId>,
}

impl Symbols {
    /// Assigns symbols while avoiding a collision between the C entry symbol
    /// and a Xenon function literally named `main`.
    pub fn for_program(program: &MirProgram) -> Self {
        let entry = program.entry();
        let has_entry_wrapper = entry
            .and_then(|id| program.decl(id).map(|decl| (id, decl.name.as_str())))
            .is_some_and(|(_, name)| name == ENTRY_WRAPPER_NAME);
        let names = program
            .decls()
            .iter_enumerated()
            .map(|(id, decl)| {
                let name = if Some(id) == entry && has_entry_wrapper {
                    "main".to_owned()
                } else if has_entry_wrapper && decl.name == "main" {
                    "_xe.main".to_owned()
                } else {
                    decl.name.clone()
                };
                (id, name)
            })
            .collect();
        Self { names, entry }
    }

    /// Symbol assigned to `def_id`.
    pub fn name(&self, def_id: DefId) -> Option<&str> {
        self.names.get(&def_id).map(String::as_str)
    }
}
