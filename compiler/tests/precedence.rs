use num_bigint::BigInt;
use xenonc::frontend::ast::{BinOp, Expr, ExprKind, StmtKind, UnaryOp};
use xenonc::frontend::tokens::Span;

fn ternary(then_branch: Expr, condition: Expr, else_branch: Expr) -> Expr {
    Expr {
        kind: ExprKind::IfElse {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        span: Span::ZERO,
    }
}
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;

// Helper functions

fn parse_expr(expr_src: &str) -> Expr {
    let src = format!("fn x()->u32{{return {};}}", expr_src);
    let tokens = lex(&src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    match program.functions[0].body[0].kind.clone() {
        StmtKind::Return(expr) => *expr,
        other => panic!("expected return statement, got {:?}", other),
    }
}

fn int(n: i64) -> Expr {
    Expr {
        kind: ExprKind::Int(BigInt::from(n)),
        span: Span::ZERO,
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

// Tests

#[test]
fn mul_binds_tighter_than_add() {
    assert_eq!(
        parse_expr("1 + 2 * 3"),
        binop(int(1), BinOp::Add, binop(int(2), BinOp::Mul, int(3)))
    );
}

#[test]
fn div_binds_tighter_than_sub() {
    assert_eq!(
        parse_expr("6 - 4 / 2"),
        binop(int(6), BinOp::Sub, binop(int(4), BinOp::Div, int(2)))
    );
}

#[test]
fn mod_binds_tighter_than_sub() {
    assert_eq!(
        parse_expr("7 - 3 % 2"),
        binop(int(7), BinOp::Sub, binop(int(3), BinOp::Mod, int(2)))
    );
}

#[test]
fn pow_binds_tighter_than_mul() {
    assert_eq!(
        parse_expr("2 ** 3 * 4"),
        binop(binop(int(2), BinOp::Pow, int(3)), BinOp::Mul, int(4))
    );
}

#[test]
fn add_binds_tighter_than_lshift() {
    assert_eq!(
        parse_expr("1 + 2 << 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::LShift, int(3))
    );
}

#[test]
fn add_binds_tighter_than_rshift() {
    assert_eq!(
        parse_expr("8 >> 1 + 1"),
        binop(int(8), BinOp::RShift, binop(int(1), BinOp::Add, int(1)))
    );
}

#[test]
fn mul_binds_tighter_than_rshift() {
    assert_eq!(
        parse_expr("2 * 3 >> 1"),
        binop(binop(int(2), BinOp::Mul, int(3)), BinOp::RShift, int(1))
    );
}

#[test]
fn shift_binds_tighter_than_bitwise_and() {
    assert_eq!(
        parse_expr("1 << 2 & 3"),
        binop(
            binop(int(1), BinOp::LShift, int(2)),
            BinOp::BitwiseAnd,
            int(3)
        )
    );
}

#[test]
fn bitwise_and_binds_tighter_than_or() {
    assert_eq!(
        parse_expr("1 & 2 | 3"),
        binop(
            binop(int(1), BinOp::BitwiseAnd, int(2)),
            BinOp::BitwiseOr,
            int(3)
        )
    );
}

#[test]
fn bitwise_and_binds_tighter_than_or_rhs() {
    assert_eq!(
        parse_expr("1 | 2 & 3"),
        binop(
            int(1),
            BinOp::BitwiseOr,
            binop(int(2), BinOp::BitwiseAnd, int(3))
        )
    );
}

#[test]
fn bitwise_and_binds_tighter_than_xor() {
    assert_eq!(
        parse_expr("1 ^ 2 & 3"),
        binop(
            int(1),
            BinOp::BitwiseXor,
            binop(int(2), BinOp::BitwiseAnd, int(3))
        )
    );
}

#[test]
fn bitwise_xor_binds_tighter_than_or() {
    assert_eq!(
        parse_expr("1 | 2 ^ 3"),
        binop(
            int(1),
            BinOp::BitwiseOr,
            binop(int(2), BinOp::BitwiseXor, int(3))
        )
    );
}

#[test]
fn full_bitwise_chain() {
    assert_eq!(
        parse_expr("1 & 2 ^ 3 | 4"),
        binop(
            binop(
                binop(int(1), BinOp::BitwiseAnd, int(2)),
                BinOp::BitwiseXor,
                int(3)
            ),
            BinOp::BitwiseOr,
            int(4)
        )
    );
}

#[test]
fn bitwise_and_binds_tighter_than_equality() {
    assert_eq!(
        parse_expr("1 & 2 == 3"),
        binop(binop(int(1), BinOp::BitwiseAnd, int(2)), BinOp::Eq, int(3))
    );
}

#[test]
fn bitwise_and_binds_tighter_than_equality_rhs() {
    assert_eq!(
        parse_expr("1 == 2 & 3"),
        binop(int(1), BinOp::Eq, binop(int(2), BinOp::BitwiseAnd, int(3)))
    );
}

#[test]
fn bitwise_or_binds_tighter_than_equality() {
    assert_eq!(
        parse_expr("1 | 2 == 3"),
        binop(binop(int(1), BinOp::BitwiseOr, int(2)), BinOp::Eq, int(3))
    );
}

#[test]
fn relational_binds_tighter_than_eq() {
    assert_eq!(
        parse_expr("1 < 2 == 3"),
        binop(binop(int(1), BinOp::Lt, int(2)), BinOp::Eq, int(3))
    );
}

#[test]
fn relational_binds_tighter_than_eq_rhs() {
    assert_eq!(
        parse_expr("1 == 2 < 3"),
        binop(int(1), BinOp::Eq, binop(int(2), BinOp::Lt, int(3)))
    );
}

#[test]
fn relational_binds_tighter_than_neq() {
    assert_eq!(
        parse_expr("1 != 2 >= 3"),
        binop(int(1), BinOp::NotEq, binop(int(2), BinOp::GtEq, int(3)))
    );
}

#[test]
fn shift_binds_tighter_than_lt() {
    assert_eq!(
        parse_expr("1 < 2 << 3"),
        binop(int(1), BinOp::Lt, binop(int(2), BinOp::LShift, int(3)))
    );
}

#[test]
fn equality_binds_tighter_than_logical_and() {
    assert_eq!(
        parse_expr("1 == 2 && 3 == 4"),
        binop(
            binop(int(1), BinOp::Eq, int(2)),
            BinOp::LogicalAnd,
            binop(int(3), BinOp::Eq, int(4))
        )
    );
}

#[test]
fn logical_and_binds_tighter_than_or_rhs() {
    assert_eq!(
        parse_expr("1 || 2 && 3"),
        binop(
            int(1),
            BinOp::LogicalOr,
            binop(int(2), BinOp::LogicalAnd, int(3))
        )
    );
}

#[test]
fn logical_and_binds_tighter_than_or_lhs() {
    assert_eq!(
        parse_expr("1 && 2 || 3"),
        binop(
            binop(int(1), BinOp::LogicalAnd, int(2)),
            BinOp::LogicalOr,
            int(3)
        )
    );
}

#[test]
fn logical_and_binds_tighter_than_xor() {
    assert_eq!(
        parse_expr("1 ^^ 2 && 3"),
        binop(
            int(1),
            BinOp::LogicalXor,
            binop(int(2), BinOp::LogicalAnd, int(3))
        )
    );
}

#[test]
fn logical_xor_binds_tighter_than_or() {
    assert_eq!(
        parse_expr("1 || 2 ^^ 3"),
        binop(
            int(1),
            BinOp::LogicalOr,
            binop(int(2), BinOp::LogicalXor, int(3))
        )
    );
}

#[test]
fn bitwise_or_binds_tighter_than_logical_or() {
    assert_eq!(
        parse_expr("1 | 2 || 3"),
        binop(
            binop(int(1), BinOp::BitwiseOr, int(2)),
            BinOp::LogicalOr,
            int(3)
        )
    );
}

#[test]
fn bitwise_and_binds_tighter_than_logical_and() {
    assert_eq!(
        parse_expr("1 & 2 && 3"),
        binop(
            binop(int(1), BinOp::BitwiseAnd, int(2)),
            BinOp::LogicalAnd,
            int(3)
        )
    );
}

#[test]
fn add_is_left_associative() {
    assert_eq!(
        parse_expr("1 + 2 + 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::Add, int(3))
    );
}

#[test]
fn sub_is_left_associative() {
    assert_eq!(
        parse_expr("8 - 3 - 2"),
        binop(binop(int(8), BinOp::Sub, int(3)), BinOp::Sub, int(2))
    );
}

#[test]
fn mul_is_left_associative() {
    assert_eq!(
        parse_expr("2 * 3 * 4"),
        binop(binop(int(2), BinOp::Mul, int(3)), BinOp::Mul, int(4))
    );
}

#[test]
fn div_is_left_associative() {
    assert_eq!(
        parse_expr("12 / 3 / 2"),
        binop(binop(int(12), BinOp::Div, int(3)), BinOp::Div, int(2))
    );
}

#[test]
fn lshift_is_left_associative() {
    assert_eq!(
        parse_expr("1 << 2 << 3"),
        binop(binop(int(1), BinOp::LShift, int(2)), BinOp::LShift, int(3))
    );
}

#[test]
fn bitwise_and_is_left_associative() {
    assert_eq!(
        parse_expr("1 & 2 & 3"),
        binop(
            binop(int(1), BinOp::BitwiseAnd, int(2)),
            BinOp::BitwiseAnd,
            int(3)
        )
    );
}

#[test]
fn bitwise_or_is_left_associative() {
    assert_eq!(
        parse_expr("1 | 2 | 3"),
        binop(
            binop(int(1), BinOp::BitwiseOr, int(2)),
            BinOp::BitwiseOr,
            int(3)
        )
    );
}

#[test]
fn logical_and_is_left_associative() {
    assert_eq!(
        parse_expr("1 && 2 && 3"),
        binop(
            binop(int(1), BinOp::LogicalAnd, int(2)),
            BinOp::LogicalAnd,
            int(3)
        )
    );
}

#[test]
fn logical_or_is_left_associative() {
    assert_eq!(
        parse_expr("1 || 2 || 3"),
        binop(
            binop(int(1), BinOp::LogicalOr, int(2)),
            BinOp::LogicalOr,
            int(3)
        )
    );
}

#[test]
fn pow_is_right_associative() {
    assert_eq!(
        parse_expr("2 ** 3 ** 4"),
        binop(int(2), BinOp::Pow, binop(int(3), BinOp::Pow, int(4)))
    );
}

#[test]
fn parens_override_mul_over_add() {
    assert_eq!(
        parse_expr("(1 + 2) * 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::Mul, int(3))
    );
}

#[test]
fn parens_on_rhs_override_precedence() {
    assert_eq!(
        parse_expr("2 * (3 + 4)"),
        binop(int(2), BinOp::Mul, binop(int(3), BinOp::Add, int(4)))
    );
}

#[test]
fn parens_override_pow_right_associativity() {
    assert_eq!(
        parse_expr("(2 ** 3) ** 4"),
        binop(binop(int(2), BinOp::Pow, int(3)), BinOp::Pow, int(4))
    );
}

#[test]
fn parens_on_both_sides_of_eq() {
    assert_eq!(
        parse_expr("(1 + 2) == (3 + 4)"),
        binop(
            binop(int(1), BinOp::Add, int(2)),
            BinOp::Eq,
            binop(int(3), BinOp::Add, int(4))
        )
    );
}

#[test]
fn unary_neg_binds_tighter_than_add() {
    assert_eq!(
        parse_expr("-1 + 2"),
        binop(unary(UnaryOp::Neg, int(1)), BinOp::Add, int(2))
    );
}

#[test]
fn unary_not_binds_tighter_than_add() {
    assert_eq!(
        parse_expr("!1 + 2"),
        binop(unary(UnaryOp::Not, int(1)), BinOp::Add, int(2))
    );
}

#[test]
fn unary_bitwise_not_binds_tighter_than_add() {
    assert_eq!(
        parse_expr("~1 + 2"),
        binop(unary(UnaryOp::BitwiseNot, int(1)), BinOp::Add, int(2))
    );
}

#[test]
fn ternary_is_lowest_precedence() {
    assert_eq!(
        parse_expr("1 + 2 if 3 else 4"),
        ternary(binop(int(1), BinOp::Add, int(2)), int(3), int(4),)
    );
}

#[test]
fn ternary_rhs_is_full_expression() {
    assert_eq!(
        parse_expr("1 if 2 else 3 + 4"),
        ternary(int(1), int(2), binop(int(3), BinOp::Add, int(4)))
    );
}
