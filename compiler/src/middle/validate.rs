use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{Expr, Program, Stmt, Type};
use crate::frontend::tokens::Span;

/// Walks the program and checks that integer literal constants fit within the
/// declared type of their binding. Returns an error on the first violation.
pub fn validate_program(program: &Program) -> SemanticResult<()> {
    for f in &program.functions {
        for stmt in &f.body {
            validate_stmt(stmt)?;
        }
    }
    Ok(())
}

fn validate_stmt(stmt: &Stmt) -> SemanticResult<()> {
    match stmt {
        Stmt::VarDecl(binding) => {
            if let Some(default) = &binding.default
                && let Expr::Int(v) = default.as_ref()
            {
                check_constant_range(binding.name.as_deref().unwrap_or("_"), *v, &binding.ty)?;
            }
            Ok(())
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                validate_stmt(s)?;
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    validate_stmt(s)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_constant_range(name: &str, value: i64, ty: &Type) -> SemanticResult<()> {
    let fits = match ty {
        Type::UInt(n) => {
            let n = *n;
            if n >= 64 {
                value >= 0
            } else {
                value >= 0 && value < (1i64 << n)
            }
        }
        Type::Int(n) => {
            let n = *n;
            if n >= 64 {
                true
            } else {
                let min = -(1i64 << (n - 1));
                let max = (1i64 << (n - 1)) - 1;
                value >= min && value <= max
            }
        }
        _ => true,
    };
    if fits {
        Ok(())
    } else {
        Err(SemanticError::ConstantOutOfRange {
            name: name.to_owned(),
            value,
            ty: ty.clone(),
            span: Span { start: 0, end: 0 },
        })
    }
}
