use crate::ast::{BinOp, Expr, Function, Program, Stmt, UnaryOp};

pub fn fold_constants(program: Program) -> Program {
    Program {
        functions: program.functions.into_iter().map(fold_function).collect(),
    }
}

fn fold_function(func: Function) -> Function {
    Function {
        name: func.name,
        params: func.params,
        return_type: func.return_type,
        body: func.body.into_iter().map(fold_stmt).collect(),
    }
}

fn fold_stmt(stmt: Stmt) -> Stmt {
    match stmt {
        Stmt::Return(inner) => Stmt::Return(Box::new(fold_expr(*inner))),
        Stmt::Expr(inner) => Stmt::Expr(Box::new(fold_expr(*inner))),
        Stmt::VarDecl { name, ty, value } => Stmt::VarDecl {
            name,
            ty,
            value: Box::new(fold_expr(*value)),
        },
        Stmt::Assign { name, value } => Stmt::Assign {
            name,
            value: Box::new(fold_expr(*value)),
        },
    }
}

fn fold_expr(expr: Expr) -> Expr {
    match expr {
        Expr::BinOp { lhs, op, rhs } => {
            // Bottom-up: fold children first
            let lhs = fold_expr(*lhs);
            let rhs = fold_expr(*rhs);

            if let (Expr::Int(a), Expr::Int(b)) = (&lhs, &rhs)
                && let Some(result) = eval_binop(&op, *a, *b)
            {
                return Expr::Int(result);
            }

            Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            }
        }

        Expr::UnaryOp { op, operand } => {
            let operand = fold_expr(*operand);

            match (&op, &operand) {
                (UnaryOp::Neg, Expr::Int(n)) => Expr::Int(n.wrapping_neg()),
                (UnaryOp::BitwiseNot, Expr::Int(n)) => Expr::Int(!n),
                // TODO : Once bools are supported handle logical not
                _ => Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                },
            }
        }

        Expr::Int(_) | Expr::Ident(_) => expr,
    }
}

fn eval_binop(op: &BinOp, a: i64, b: i64) -> Option<i64> {
    match op {
        BinOp::Add => Some(a + b),
        BinOp::Sub => Some(a - b),
        BinOp::Mul => Some(a * b),
        BinOp::Div => {
            if b != 0 {
                Some(a / b)
            } else {
                None
            }
        } // TODO : leave x/0 for runtime or an error pass
        BinOp::Mod => {
            if b != 0 {
                Some(a % b)
            } else {
                None
            }
        }
        BinOp::Pow => {
            if b >= 0 && b <= u32::MAX as i64 {
                Some(a.wrapping_pow(b as u32))
            } else {
                None // negative or out-of-range exponent — leave for runtime
            }
        }
        BinOp::LShift => {
            if b >= 0 && b < i64::BITS as i64 {
                Some(a << b)
            } else {
                None // shift amount out of range — leave for runtime/error pass
            }
        }
        BinOp::RShift => {
            if b >= 0 && b < i64::BITS as i64 {
                Some(a >> b)
            } else {
                None
            }
        }
        BinOp::BitwiseAnd => Some(a & b),
        BinOp::BitwiseOr => Some(a | b),
        BinOp::BitwiseXor => Some(a ^ b),
        // TODO: Once bools are supported handle logical ops
        BinOp::Eq
        | BinOp::NotEq
        | BinOp::Lt
        | BinOp::Gt
        | BinOp::LtEq
        | BinOp::GtEq
        | BinOp::LogicalAnd
        | BinOp::LogicalOr
        | BinOp::LogicalXor => None,
    }
}
