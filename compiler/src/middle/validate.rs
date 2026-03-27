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
        Stmt::Expr(expr) => validate_expr(expr),
        Stmt::Break(Some(expr)) => validate_expr(expr),
        _ => Ok(()),
    }
}

fn validate_expr(expr: &Expr) -> SemanticResult<()> {
    match expr {
        Expr::Loop { body } | Expr::CondLoop { body, .. } => {
            for s in body {
                validate_stmt(s)?;
            }
            Ok(())
        }
        Expr::IfElse {
            then_branch,
            else_branch,
            condition,
        } => {
            validate_expr(condition)?;
            validate_expr(then_branch)?;
            validate_expr(else_branch)
        }
        Expr::BinOp { lhs, rhs, .. } => {
            validate_expr(lhs)?;
            validate_expr(rhs)
        }
        Expr::UnaryOp { operand, .. } => validate_expr(operand),
        _ => Ok(()),
    }
}

fn check_constant_range(name: &str, value: i64, ty: &Type) -> SemanticResult<()> {
    let fits = match ty {
        Type::UInt(n) => {
            let n = *n;
            if value < 0 {
                false
            } else if n >= 64 {
                true
            } else {
                let v = value as u128;
                let max = (1u128 << n) - 1;
                v <= max
            }
        }
        Type::Int(n) => {
            let n = *n;
            if n >= 64 {
                true
            } else {
                let min = -(1i128 << (n - 1));
                let max = (1i128 << (n - 1)) - 1;
                let v = value as i128;
                v >= min && v <= max
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
