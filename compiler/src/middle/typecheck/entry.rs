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
/// returning `u8` (the exit code) or `()` (exit code 0), and returns it.
///
/// POSIX systems only report the low 8 bits of an exit code, so `u8` is
/// the widest type whose every value reaches the parent process intact.
pub fn validate_entry_point(program: &ThirProgram) -> SemanticResult<DefId> {
    let entry = program.entry.ok_or(SemanticError::NoEntryPoint)?;
    let function = &program.functions[entry];
    if !function.params.is_empty() {
        return Err(SemanticError::EntryWithParams {
            span: function.span,
        });
    }
    if !is_entry_return_type(&function.return_ty) {
        return Err(SemanticError::EntryWrongReturn {
            found: function.return_ty.clone(),
            span: function.span,
        });
    }
    Ok(entry)
}

/// Whether an entry point may return `ty`: `u8` or `()`.
pub fn is_entry_return_type(ty: &Type) -> bool {
    *ty == Type::UInt(8) || ty.is_unit()
}
