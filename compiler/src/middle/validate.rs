use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{Expr, ExprKind, Program, Stmt, StmtKind, Type};
use num_bigint::BigInt;

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
    match &stmt.kind {
        StmtKind::VarDecl(binding) => {
            if let Some(default) = &binding.default
                && let ExprKind::Int(v) = &default.kind
            {
                check_constant_range(
                    binding.name.as_deref().unwrap_or("_"),
                    v,
                    &binding.ty,
                    binding.span,
                )?;
            }
            if let Some(default) = &binding.default {
                validate_expr(default)?;
            }
            Ok(())
        }
        StmtKind::Return(expr) => validate_expr(expr),
        StmtKind::Assign { value, .. } => validate_expr(value),
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expr(condition)?;
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
        StmtKind::Expr(expr) => validate_expr(expr),
        StmtKind::Break(Some(expr)) => validate_expr(expr),
        StmtKind::Break(None) | StmtKind::Continue => Ok(()),
    }
}

fn validate_expr(expr: &Expr) -> SemanticResult<()> {
    match &expr.kind {
        ExprKind::Loop { body } => {
            for s in body {
                validate_stmt(s)?;
            }
            Ok(())
        }
        ExprKind::CondLoop {
            condition, body, ..
        } => {
            validate_expr(condition)?;
            for s in body {
                validate_stmt(s)?;
            }
            Ok(())
        }
        ExprKind::IfElse {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expr(condition)?;
            validate_expr(then_branch)?;
            validate_expr(else_branch)
        }
        ExprKind::BinOp { lhs, rhs, .. } => {
            validate_expr(lhs)?;
            validate_expr(rhs)
        }
        ExprKind::UnaryOp { operand, .. } => validate_expr(operand),
        ExprKind::Call { args, .. } => {
            for arg in args {
                validate_expr(arg)?;
            }
            Ok(())
        }
        ExprKind::Int(_) | ExprKind::Ident(_) => Ok(()),
    }
}

fn check_constant_range(
    name: &str,
    value: &BigInt,
    ty: &Type,
    span: crate::frontend::tokens::Span,
) -> SemanticResult<()> {
    let fits = match ty.bounds() {
        Some((min, max)) => value >= &min && value <= &max,
        None => true,
    };
    if fits {
        Ok(())
    } else {
        Err(SemanticError::ConstantOutOfRange {
            name: name.to_owned(),
            value: value.clone(),
            ty: ty.clone(),
            span,
        })
    }
}
