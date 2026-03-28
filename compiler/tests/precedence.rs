use num_bigint::BigInt;
use xenonc::frontend::ast::{BinOp, Expr, Stmt, UnaryOp};

fn ternary(then_branch: Expr, condition: Expr, else_branch: Expr) -> Expr {
    Expr::IfElse {
        condition: Box::new(condition),
        then_branch: Box::new(then_branch),
        else_branch: Box::new(else_branch),
    }
}
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_expr(expr_src: &str) -> Expr {
    let src = format!("fn x()->u32{{return {};}}", expr_src);
    let tokens = lex(&src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    match program.functions[0].body[0].clone() {
        Stmt::Return(expr) => *expr,
        other => panic!("expected return statement, got {:?}", other),
    }
}

fn int(n: i64) -> Expr {
    Expr::Int(BigInt::from(n))
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

// ── Tier 1: Arithmetic ────────────────────────────────────────────────────────
/// `1 + 2 * 3`  →  `1 + (2 * 3)`
#[test]
fn mul_binds_tighter_than_add() {
    assert_eq!(
        parse_expr("1 + 2 * 3"),
        binop(int(1), BinOp::Add, binop(int(2), BinOp::Mul, int(3)))
    );
}

/// `6 - 4 / 2`  →  `6 - (4 / 2)`
#[test]
fn div_binds_tighter_than_sub() {
    assert_eq!(
        parse_expr("6 - 4 / 2"),
        binop(int(6), BinOp::Sub, binop(int(4), BinOp::Div, int(2)))
    );
}

/// `7 - 3 % 2`  →  `7 - (3 % 2)`
#[test]
fn mod_binds_tighter_than_sub() {
    assert_eq!(
        parse_expr("7 - 3 % 2"),
        binop(int(7), BinOp::Sub, binop(int(3), BinOp::Mod, int(2)))
    );
}

/// `2 ** 3 * 4`  →  `(2 ** 3) * 4`  (pow > mul)
#[test]
fn pow_binds_tighter_than_mul() {
    assert_eq!(
        parse_expr("2 ** 3 * 4"),
        binop(binop(int(2), BinOp::Pow, int(3)), BinOp::Mul, int(4))
    );
}
// ── Tier 2: Shift ─────────────────────────────────────────────────────────────

/// `1 + 2 << 3`  →  `(1 + 2) << 3`  (add > shift)
#[test]
fn add_binds_tighter_than_lshift() {
    assert_eq!(
        parse_expr("1 + 2 << 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::LShift, int(3))
    );
}

/// `8 >> 1 + 1`  →  `8 >> (1 + 1)`
#[test]
fn add_binds_tighter_than_rshift() {
    assert_eq!(
        parse_expr("8 >> 1 + 1"),
        binop(int(8), BinOp::RShift, binop(int(1), BinOp::Add, int(1)))
    );
}

/// `2 * 3 >> 1`  →  `(2 * 3) >> 1`  (mul > shift)
#[test]
fn mul_binds_tighter_than_rshift() {
    assert_eq!(
        parse_expr("2 * 3 >> 1"),
        binop(binop(int(2), BinOp::Mul, int(3)), BinOp::RShift, int(1))
    );
}

// ── Tier 3: Bitwise ───────────────────────────────────────────────────────────

/// `1 << 2 & 3`  →  `(1 << 2) & 3`  (shift > bitwise-and)
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

/// `1 & 2 | 3`  →  `(1 & 2) | 3`  (bitwise-and > bitwise-or)
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

/// `1 | 2 & 3`  →  `1 | (2 & 3)`
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

/// `1 ^ 2 & 3`  →  `1 ^ (2 & 3)`  (bitwise-and > bitwise-xor)
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

/// `1 | 2 ^ 3`  →  `1 | (2 ^ 3)`  (bitwise-xor > bitwise-or)
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

/// `1 & 2 ^ 3 | 4`  →  `((1 & 2) ^ 3) | 4`  (full bitwise chain)
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

// ── Tier 4: Comparison ────────────────────────────────────────────────────────

/// `1 & 2 == 3`  →  `(1 & 2) == 3`  (bitwise-and > equality)
#[test]
fn bitwise_and_binds_tighter_than_equality() {
    assert_eq!(
        parse_expr("1 & 2 == 3"),
        binop(binop(int(1), BinOp::BitwiseAnd, int(2)), BinOp::Eq, int(3))
    );
}

/// `1 == 2 & 3`  →  `1 == (2 & 3)`
#[test]
fn bitwise_and_binds_tighter_than_equality_rhs() {
    assert_eq!(
        parse_expr("1 == 2 & 3"),
        binop(int(1), BinOp::Eq, binop(int(2), BinOp::BitwiseAnd, int(3)))
    );
}

/// `1 | 2 == 3`  →  `(1 | 2) == 3`  (bitwise-or > equality)
#[test]
fn bitwise_or_binds_tighter_than_equality() {
    assert_eq!(
        parse_expr("1 | 2 == 3"),
        binop(binop(int(1), BinOp::BitwiseOr, int(2)), BinOp::Eq, int(3))
    );
}

/// `1 < 2 == 3`  →  `(1 < 2) == 3`  (relational > equality)
#[test]
fn relational_binds_tighter_than_eq() {
    assert_eq!(
        parse_expr("1 < 2 == 3"),
        binop(binop(int(1), BinOp::Lt, int(2)), BinOp::Eq, int(3))
    );
}

/// `1 == 2 < 3`  →  `1 == (2 < 3)`
#[test]
fn relational_binds_tighter_than_eq_rhs() {
    assert_eq!(
        parse_expr("1 == 2 < 3"),
        binop(int(1), BinOp::Eq, binop(int(2), BinOp::Lt, int(3)))
    );
}

/// `1 != 2 >= 3`  →  `1 != (2 >= 3)`
#[test]
fn relational_binds_tighter_than_neq() {
    assert_eq!(
        parse_expr("1 != 2 >= 3"),
        binop(int(1), BinOp::NotEq, binop(int(2), BinOp::GtEq, int(3)))
    );
}

/// `1 < 2 << 3`  →  `1 < (2 << 3)`  (shift > relational)
#[test]
fn shift_binds_tighter_than_lt() {
    assert_eq!(
        parse_expr("1 < 2 << 3"),
        binop(int(1), BinOp::Lt, binop(int(2), BinOp::LShift, int(3)))
    );
}

// ── Tier 5: Logical ───────────────────────────────────────────────────────────

/// `1 == 2 && 3 == 4`  →  `(1 == 2) && (3 == 4)`  (equality > logical-and)
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

/// `1 || 2 && 3`  →  `1 || (2 && 3)`  (logical-and > logical-or)
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

/// `1 && 2 || 3`  →  `(1 && 2) || 3`
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

/// `1 ^^ 2 && 3`  →  `1 ^^ (2 && 3)`  (logical-and > logical-xor)
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

/// `1 || 2 ^^ 3`  →  `1 || (2 ^^ 3)`  (logical-xor > logical-or)
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

/// `1 | 2 || 3`  →  `(1 | 2) || 3`  (bitwise-or > logical-or)
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

/// `1 & 2 && 3`  →  `(1 & 2) && 3`  (bitwise-and > logical-and)
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

// ── Associativity ─────────────────────────────────────────────────────────────

/// `1 + 2 + 3`  →  `(1 + 2) + 3`
#[test]
fn add_is_left_associative() {
    assert_eq!(
        parse_expr("1 + 2 + 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::Add, int(3))
    );
}

/// `8 - 3 - 2`  →  `(8 - 3) - 2`
#[test]
fn sub_is_left_associative() {
    assert_eq!(
        parse_expr("8 - 3 - 2"),
        binop(binop(int(8), BinOp::Sub, int(3)), BinOp::Sub, int(2))
    );
}

/// `2 * 3 * 4`  →  `(2 * 3) * 4`
#[test]
fn mul_is_left_associative() {
    assert_eq!(
        parse_expr("2 * 3 * 4"),
        binop(binop(int(2), BinOp::Mul, int(3)), BinOp::Mul, int(4))
    );
}

/// `12 / 3 / 2`  →  `(12 / 3) / 2`
#[test]
fn div_is_left_associative() {
    assert_eq!(
        parse_expr("12 / 3 / 2"),
        binop(binop(int(12), BinOp::Div, int(3)), BinOp::Div, int(2))
    );
}

/// `1 << 2 << 3`  →  `(1 << 2) << 3`
#[test]
fn lshift_is_left_associative() {
    assert_eq!(
        parse_expr("1 << 2 << 3"),
        binop(binop(int(1), BinOp::LShift, int(2)), BinOp::LShift, int(3))
    );
}

/// `1 & 2 & 3`  →  `(1 & 2) & 3`
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

/// `1 | 2 | 3`  →  `(1 | 2) | 3`
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

/// `1 && 2 && 3`  →  `(1 && 2) && 3`
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

/// `1 || 2 || 3`  →  `(1 || 2) || 3`
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

/// `2 ** 3 ** 4`  →  `2 ** (3 ** 4)`  (right-associative)
#[test]
fn pow_is_right_associative() {
    assert_eq!(
        parse_expr("2 ** 3 ** 4"),
        binop(int(2), BinOp::Pow, binop(int(3), BinOp::Pow, int(4)))
    );
}

// ── Parentheses ───────────────────────────────────────────────────────────────

/// `(1 + 2) * 3`  →  `(1 + 2) * 3`
#[test]
fn parens_override_mul_over_add() {
    assert_eq!(
        parse_expr("(1 + 2) * 3"),
        binop(binop(int(1), BinOp::Add, int(2)), BinOp::Mul, int(3))
    );
}

/// `2 * (3 + 4)`  →  `2 * (3 + 4)`
#[test]
fn parens_on_rhs_override_precedence() {
    assert_eq!(
        parse_expr("2 * (3 + 4)"),
        binop(int(2), BinOp::Mul, binop(int(3), BinOp::Add, int(4)))
    );
}

/// `(2 ** 3) ** 4`  →  `(2 ** 3) ** 4`  (parens force left-assoc on pow)
#[test]
fn parens_override_pow_right_associativity() {
    assert_eq!(
        parse_expr("(2 ** 3) ** 4"),
        binop(binop(int(2), BinOp::Pow, int(3)), BinOp::Pow, int(4))
    );
}

/// `(1 + 2) == (3 + 4)`
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

/// `((1 + 2))`  →  `1 + 2`  (double parens)
#[test]
fn double_parens_are_transparent() {
    assert_eq!(parse_expr("((1 + 2))"), binop(int(1), BinOp::Add, int(2)));
}

/// `1 | (2 && 3)`  →  `1 | (2 && 3)`  (parens override logical/bitwise tier)
#[test]
fn parens_override_bitwise_logical_boundary() {
    assert_eq!(
        parse_expr("1 | (2 && 3)"),
        binop(
            int(1),
            BinOp::BitwiseOr,
            binop(int(2), BinOp::LogicalAnd, int(3))
        )
    );
}

// ── Unary operators ───────────────────────────────────────────────────────────

/// `-2 * 3`  →  `(-2) * 3`
#[test]
fn unary_neg_is_tighter_than_mul() {
    assert_eq!(
        parse_expr("-2 * 3"),
        binop(unary(UnaryOp::Neg, int(2)), BinOp::Mul, int(3))
    );
}

/// `!1 == 0`  →  `(!1) == 0`
#[test]
fn unary_not_is_tighter_than_eq() {
    assert_eq!(
        parse_expr("!1 == 0"),
        binop(unary(UnaryOp::Not, int(1)), BinOp::Eq, int(0))
    );
}

/// `~1 & 3`  →  `(~1) & 3`
#[test]
fn unary_bitwise_not_is_tighter_than_bitwise_and() {
    assert_eq!(
        parse_expr("~1 & 3"),
        binop(
            unary(UnaryOp::BitwiseNot, int(1)),
            BinOp::BitwiseAnd,
            int(3)
        )
    );
}

/// `- -3`  →  `-(-3)`  (double negation)
#[test]
fn double_unary_chains() {
    assert_eq!(
        parse_expr("- -3"),
        unary(UnaryOp::Neg, unary(UnaryOp::Neg, int(3)))
    );
}

/// `1 + -2`  →  `1 + (-2)`  (unary on rhs of binary)
#[test]
fn unary_neg_on_rhs_of_add() {
    assert_eq!(
        parse_expr("1 + -2"),
        binop(int(1), BinOp::Add, unary(UnaryOp::Neg, int(2)))
    );
}

/// `~1 & ~2`  →  `(~1) & (~2)`  (unary on both sides)
#[test]
fn unary_on_both_sides_of_bitwise_and() {
    assert_eq!(
        parse_expr("~1 & ~2"),
        binop(
            unary(UnaryOp::BitwiseNot, int(1)),
            BinOp::BitwiseAnd,
            unary(UnaryOp::BitwiseNot, int(2))
        )
    );
}

/// `-1 + -2`  →  `(-1) + (-2)`
#[test]
fn unary_neg_on_both_sides_of_add() {
    assert_eq!(
        parse_expr("-1 + -2"),
        binop(
            unary(UnaryOp::Neg, int(1)),
            BinOp::Add,
            unary(UnaryOp::Neg, int(2))
        )
    );
}

/// `-2 ** 3`  →  `-(2 ** 3)`  (pow binds tighter than unary neg)
#[test]
fn pow_binds_tighter_than_unary_neg() {
    assert_eq!(
        parse_expr("-2 ** 3"),
        unary(UnaryOp::Neg, binop(int(2), BinOp::Pow, int(3)))
    );
}

/// `(-2) ** 3`  →  `(-2) ** 3`  (parens force unary first)
#[test]
fn parens_force_unary_neg_before_pow() {
    assert_eq!(
        parse_expr("(-2) ** 3"),
        binop(unary(UnaryOp::Neg, int(2)), BinOp::Pow, int(3))
    );
}

// ── Ternary `x if c else y` ───────────────────────────────────────────────────

/// `1 if 0 else 2`  →  `IfElse { then: 1, condition: 0, else: 2 }`
#[test]
fn ternary_basic() {
    assert_eq!(parse_expr("1 if 0 else 2"), ternary(int(1), int(0), int(2)));
}

/// `1 + 2 if 0 else 3`  →  `(1 + 2) if 0 else 3`  (add binds tighter than if)
#[test]
fn binop_then_branch_binds_tighter_than_if() {
    assert_eq!(
        parse_expr("1 + 2 if 0 else 3"),
        ternary(binop(int(1), BinOp::Add, int(2)), int(0), int(3))
    );
}

/// `1 if 0 else 2 + 3`  →  `1 if 0 else (2 + 3)`  (add binds tighter than if)
#[test]
fn binop_else_branch_binds_tighter_than_if() {
    assert_eq!(
        parse_expr("1 if 0 else 2 + 3"),
        ternary(int(1), int(0), binop(int(2), BinOp::Add, int(3)))
    );
}

/// `1 if 2 + 3 else 4`  →  `1 if (2 + 3) else 4`  (add binds tighter than if)
#[test]
fn binop_condition_binds_tighter_than_if() {
    assert_eq!(
        parse_expr("1 if 2 + 3 else 4"),
        ternary(int(1), binop(int(2), BinOp::Add, int(3)), int(4))
    );
}

/// `1 if 0 else 2 if 3 else 4`  →  `1 if 0 else (2 if 3 else 4)`  (right-associative)
#[test]
fn ternary_is_right_associative_on_else() {
    assert_eq!(
        parse_expr("1 if 0 else 2 if 3 else 4"),
        ternary(int(1), int(0), ternary(int(2), int(3), int(4)))
    );
}

/// `1 || 2 if 0 else 3`  →  `(1 || 2) if 0 else 3`  (|| binds tighter than if)
#[test]
fn logical_or_binds_tighter_than_if() {
    assert_eq!(
        parse_expr("1 || 2 if 0 else 3"),
        ternary(binop(int(1), BinOp::LogicalOr, int(2)), int(0), int(3))
    );
}
