use crate::error::{FoldError, FoldResult};
use crate::frontend::ast::{
    BinOp, Binding, Expr, ExprKind, Function, Program, Stmt, StmtKind, UnaryOp,
};
use crate::frontend::tokens::Span;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

pub fn fold_constants(program: Program) -> FoldResult<Program> {
    Ok(Program {
        functions: program
            .functions
            .into_iter()
            .map(fold_function)
            .collect::<FoldResult<_>>()?,
    })
}

fn fold_function(func: Function) -> FoldResult<Function> {
    Ok(Function {
        name: func.name,
        params: func.params,
        return_type: func.return_type,
        body: func
            .body
            .into_iter()
            .map(fold_stmt)
            .collect::<FoldResult<_>>()?,
        attributes: func.attributes,
        span: func.span,
    })
}

fn fold_stmt(stmt: Stmt) -> FoldResult<Stmt> {
    let span = stmt.span;
    let kind = match stmt.kind {
        StmtKind::Return(inner) => StmtKind::Return(Box::new(fold_expr(*inner)?)),
        StmtKind::Expr(inner) => StmtKind::Expr(Box::new(fold_expr(*inner)?)),
        StmtKind::VarDecl(binding) => StmtKind::VarDecl(Binding {
            default: binding
                .default
                .map(|v| fold_expr(*v).map(Box::new))
                .transpose()?,
            ..binding
        }),
        StmtKind::Assign { name, value } => StmtKind::Assign {
            name,
            value: Box::new(fold_expr(*value)?),
        },
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => StmtKind::If {
            condition: Box::new(fold_expr(*condition)?),
            then_branch: then_branch
                .into_iter()
                .map(fold_stmt)
                .collect::<FoldResult<_>>()?,
            else_branch: else_branch
                .map(|branch| branch.into_iter().map(fold_stmt).collect::<FoldResult<_>>())
                .transpose()?,
        },
        StmtKind::Break(opt) => {
            StmtKind::Break(opt.map(|e| fold_expr(*e).map(Box::new)).transpose()?)
        }
        StmtKind::Continue => StmtKind::Continue,
    };
    Ok(Stmt { kind, span })
}

fn fold_expr(expr: Expr) -> FoldResult<Expr> {
    let span = expr.span;
    let kind = match expr.kind {
        ExprKind::BinOp { lhs, op, rhs } => {
            let lhs = fold_expr(*lhs)?;
            let rhs = fold_expr(*rhs)?;

            if let (ExprKind::Int(a), ExprKind::Int(b)) = (&lhs.kind, &rhs.kind)
                && let Some(result) = eval_binop(&op, a, b, span)?
            {
                return Ok(Expr {
                    kind: ExprKind::Int(result),
                    span,
                });
            }

            ExprKind::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            }
        }

        ExprKind::UnaryOp { op, operand } => {
            let operand = fold_expr(*operand)?;

            match (&op, &operand.kind) {
                (UnaryOp::Neg, ExprKind::Int(n)) => {
                    return Ok(Expr {
                        kind: ExprKind::Int(-n),
                        span,
                    });
                }
                (UnaryOp::Neg, ExprKind::Float(f)) => {
                    return Ok(Expr {
                        kind: ExprKind::Float(-f),
                        span,
                    });
                }
                (UnaryOp::BitwiseNot, ExprKind::Int(n)) => {
                    return Ok(Expr {
                        kind: ExprKind::Int(!n),
                        span,
                    });
                }
                _ => {}
            }

            ExprKind::UnaryOp {
                op,
                operand: Box::new(operand),
            }
        }

        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Float(_) | ExprKind::Ident(_) => expr.kind,

        ExprKind::IfElse {
            condition,
            then_branch,
            else_branch,
        } => ExprKind::IfElse {
            condition: Box::new(fold_expr(*condition)?),
            then_branch: Box::new(fold_expr(*then_branch)?),
            else_branch: Box::new(fold_expr(*else_branch)?),
        },

        ExprKind::Loop { body } => ExprKind::Loop {
            body: body.into_iter().map(fold_stmt).collect::<FoldResult<_>>()?,
        },

        ExprKind::CondLoop {
            post,
            inverted,
            condition,
            body,
        } => ExprKind::CondLoop {
            post,
            inverted,
            condition: Box::new(fold_expr(*condition)?),
            body: body.into_iter().map(fold_stmt).collect::<FoldResult<_>>()?,
        },

        ExprKind::Call { name, args } => ExprKind::Call {
            name,
            args: args.into_iter().map(fold_expr).collect::<FoldResult<_>>()?,
        },
    };
    Ok(Expr { kind, span })
}

fn eval_binop(op: &BinOp, a: &BigInt, b: &BigInt, span: Span) -> FoldResult<Option<BigInt>> {
    match op {
        BinOp::Add => Ok(Some(a + b)),
        BinOp::Sub => Ok(Some(a - b)),
        BinOp::Mul => Ok(Some(a * b)),
        BinOp::Div => {
            if b.is_zero() {
                Err(FoldError::DivisionByZero { span })
            } else {
                Ok(Some(a / b))
            }
        }
        BinOp::Mod => {
            if b.is_zero() {
                Err(FoldError::DivisionByZero { span })
            } else {
                Ok(Some(a % b))
            }
        }
        BinOp::Pow => Ok(b.to_u32().map(|e| a.pow(e))),
        BinOp::LShift => Ok(b.to_u64().map(|s| a << s)),
        BinOp::RShift => Ok(b.to_u64().map(|s| a >> s)),
        BinOp::BitwiseAnd => Ok(Some(a & b)),
        BinOp::BitwiseOr => Ok(Some(a | b)),
        BinOp::BitwiseXor => Ok(Some(a ^ b)),
        BinOp::Eq
        | BinOp::NotEq
        | BinOp::Lt
        | BinOp::Gt
        | BinOp::LtEq
        | BinOp::GtEq
        | BinOp::LogicalAnd
        | BinOp::LogicalOr
        | BinOp::LogicalXor => Ok(None),
    }
}
