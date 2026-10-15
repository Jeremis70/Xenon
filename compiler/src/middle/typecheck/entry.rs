//! Validation of the executable entry point.
//!
//! Item collection already rejected unknown attributes and duplicate
//! `#[entry]` functions. What remains is the contract an executable needs
//! from its entry point, checked on THIR. It is kept apart from type
//! checking because only executables need an entry point.

use crate::error::{SemanticError, SemanticResult};
use crate::middle::ids::DefId;
use crate::middle::thir::ThirProgram;
use crate::types::Type;

/// Checks that `program` has an entry point taking no parameters and
/// returning `i32`, and returns it.
pub fn validate_entry_point(program: &ThirProgram) -> SemanticResult<DefId> {
    let entry = program.entry.ok_or(SemanticError::NoEntryPoint)?;
    let function = &program.functions[entry];
    if !function.params.is_empty() {
        return Err(SemanticError::EntryWithParams {
            span: function.span,
        });
    }
    if function.return_ty != Type::Int(32) {
        return Err(SemanticError::EntryWrongReturn {
            span: function.span,
        });
    }
    Ok(entry)
}
