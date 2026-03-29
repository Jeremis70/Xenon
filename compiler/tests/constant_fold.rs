use num_bigint::BigInt;
use xenonc::frontend::ast::{
    BinOp, Binding, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp,
};
use xenonc::frontend::tokens::Span;
use xenonc::middle::constant_fold::fold_constants;

fn make_program(expr: Expr) -> Program {
    Program {
        functions: vec![Function {
            name: "test".to_string(),
            params: vec![],
            return_type: Binding {
                name: None,
                ty: Type::Int(64),
                default: None,
                span: Span::ZERO,
            },
            body: vec![Stmt {
                kind: StmtKind::Expr(Box::new(expr)),
                span: Span::ZERO,
            }],
            span: Span::ZERO,
        }],
    }
}

fn fold(expr: Expr) -> Expr {
    match fold_constants(make_program(expr))
        .expect("fold should succeed")
        .functions
        .into_iter()
        .next()
        .unwrap()
        .body
        .into_iter()
        .next()
        .unwrap()
        .kind
    {
        StmtKind::Expr(inner) => *inner,
        other => panic!("expected StmtKind::Expr, got {:?}", other),
    }
}

fn binop(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
    Expr {
        kind: ExprKind::BinOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        },
        span: Span::ZERO,
    }
}

fn unary(op: UnaryOp, operand: Expr) -> Expr {
    Expr {
        kind: ExprKind::UnaryOp {
            op,
            operand: Box::new(operand),
        },
        span: Span::ZERO,
    }
}

fn int(n: i64) -> Expr {
    Expr {
        kind: ExprKind::Int(BigInt::from(n)),
        span: Span::ZERO,
    }
}

fn ident(s: &str) -> Expr {
    Expr {
        kind: ExprKind::Ident(s.to_string()),
        span: Span::ZERO,
    }
}

// Tests

#[test]
fn fold_add() {
    assert_eq!(fold(binop(int(2), BinOp::Add, int(3))), int(5));
}

#[test]
fn fold_sub() {
    assert_eq!(fold(binop(int(10), BinOp::Sub, int(4))), int(6));
}

#[test]
fn fold_mul() {
    assert_eq!(fold(binop(int(3), BinOp::Mul, int(7))), int(21));
}

#[test]
fn fold_div() {
    assert_eq!(fold(binop(int(15), BinOp::Div, int(3))), int(5));
}

#[test]
fn fold_mod() {
    assert_eq!(fold(binop(int(17), BinOp::Mod, int(5))), int(2));
}

#[test]
fn fold_div_by_zero_is_error() {
    let program = make_program(binop(int(1), BinOp::Div, int(0)));
    let err = fold_constants(program).expect_err("division by zero should error");
    assert!(
        matches!(err, xenonc::error::FoldError::DivisionByZero { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn fold_mod_by_zero_is_error() {
    let program = make_program(binop(int(1), BinOp::Mod, int(0)));
    let err = fold_constants(program).expect_err("modulo by zero should error");
    assert!(
        matches!(err, xenonc::error::FoldError::DivisionByZero { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn fold_pow() {
    assert_eq!(fold(binop(int(2), BinOp::Pow, int(10))), int(1024));
}

#[test]
fn fold_lshift() {
    assert_eq!(fold(binop(int(1), BinOp::LShift, int(4))), int(16));
}

#[test]
fn fold_rshift() {
    assert_eq!(fold(binop(int(64), BinOp::RShift, int(3))), int(8));
}

#[test]
fn fold_bitwise_and() {
    assert_eq!(
        fold(binop(int(0b1100), BinOp::BitwiseAnd, int(0b1010))),
        int(0b1000)
    );
}

#[test]
fn fold_bitwise_or() {
    assert_eq!(
        fold(binop(int(0b1100), BinOp::BitwiseOr, int(0b1010))),
        int(0b1110)
    );
}

#[test]
fn fold_bitwise_xor() {
    assert_eq!(
        fold(binop(int(0b1100), BinOp::BitwiseXor, int(0b1010))),
        int(0b0110)
    );
}

#[test]
fn fold_nested_arithmetic() {
    // (2 + 3) * (10 - 4) = 5 * 6 = 30
    let inner_add = binop(int(2), BinOp::Add, int(3));
    let inner_sub = binop(int(10), BinOp::Sub, int(4));
    assert_eq!(fold(binop(inner_add, BinOp::Mul, inner_sub)), int(30));
}

#[test]
fn fold_neg() {
    assert_eq!(fold(unary(UnaryOp::Neg, int(42))), int(-42));
}

#[test]
fn fold_bitwise_not() {
    assert_eq!(fold(unary(UnaryOp::BitwiseNot, int(0))), int(-1));
}

#[test]
fn fold_preserves_ident() {
    let e = binop(ident("x"), BinOp::Add, int(1));
    let result = fold(e);
    assert!(
        matches!(&result.kind, ExprKind::BinOp { .. }),
        "expected unfoldable BinOp, got {:?}",
        result
    );
}

#[test]
fn fold_out_of_range_shift_is_not_folded() {
    let e = binop(int(1), BinOp::LShift, int(u64::MAX as i64));
    let result = fold(e);
    assert!(
        matches!(&result.kind, ExprKind::BinOp { .. }),
        "out-of-range shift should remain as BinOp, got {:?}",
        result
    );
}
