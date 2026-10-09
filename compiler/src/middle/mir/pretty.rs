//! Textual dumps of MIR, used by `--emit=mir`, snapshot tests, and
//! debugging.
//!
//! The format is modeled on rustc's MIR dumps and is fully deterministic:
//! bodies appear in [`DefId`](crate::middle::ids::DefId) order and every
//! entity is printed by its index.

use std::fmt::{self, Write};

use super::body::{Body, LocalKind};
use super::program::MirProgram;
use super::syntax::{Terminator, TerminatorKind, write_call};

const INDENT: &str = "    ";

/// Writes every body of `program`, separated by blank lines.
pub fn write_mir_program(program: &MirProgram, out: &mut dyn Write) -> fmt::Result {
    for (index, body) in program.bodies().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        write_body(program, body, out)?;
    }
    Ok(())
}

/// Writes a single body. `program` supplies function names.
pub fn write_body(program: &MirProgram, body: &Body, out: &mut dyn Write) -> fmt::Result {
    let name = program.fn_name(body.def_id());
    writeln!(out, "// MIR for `{name}` (phase: {})", body.phase())?;

    write!(out, "fn {name}(")?;
    for (index, arg) in body.args_iter().enumerate() {
        if index > 0 {
            out.write_str(", ")?;
        }
        write!(out, "{} {arg}", body.local_decls()[arg].ty)?;
    }
    writeln!(out, ") -> {} {{", body.return_ty())?;

    for (local, decl) in body.local_decls().iter_enumerated() {
        if body.local_kind(local) != LocalKind::Argument {
            writeln!(out, "{INDENT}let {} {local};", decl.ty)?;
        }
    }
    for (local, decl) in body.local_decls().iter_enumerated() {
        if let Some(debug_name) = &decl.debug_name {
            writeln!(out, "{INDENT}debug {debug_name} => {local};")?;
        }
    }

    for (block, data) in body.basic_blocks().iter_enumerated() {
        writeln!(out)?;
        writeln!(out, "{INDENT}{block}: {{")?;
        for statement in &data.statements {
            writeln!(out, "{INDENT}{INDENT}{};", statement.kind)?;
        }
        out.write_str(INDENT)?;
        out.write_str(INDENT)?;
        write_terminator(program, &data.terminator, out)?;
        writeln!(out, ";")?;
        writeln!(out, "{INDENT}}}")?;
    }
    writeln!(out, "}}")
}

/// Renders the whole program as a string.
pub fn mir_program_to_string(program: &MirProgram) -> String {
    let mut out = String::new();
    // Formatting into a `String` cannot fail.
    let _ = write_mir_program(program, &mut out);
    out
}

/// Writes a terminator, resolving callee names through `program`.
fn write_terminator(
    program: &MirProgram,
    terminator: &Terminator,
    out: &mut dyn Write,
) -> fmt::Result {
    match &terminator.kind {
        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
        } => write_call(out, program.fn_name(*func), args, destination, *target),
        kind => write!(out, "{kind}"),
    }
}
