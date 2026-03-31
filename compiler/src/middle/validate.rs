use std::collections::HashMap;

use crate::error::{SemanticError, SemanticResult};
use crate::frontend::ast::{
    BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp,
};
use num_bigint::BigInt;

fn is_integer_type(ty: &Type) -> bool {
    matches!(ty, Type::Int(_) | Type::UInt(_) | Type::USize | Type::ISize)
}

fn is_float_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Float16 | Type::BFloat16 | Type::Float32 | Type::Float64 | Type::Float128
    )
}

fn is_bool_type(ty: &Type) -> bool {
    matches!(ty, Type::Bool)
}

/// Integer literal marker: only `ExprKind::Int` maps to this during inference.
fn type_of_int_literal() -> Type {
    Type::Int(64)
}

fn types_equal(a: &Type, b: &Type) -> bool {
    a == b
}

fn assert_assignable(
    expr_ty: &Type,
    target: &Type,
    span: crate::frontend::tokens::Span,
) -> SemanticResult<()> {
    if types_equal(expr_ty, target) {
        return Ok(());
    }
    // Allow int literal to any integer type (range checked separately for var decl).
    if matches!(expr_ty, Type::Int(64)) && is_integer_type(target) {
        return Ok(());
    }
    // Allow float literal to any float type.
    if matches!(expr_ty, Type::Float64) && is_float_type(target) {
        return Ok(());
    }
    Err(SemanticError::TypeMismatch {
        expected: target.to_string(),
        found: expr_ty.to_string(),
        span,
    })
}

fn unify_arithmetic(
    lhs: Type,
    rhs: Type,
    span: crate::frontend::tokens::Span,
) -> SemanticResult<Type> {
    match (&lhs, &rhs) {
        (l, r) if is_float_type(l) && is_float_type(r) => {
            if types_equal(l, r) {
                Ok(lhs)
            } else {
                Err(SemanticError::InvalidOperands {
                    op: "arithmetic".to_owned(),
                    detail: format!(
                        "mixed float types `{l}` and `{r}` (use explicit cast when available)"
                    ),
                    span,
                })
            }
        }
        (Type::Int(64), o) | (o, Type::Int(64)) if is_integer_type(o) => Ok(o.clone()),
        (l, r) if is_integer_type(l) && is_integer_type(r) && types_equal(l, r) => Ok(lhs),
        (l, r) if is_integer_type(l) && is_integer_type(r) => Err(SemanticError::InvalidOperands {
            op: "arithmetic".to_owned(),
            detail: format!("mixed integer types `{l}` and `{r}` without a literal to unify"),
            span,
        }),
        (l, r) => Err(SemanticError::InvalidOperands {
            op: "arithmetic".to_owned(),
            detail: format!("cannot combine `{l}` and `{r}`"),
            span,
        }),
    }
}

fn unify_compare(lhs: Type, rhs: Type, span: crate::frontend::tokens::Span) -> SemanticResult<()> {
    if (is_integer_type(&lhs) || matches!(lhs, Type::Int(64)))
        && (is_integer_type(&rhs) || matches!(rhs, Type::Int(64)))
    {
        return Ok(());
    }
    if let (Type::Int(64), r) | (r, Type::Int(64)) = (&lhs, &rhs)
        && is_integer_type(r)
    {
        return Ok(());
    }
    if (is_float_type(&lhs) || matches!(lhs, Type::Float64))
        && (is_float_type(&rhs) || matches!(rhs, Type::Float64))
    {
        return Ok(());
    }
    if is_bool_type(&lhs) && is_bool_type(&rhs) {
        return Ok(());
    }
    Err(SemanticError::InvalidOperands {
        op: "comparison".to_owned(),
        detail: format!("incompatible types `{lhs}` and `{rhs}`"),
        span,
    })
}

fn unify_shift(lhs: Type, rhs: Type, span: crate::frontend::tokens::Span) -> SemanticResult<Type> {
    match (&lhs, &rhs) {
        (l, r) if is_integer_type(l) && (is_integer_type(r) || matches!(r, Type::Int(64))) => {
            Ok(l.clone())
        }
        (Type::Int(64), r) if is_integer_type(r) => Ok(r.clone()),
        _ => Err(SemanticError::InvalidOperands {
            op: "shift".to_owned(),
            detail: format!("expected integer operands, got `{lhs}` and `{rhs}`"),
            span,
        }),
    }
}

/// Walks the program: type-checks, range-checks integer literals, and validates `bool` conditions.
pub fn validate_program(program: &Program) -> SemanticResult<()> {
    let fn_map: HashMap<String, &Function> = program
        .functions
        .iter()
        .map(|f| (f.name.clone(), f))
        .collect();

    for f in &program.functions {
        check_function(f, &fn_map)?;
    }
    Ok(())
}

struct Env<'a> {
    vars: HashMap<String, Type>,
    return_ty: Type,
    fn_map: &'a HashMap<String, &'a Function>,
    loop_stack: Vec<LoopFrame>,
}

struct LoopFrame {
    /// From `<type> x = loop ...` when applicable.
    contextual: Option<Type>,
    /// From `break expr` / `break;`.
    inferred: Option<Type>,
}

impl LoopFrame {
    fn merge_break(&mut self, ty: Type, span: crate::frontend::tokens::Span) -> SemanticResult<()> {
        match &self.inferred {
            None => {
                self.inferred = Some(ty);
                Ok(())
            }
            Some(et) if types_equal(et, &ty) => Ok(()),
            Some(et) => Err(SemanticError::BreakTypeConflict {
                earlier: et.to_string(),
                found: ty.to_string(),
                span,
            }),
        }
    }

    fn finish(self) -> Type {
        self.inferred.or(self.contextual).unwrap_or(Type::Int(64))
    }
}

fn check_function(f: &Function, fn_map: &HashMap<String, &Function>) -> SemanticResult<()> {
    let mut vars = HashMap::new();
    for p in &f.params {
        let name = p.name.as_deref().unwrap_or("");
        vars.insert(name.to_string(), p.ty.clone());
    }
    if let Some(n) = &f.return_type.name {
        vars.insert(n.clone(), f.return_type.ty.clone());
    }

    let mut env = Env {
        vars,
        return_ty: f.return_type.ty.clone(),
        fn_map,
        loop_stack: Vec::new(),
    };

    for s in &f.body {
        check_stmt(s, &mut env)?;
    }
    Ok(())
}

fn check_stmt(stmt: &Stmt, env: &mut Env) -> SemanticResult<()> {
    match &stmt.kind {
        StmtKind::VarDecl(binding) => {
            if let Some(default) = &binding.default {
                if let ExprKind::Int(v) = &default.kind {
                    check_constant_range(
                        binding.name.as_deref().unwrap_or("_"),
                        v,
                        &binding.ty,
                        binding.span,
                    )?;
                }
                let t = infer_expr_with_expect(default, env, Some(&binding.ty))?;
                assert_assignable(&t, &binding.ty, default.span)?;
            }
            let name = binding.name.as_deref().unwrap_or("");
            env.vars.insert(name.to_string(), binding.ty.clone());
            Ok(())
        }
        StmtKind::Return(expr) => {
            let t = infer_expr(expr, env)?;
            assert_assignable(&t, &env.return_ty, expr.span)?;
            Ok(())
        }
        StmtKind::Assign { name, value } => {
            let lhs_ty =
                env.vars
                    .get(name)
                    .cloned()
                    .ok_or_else(|| SemanticError::UndefinedVariable {
                        name: name.clone(),
                        span: stmt.span,
                    })?;
            let rhs_t = infer_expr(value, env)?;
            assert_assignable(&rhs_t, &lhs_ty, value.span)?;
            Ok(())
        }
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            require_bool_expr(condition, env)?;
            for s in then_branch {
                check_stmt(s, env)?;
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    check_stmt(s, env)?;
                }
            }
            Ok(())
        }
        StmtKind::Expr(expr) => {
            let _ = infer_expr(expr, env)?;
            Ok(())
        }
        StmtKind::Break(opt) => {
            let (ty, merge_span) = match opt {
                Some(e) => (infer_expr(e, env)?, e.span),
                None => (
                    env.loop_stack
                        .last()
                        .and_then(|f| f.contextual.clone())
                        .unwrap_or(Type::Int(64)),
                    stmt.span,
                ),
            };
            let frame = env
                .loop_stack
                .last_mut()
                .ok_or(SemanticError::BreakOutsideLoop { span: stmt.span })?;
            frame.merge_break(ty, merge_span)?;
            Ok(())
        }
        StmtKind::Continue => {
            if env.loop_stack.is_empty() {
                return Err(SemanticError::ContinueOutsideLoop { span: stmt.span });
            }
            Ok(())
        }
    }
}

fn require_bool_expr(expr: &Expr, env: &mut Env) -> SemanticResult<()> {
    let t = infer_expr(expr, env)?;
    if !is_bool_type(&t) {
        return Err(SemanticError::ConditionNotBool {
            found: t.to_string(),
            span: expr.span,
        });
    }
    Ok(())
}

fn infer_expr(expr: &Expr, env: &mut Env) -> SemanticResult<Type> {
    infer_expr_with_expect(expr, env, None)
}

fn infer_expr_with_expect(
    expr: &Expr,
    env: &mut Env,
    expect: Option<&Type>,
) -> SemanticResult<Type> {
    match &expr.kind {
        ExprKind::Int(_) => Ok(type_of_int_literal()),
        ExprKind::Bool(_) => Ok(Type::Bool),
        ExprKind::Float(_) => Ok(Type::Float64),
        ExprKind::Ident(name) => {
            env.vars
                .get(name)
                .cloned()
                .ok_or_else(|| SemanticError::UndefinedVariable {
                    name: name.clone(),
                    span: expr.span,
                })
        }
        ExprKind::UnaryOp { op, operand } => {
            let t = infer_expr(operand, env)?;
            match op {
                UnaryOp::Neg => {
                    if is_integer_type(&t)
                        || matches!(t, Type::Int(64))
                        || is_float_type(&t)
                        || matches!(t, Type::Float64)
                    {
                        Ok(t)
                    } else {
                        Err(SemanticError::InvalidOperands {
                            op: "-".to_owned(),
                            detail: format!("expected numeric type, found `{t}`"),
                            span: expr.span,
                        })
                    }
                }
                UnaryOp::Not => {
                    if is_bool_type(&t) {
                        Ok(Type::Bool)
                    } else {
                        Err(SemanticError::InvalidOperands {
                            op: "!".to_owned(),
                            detail: format!("expected `bool`, found `{t}`"),
                            span: expr.span,
                        })
                    }
                }
                UnaryOp::BitwiseNot => {
                    if is_integer_type(&t) || matches!(t, Type::Int(64)) {
                        Ok(t)
                    } else {
                        Err(SemanticError::InvalidOperands {
                            op: "~".to_owned(),
                            detail: format!("expected integer type, found `{t}`"),
                            span: expr.span,
                        })
                    }
                }
            }
        }
        ExprKind::BinOp { lhs, op, rhs } => {
            let lt = infer_expr(lhs, env)?;
            let rt = infer_expr(rhs, env)?;
            infer_binop(op, lt, rt, expr.span)
        }
        ExprKind::Call { name, args } => {
            let callee = env
                .fn_map
                .get(name)
                .ok_or_else(|| SemanticError::UndefinedFunction {
                    name: name.clone(),
                    span: expr.span,
                })?;
            let expected = callee.params.len();
            if args.len() != expected {
                return Err(SemanticError::ArgumentCountMismatch {
                    name: name.clone(),
                    expected,
                    got: args.len(),
                    span: expr.span,
                });
            }
            for (arg, param) in args.iter().zip(callee.params.iter()) {
                let at = infer_expr(arg, env)?;
                assert_assignable(&at, &param.ty, arg.span)?;
            }
            Ok(callee.return_type.ty.clone())
        }
        ExprKind::IfElse {
            condition,
            then_branch,
            else_branch,
        } => {
            require_bool_expr(condition, env)?;
            let tt = infer_expr_with_expect(then_branch, env, expect)?;
            let et = infer_expr_with_expect(else_branch, env, expect)?;
            if types_equal(&tt, &et) {
                Ok(tt)
            } else if matches!(tt, Type::Int(64)) && is_integer_type(&et) {
                Ok(et)
            } else if matches!(et, Type::Int(64)) && is_integer_type(&tt) {
                Ok(tt)
            } else if matches!(tt, Type::Float64) && is_float_type(&et) {
                Ok(et)
            } else if matches!(et, Type::Float64) && is_float_type(&tt) {
                Ok(tt)
            } else {
                Err(SemanticError::TypeMismatch {
                    expected: tt.to_string(),
                    found: et.to_string(),
                    span: else_branch.span,
                })
            }
        }
        ExprKind::Loop { body } => {
            env.loop_stack.push(LoopFrame {
                contextual: expect.cloned(),
                inferred: None,
            });
            for s in body {
                check_stmt(s, env)?;
            }
            let frame = env
                .loop_stack
                .pop()
                .ok_or_else(|| SemanticError::InvalidOperands {
                    op: "loop".to_owned(),
                    detail: "internal: missing loop frame".to_owned(),
                    span: expr.span,
                })?;
            Ok(frame.finish())
        }
        ExprKind::CondLoop {
            condition, body, ..
        } => {
            require_bool_expr(condition, env)?;
            env.loop_stack.push(LoopFrame {
                contextual: expect.cloned(),
                inferred: None,
            });
            for s in body {
                check_stmt(s, env)?;
            }
            let frame = env
                .loop_stack
                .pop()
                .ok_or_else(|| SemanticError::InvalidOperands {
                    op: "loop".to_owned(),
                    detail: "internal: missing loop frame".to_owned(),
                    span: expr.span,
                })?;
            Ok(frame.finish())
        }
    }
}

fn infer_binop(
    op: &BinOp,
    lhs_ty: Type,
    rhs_ty: Type,
    span: crate::frontend::tokens::Span,
) -> SemanticResult<Type> {
    match op {
        BinOp::Pow => {
            if is_float_type(&lhs_ty)
                || is_float_type(&rhs_ty)
                || matches!(lhs_ty, Type::Float64)
                || matches!(rhs_ty, Type::Float64)
            {
                return Err(SemanticError::InvalidOperands {
                    op: "**".to_owned(),
                    detail: "integer-only in this version".to_owned(),
                    span,
                });
            }
            unify_arithmetic(lhs_ty, rhs_ty, span)
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            unify_arithmetic(lhs_ty, rhs_ty, span)
        }
        BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
            unify_compare(lhs_ty.clone(), rhs_ty.clone(), span)?;
            Ok(Type::Bool)
        }
        BinOp::BitwiseAnd | BinOp::BitwiseOr | BinOp::BitwiseXor => {
            if !(is_integer_type(&lhs_ty) || matches!(lhs_ty, Type::Int(64)))
                || !(is_integer_type(&rhs_ty) || matches!(rhs_ty, Type::Int(64)))
            {
                return Err(SemanticError::InvalidOperands {
                    op: format!("{op:?}"),
                    detail: "integer operands required".to_owned(),
                    span,
                });
            }
            unify_arithmetic(lhs_ty, rhs_ty, span)
        }
        BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::LogicalXor => {
            if !is_bool_type(&lhs_ty) || !is_bool_type(&rhs_ty) {
                return Err(SemanticError::InvalidOperands {
                    op: format!("{:?}", op),
                    detail: format!("expected `bool`, got `{lhs_ty}` and `{rhs_ty}`"),
                    span,
                });
            }
            Ok(Type::Bool)
        }
        BinOp::LShift | BinOp::RShift => unify_shift(lhs_ty, rhs_ty, span),
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

/// Infers the type of an expression after validation has succeeded (for codegen).
///
/// `visible_vars` must list every in-scope local (and may repeat params / named-return
/// bindings); entries are merged after the function header so codegen can supply LLVM
/// stack locals that validation knew about but this helper cannot see otherwise.
pub fn infer_expr_type_after_validate(
    expr: &Expr,
    program: &Program,
    current_fn: &Function,
    visible_vars: &HashMap<String, Type>,
) -> SemanticResult<Type> {
    let fn_map: HashMap<String, &Function> = program
        .functions
        .iter()
        .map(|f| (f.name.clone(), f))
        .collect();

    let mut vars = HashMap::new();
    for p in &current_fn.params {
        let name = p.name.as_deref().unwrap_or("");
        vars.insert(name.to_string(), p.ty.clone());
    }
    if let Some(n) = &current_fn.return_type.name {
        vars.insert(n.clone(), current_fn.return_type.ty.clone());
    }
    for (name, ty) in visible_vars {
        vars.insert(name.clone(), ty.clone());
    }

    let mut env = Env {
        vars,
        return_ty: current_fn.return_type.ty.clone(),
        fn_map: &fn_map,
        loop_stack: Vec::new(),
    };

    infer_expr(expr, &mut env)
}
