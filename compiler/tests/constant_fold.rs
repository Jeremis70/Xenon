use xenonc::frontend::ast::{BinOp, Binding, Expr, Function, Program, Stmt, Type, UnaryOp};
use xenonc::middle::constant_fold::fold_constants;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_program(expr: Expr) -> Program {
    Program {
        functions: vec![Function {
            name: "test".to_string(),
            params: vec![],
            return_type: Binding {
                name: None,
                ty: Type::Int(64),
                default: None,
            },
            body: vec![Stmt::Expr(Box::new(expr))],
        }],
    }
}

/// Fold a single expression and return the result.
fn fold(expr: Expr) -> Expr {
    match fold_constants(make_program(expr))
        .functions
        .into_iter()
        .next()
        .unwrap()
        .body
        .into_iter()
        .next()
        .unwrap()
    {
        Stmt::Expr(inner) => *inner,
        other => panic!("expected Stmt::Expr, got {:?}", other),
    }
}

fn binop(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
    Expr::BinOp {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    }
}

fn unary(op: UnaryOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
    }
}

fn int(n: i64) -> Expr {
    Expr::Int(n)
}

fn ident(s: &str) -> Expr {
    Expr::Ident(s.to_string())
}

// ── Arithmetic ────────────────────────────────────────────────────────────────

#[test]
fn folds_addition() {
    assert_eq!(fold(binop(int(2), BinOp::Add, int(3))), int(5));
}

#[test]
fn folds_subtraction() {
    assert_eq!(fold(binop(int(10), BinOp::Sub, int(3))), int(7));
}

#[test]
fn folds_multiplication() {
    assert_eq!(fold(binop(int(4), BinOp::Mul, int(5))), int(20));
}

#[test]
fn folds_division() {
    assert_eq!(fold(binop(int(10), BinOp::Div, int(2))), int(5));
}

#[test]
fn folds_modulo() {
    assert_eq!(fold(binop(int(10), BinOp::Mod, int(3))), int(1));
}

#[test]
fn folds_pow() {
    assert_eq!(fold(binop(int(2), BinOp::Pow, int(10))), int(1024));
}

// ── Division / mod by zero stays unfolded ────────────────────────────────────

#[test]
fn division_by_zero_not_folded() {
    let expr = binop(int(5), BinOp::Div, int(0));
    assert_eq!(fold(expr.clone()), expr);
}

#[test]
fn modulo_by_zero_not_folded() {
    let expr = binop(int(5), BinOp::Mod, int(0));
    assert_eq!(fold(expr.clone()), expr);
}

// ── Pow edge cases ────────────────────────────────────────────────────────────

#[test]
fn pow_negative_exponent_not_folded() {
    let expr = binop(int(2), BinOp::Pow, int(-1));
    assert_eq!(fold(expr.clone()), expr);
}

// ── Shifts ────────────────────────────────────────────────────────────────────

#[test]
fn folds_left_shift() {
    assert_eq!(fold(binop(int(1), BinOp::LShift, int(3))), int(8));
}

#[test]
fn folds_right_shift() {
    assert_eq!(fold(binop(int(16), BinOp::RShift, int(2))), int(4));
}

#[test]
fn left_shift_out_of_range_not_folded() {
    let expr = binop(int(1), BinOp::LShift, int(64));
    assert_eq!(fold(expr.clone()), expr);
}

#[test]
fn right_shift_out_of_range_not_folded() {
    let expr = binop(int(1), BinOp::RShift, int(64));
    assert_eq!(fold(expr.clone()), expr);
}

#[test]
fn left_shift_negative_amount_not_folded() {
    let expr = binop(int(1), BinOp::LShift, int(-1));
    assert_eq!(fold(expr.clone()), expr);
}

// ── Bitwise ───────────────────────────────────────────────────────────────────

#[test]
fn folds_bitwise_and() {
    assert_eq!(
        fold(binop(int(0b1010), BinOp::BitwiseAnd, int(0b1100))),
        int(0b1000)
    );
}

#[test]
fn folds_bitwise_or() {
    assert_eq!(
        fold(binop(int(0b1010), BinOp::BitwiseOr, int(0b1100))),
        int(0b1110)
    );
}

#[test]
fn folds_bitwise_xor() {
    assert_eq!(
        fold(binop(int(0b1010), BinOp::BitwiseXor, int(0b1100))),
        int(0b0110)
    );
}

// ── Unary ─────────────────────────────────────────────────────────────────────

#[test]
fn folds_unary_negation() {
    assert_eq!(fold(unary(UnaryOp::Neg, int(5))), int(-5));
}

#[test]
fn folds_unary_bitwise_not() {
    assert_eq!(fold(unary(UnaryOp::BitwiseNot, int(0))), int(!0_i64));
}

#[test]
fn unary_neg_min_i64_wraps() {
    // i64::MIN.wrapping_neg() == i64::MIN (no panic)
    assert_eq!(fold(unary(UnaryOp::Neg, int(i64::MIN))), int(i64::MIN));
}

// ── Nested / bottom-up folding ────────────────────────────────────────────────

#[test]
fn folds_nested_binop() {
    // (2 + 3) * 4  →  20
    let inner = binop(int(2), BinOp::Add, int(3));
    let expr = binop(inner, BinOp::Mul, int(4));
    assert_eq!(fold(expr), int(20));
}

#[test]
fn folds_deep_nesting() {
    // ((1 + 2) + (3 + 4))  →  10
    let lhs = binop(int(1), BinOp::Add, int(2));
    let rhs = binop(int(3), BinOp::Add, int(4));
    assert_eq!(fold(binop(lhs, BinOp::Add, rhs)), int(10));
}

// ── Return wrapping ───────────────────────────────────────────────────────────

#[test]
fn folds_through_return() {
    let program = Program {
        functions: vec![Function {
            name: "test".to_string(),
            params: vec![],
            return_type: Binding {
                name: None,
                ty: Type::Int(64),
                default: None,
            },
            body: vec![Stmt::Return(Box::new(binop(int(2), BinOp::Add, int(3))))],
        }],
    };
    let result = fold_constants(program);
    assert_eq!(result.functions[0].body[0], Stmt::Return(Box::new(int(5))));
}

// ── Identifiers block folding ────────────────────────────────────────────────

#[test]
fn ident_prevents_binop_fold() {
    let expr = binop(ident("x"), BinOp::Add, int(1));
    assert_eq!(fold(expr.clone()), expr);
}

#[test]
fn partial_constant_ident_not_folded() {
    // (2 + 3) + x  →  5 + x  (left side is folded, but the outer op is not)
    let expr = binop(binop(int(2), BinOp::Add, int(3)), BinOp::Add, ident("x"));
    let expected = binop(int(5), BinOp::Add, ident("x"));
    assert_eq!(fold(expr), expected);
}

// ── Comparison ops stay unfolded (bools not yet supported) ───────────────────

#[test]
fn comparison_ops_not_folded() {
    for op in [
        BinOp::Eq,
        BinOp::NotEq,
        BinOp::Lt,
        BinOp::Gt,
        BinOp::LtEq,
        BinOp::GtEq,
    ] {
        let expr = binop(int(1), op, int(2));
        assert_eq!(fold(expr.clone()), expr);
    }
}

#[test]
fn logical_ops_not_folded() {
    for op in [BinOp::LogicalAnd, BinOp::LogicalOr, BinOp::LogicalXor] {
        let expr = binop(int(1), op, int(2));
        assert_eq!(fold(expr.clone()), expr);
    }
}

// ── If statement folding ──────────────────────────────────────────────────────

/// Helper: build a Program that contains a single Stmt::If and run
/// constant folding over it, returning the resulting Stmt.
fn fold_if(condition: Expr, then_stmts: Vec<Stmt>, else_stmts: Option<Vec<Stmt>>) -> Stmt {
    let program = Program {
        functions: vec![Function {
            name: "test".to_string(),
            params: vec![],
            return_type: Binding {
                name: None,
                ty: Type::Int(64),
                default: None,
            },
            body: vec![Stmt::If {
                condition: Box::new(condition),
                then_branch: then_stmts,
                else_branch: else_stmts,
            }],
        }],
    };
    fold_constants(program)
        .functions
        .into_iter()
        .next()
        .unwrap()
        .body
        .into_iter()
        .next()
        .unwrap()
}

#[test]
fn fold_if_folds_constant_condition() {
    // The condition itself is a constant expression and should be folded.
    let condition = binop(int(1), BinOp::Add, int(1)); // 1 + 1 → 2
    let result = fold_if(condition, vec![Stmt::Return(Box::new(int(0)))], None);
    match result {
        Stmt::If { condition, .. } => {
            assert_eq!(*condition, int(2), "condition should fold to 2");
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn fold_if_folds_constant_exprs_inside_then_branch() {
    let result = fold_if(
        ident("x"),
        vec![Stmt::Return(Box::new(binop(int(2), BinOp::Add, int(3))))],
        None,
    );
    match result {
        Stmt::If { then_branch, .. } => match &then_branch[0] {
            Stmt::Return(expr) => assert_eq!(expr.as_ref(), &int(5)),
            other => panic!("expected Return(5), got {:?}", other),
        },
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn fold_if_folds_constant_exprs_inside_else_branch() {
    let result = fold_if(
        ident("x"),
        vec![Stmt::Return(Box::new(int(0)))],
        Some(vec![Stmt::Return(Box::new(binop(
            int(3),
            BinOp::Mul,
            int(4),
        )))]),
    );
    match result {
        Stmt::If {
            else_branch: Some(else_stmts),
            ..
        } => match &else_stmts[0] {
            Stmt::Return(expr) => assert_eq!(expr.as_ref(), &int(12)),
            other => panic!("expected Return(12), got {:?}", other),
        },
        other => panic!("expected Stmt::If with else branch, got {:?}", other),
    }
}

#[test]
fn fold_if_without_else_branch_preserves_none() {
    let result = fold_if(ident("x"), vec![Stmt::Return(Box::new(int(1)))], None);
    match result {
        Stmt::If { else_branch, .. } => {
            assert!(else_branch.is_none(), "else branch should remain None");
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}
