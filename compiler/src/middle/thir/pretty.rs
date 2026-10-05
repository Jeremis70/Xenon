//! Textual dumps of THIR, used by snapshot tests and debugging.
//!
//! Statements are printed one per line and expressions inline, in a syntax
//! close to the source but with everything type checking decided spelled
//! out: bindings print as `name@id`, literals carry their type, implicit
//! conversions appear as `as` casts, and reference reads appear as `(*r)`.

use std::fmt::{self, Write};

use crate::middle::ids::{BindingId, DefId};
use crate::middle::ops::IndirectionKind;

use super::{
    Block, ConditionPlacement, Expr, ExprKind, Function, Literal, LoopCondition, Stmt, StmtKind,
    ThirProgram,
};

const INDENT: &str = "    ";

/// Writes every function of `program`, separated by blank lines.
pub fn write_thir_program(program: &ThirProgram, out: &mut dyn Write) -> fmt::Result {
    for (index, (def_id, _)) in program.functions.iter_enumerated().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        write_function(program, def_id, out)?;
    }
    Ok(())
}

/// Writes the function `def_id` of `program`.
pub fn write_function(program: &ThirProgram, def_id: DefId, out: &mut dyn Write) -> fmt::Result {
    let Some(function) = program.functions.get(def_id) else {
        return writeln!(out, "// unknown function {def_id}");
    };
    let mut printer = Printer {
        program,
        function,
        out,
        depth: 0,
    };
    printer.function(def_id)
}

/// Renders the whole program as a string.
pub fn thir_program_to_string(program: &ThirProgram) -> String {
    let mut out = String::new();
    // Formatting into a `String` cannot fail.
    let _ = write_thir_program(program, &mut out);
    out
}

struct Printer<'a> {
    program: &'a ThirProgram,
    function: &'a Function,
    out: &'a mut dyn Write,
    depth: usize,
}

impl Printer<'_> {
    fn function(&mut self, def_id: DefId) -> fmt::Result {
        let function = self.function;
        let entry = if self.program.entry == Some(def_id) {
            "#[entry] "
        } else {
            ""
        };
        write!(
            self.out,
            "// THIR for `{}` ({def_id})\n{entry}fn {}(",
            function.name, function.name
        )?;
        for (index, &param) in function.params.iter().enumerate() {
            if index > 0 {
                self.out.write_str(", ")?;
            }
            self.binding(param)?;
            write!(self.out, ": {}", function.bindings[param].ty)?;
        }
        write!(self.out, ") -> {}", function.return_ty)?;
        if let Some(named) = function.named_return {
            self.out.write_str(" ")?;
            self.binding(named)?;
        }
        self.out.write_str(" ")?;
        self.block(&function.body)?;
        writeln!(self.out)
    }

    fn block(&mut self, block: &Block) -> fmt::Result {
        writeln!(self.out, "{{")?;
        self.depth += 1;
        for stmt in &block.stmts {
            self.stmt(stmt)?;
        }
        self.depth -= 1;
        self.indent()?;
        self.out.write_str("}")
    }

    fn stmt(&mut self, stmt: &Stmt) -> fmt::Result {
        self.indent()?;
        match &stmt.kind {
            StmtKind::Let { binding, init } => {
                self.out.write_str("let ")?;
                self.binding(*binding)?;
                write!(self.out, ": {}", self.function.bindings[*binding].ty)?;
                if let Some(init) = init {
                    self.out.write_str(" = ")?;
                    self.expr(init)?;
                }
            }
            StmtKind::Expr(expr) => self.expr(expr)?,
            StmtKind::Assign { place, value } => {
                self.expr(place)?;
                self.out.write_str(" = ")?;
                self.expr(value)?;
            }
            StmtKind::CompoundAssign { op, place, value } => {
                self.expr(place)?;
                write!(self.out, " {}= ", op.name())?;
                self.expr(value)?;
            }
            StmtKind::Return(value) => {
                self.out.write_str("return ")?;
                self.expr(value)?;
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.out.write_str("if ")?;
                self.expr(condition)?;
                self.out.write_str(" ")?;
                self.block(then_block)?;
                if let Some(else_block) = else_block {
                    self.out.write_str(" else ")?;
                    self.block(else_block)?;
                }
                return writeln!(self.out);
            }
            StmtKind::Break(value) => {
                self.out.write_str("break")?;
                if let Some(value) = value {
                    self.out.write_str(" ")?;
                    self.expr(value)?;
                }
            }
            StmtKind::Continue => self.out.write_str("continue")?,
        }
        writeln!(self.out, ";")
    }

    fn expr(&mut self, expr: &Expr) -> fmt::Result {
        match &expr.kind {
            ExprKind::Literal(literal) => match literal {
                Literal::Bool(value) => write!(self.out, "{value}"),
                Literal::Int(value) => write!(self.out, "{value}_{}", expr.ty),
                Literal::Float(value) => write!(self.out, "{value:?}_{}", expr.ty),
                Literal::Address(value) => write!(self.out, "(@{value:#x} as {})", expr.ty),
            },
            ExprKind::Binding(binding) => self.binding(*binding),
            ExprKind::Deref(operand) => {
                self.out.write_str("(*")?;
                self.expr(operand)?;
                self.out.write_str(")")
            }
            ExprKind::AddressOf { kind, place } => {
                self.out.write_str(match kind {
                    IndirectionKind::Pointer => "&raw ",
                    IndirectionKind::Reference => "&",
                })?;
                self.expr(place)
            }
            ExprKind::Unary { op, operand } => {
                write!(self.out, "{}(", op.name())?;
                self.expr(operand)?;
                self.out.write_str(")")
            }
            ExprKind::Binary { op, lhs, rhs } => {
                write!(self.out, "{}(", op.name())?;
                self.expr(lhs)?;
                self.out.write_str(", ")?;
                self.expr(rhs)?;
                self.out.write_str(")")
            }
            ExprKind::Logical { op, lhs, rhs } => {
                write!(self.out, "Logical{}(", op.name())?;
                self.expr(lhs)?;
                self.out.write_str(", ")?;
                self.expr(rhs)?;
                self.out.write_str(")")
            }
            ExprKind::Cast { kind, operand } => {
                self.out.write_str("(")?;
                self.expr(operand)?;
                write!(self.out, " as {} [{kind:?}])", expr.ty)
            }
            ExprKind::Call { callee, args } => {
                let name = self
                    .program
                    .functions
                    .get(*callee)
                    .map_or_else(|| callee.to_string(), |f| f.name.clone());
                write!(self.out, "{name}(")?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.out.write_str(", ")?;
                    }
                    self.expr(arg)?;
                }
                self.out.write_str(")")
            }
            ExprKind::If {
                condition,
                then_expr,
                else_expr,
            } => {
                self.out.write_str("(if ")?;
                self.expr(condition)?;
                self.out.write_str(" { ")?;
                self.expr(then_expr)?;
                self.out.write_str(" } else { ")?;
                self.expr(else_expr)?;
                self.out.write_str(" })")
            }
            ExprKind::Loop { condition, body } => self.loop_expr(expr, condition.as_deref(), body),
        }
    }

    fn loop_expr(
        &mut self,
        expr: &Expr,
        condition: Option<&LoopCondition>,
        body: &Block,
    ) -> fmt::Result {
        write!(self.out, "(loop: {}) ", expr.ty)?;
        match condition {
            Some(condition) if condition.placement == ConditionPlacement::Before => {
                self.out.write_str(keyword(condition))?;
                self.out.write_str(" ")?;
                self.expr(&condition.expr)?;
                self.out.write_str(" ")?;
                self.block(body)
            }
            Some(condition) => {
                self.out.write_str("do ")?;
                self.block(body)?;
                write!(self.out, " {} ", keyword(condition))?;
                self.expr(&condition.expr)
            }
            None => {
                self.out.write_str("loop ")?;
                self.block(body)
            }
        }
    }

    fn binding(&mut self, binding: BindingId) -> fmt::Result {
        let name = self
            .function
            .bindings
            .get(binding)
            .and_then(|data| data.name.as_deref())
            .unwrap_or("_");
        write!(self.out, "{name}@{binding}")
    }

    fn indent(&mut self) -> fmt::Result {
        for _ in 0..self.depth {
            self.out.write_str(INDENT)?;
        }
        Ok(())
    }
}

fn keyword(condition: &LoopCondition) -> &'static str {
    if condition.continue_if {
        "while"
    } else {
        "until"
    }
}
