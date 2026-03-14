use xenonc::ast::{BinOp, Expr, Param, Stmt, Type};
use xenonc::lexer::lex;
use xenonc::parser::Parser;
use xenonc::tokens::Span;

// ── Variable declarations ────────────────────────────────────────────────────

#[test]
fn parse_var_decl_produces_correct_name_type_and_value() {
    let src = "fn f()->u32{u32 x = 5;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::VarDecl { name, ty, value } => {
            assert_eq!(name, "x");
            assert_eq!(*ty, Type::UInt(32));
            assert!(matches!(value.as_ref(), Expr::Int(5)));
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_accepts_signed_integer_type() {
    let src = "fn f()->i32{i64 count = 0;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::VarDecl { name, ty, .. } => {
            assert_eq!(name, "count");
            assert_eq!(*ty, Type::Int(64));
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_rejects_unknown_type() {
    let src = "fn f()->u32{foo x = 1;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("parsing should fail on unknown type");

    assert!(
        err.message.contains("unknown type"),
        "unexpected error: {}",
        err.message
    );
}

// ── Plain assignment ─────────────────────────────────────────────────────────

#[test]
fn parse_plain_assignment_produces_assign_stmt() {
    let src = "fn f()->u32{x = 10;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Assign { name, value } => {
            assert_eq!(name, "x");
            assert!(matches!(value.as_ref(), Expr::Int(10)));
        }
        other => panic!("expected Assign, got {:?}", other),
    }
}

// ── Compound assignment desugaring ───────────────────────────────────────────

fn parse_single_assign(src: &str) -> Stmt {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let mut program = parser.parse_program().expect("parsing should succeed");
    program.functions.remove(0).body.remove(0)
}

fn assert_desugared(stmt: &Stmt, var: &str, expected_op: BinOp, rhs_val: i64) {
    match stmt {
        Stmt::Assign { name, value } => {
            assert_eq!(name, var);
            match value.as_ref() {
                Expr::BinOp { lhs, op, rhs } => {
                    assert!(matches!(lhs.as_ref(), Expr::Ident(s) if s == var));
                    assert_eq!(*op, expected_op);
                    assert!(matches!(rhs.as_ref(), Expr::Int(v) if *v == rhs_val));
                }
                other => panic!("expected BinOp in desugared value, got {:?}", other),
            }
        }
        other => panic!("expected Assign stmt, got {:?}", other),
    }
}

#[test]
fn parse_compound_add_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x += 3;}");
    assert_desugared(&stmt, "x", BinOp::Add, 3);
}

#[test]
fn parse_compound_sub_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x -= 1;}");
    assert_desugared(&stmt, "x", BinOp::Sub, 1);
}

#[test]
fn parse_compound_mul_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x *= 2;}");
    assert_desugared(&stmt, "x", BinOp::Mul, 2);
}

#[test]
fn parse_compound_div_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x /= 4;}");
    assert_desugared(&stmt, "x", BinOp::Div, 4);
}

#[test]
fn parse_compound_mod_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x %= 7;}");
    assert_desugared(&stmt, "x", BinOp::Mod, 7);
}

#[test]
fn parse_compound_pow_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x **= 2;}");
    assert_desugared(&stmt, "x", BinOp::Pow, 2);
}

#[test]
fn parse_compound_bitand_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x &= 5;}");
    assert_desugared(&stmt, "x", BinOp::BitwiseAnd, 5);
}

#[test]
fn parse_compound_bitor_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x |= 5;}");
    assert_desugared(&stmt, "x", BinOp::BitwiseOr, 5);
}

#[test]
fn parse_compound_bitxor_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x ^= 5;}");
    assert_desugared(&stmt, "x", BinOp::BitwiseXor, 5);
}

#[test]
fn parse_compound_lshift_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x <<= 1;}");
    assert_desugared(&stmt, "x", BinOp::LShift, 1);
}

#[test]
fn parse_compound_rshift_assign_desugars_to_binop() {
    let stmt = parse_single_assign("fn f()->u32{x >>= 1;}");
    assert_desugared(&stmt, "x", BinOp::RShift, 1);
}

#[test]
fn parse_program_parses_minimal_function() {
    let src = "fn x()->u32{return 42;}";
    let tokens = lex(src).expect("lexing should succeed");

    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert_eq!(program.functions.len(), 1);
    let function = &program.functions[0];
    assert_eq!(function.name, "x");
    assert!(function.params.is_empty());
    assert_eq!(function.return_type, "u32");
    assert_eq!(function.body.len(), 1);

    assert!(matches!(
        &function.body[0],
        Stmt::Return(expr) if matches!(expr.as_ref(), Expr::Int(42))
    ));
}

#[test]
fn parse_function_with_parameters() {
    let src = "fn add(u32 x, u64 y)->u32{return 1;}";
    let tokens = lex(src).expect("lexing should succeed");

    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    let function = &program.functions[0];
    assert_eq!(function.name, "add");
    assert_eq!(
        function.params,
        vec![
            Param { name: "x".to_string(), ty: Type::UInt(32) },
            Param { name: "y".to_string(), ty: Type::UInt(64) },
        ]
    );
}

#[test]
fn parse_program_reports_token_span_for_invalid_return_expr() {
    let src = "fn x()->u32{return;}";
    let tokens = lex(src).expect("lexing should succeed");

    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("parsing should fail on missing return expression");

    assert_eq!(
        err.message,
        "Expected one of [Int, Ident, Minus, Bang, Tilde, LParen], found Semicolon"
    );
    assert_eq!(err.span, Span { start: 18, end: 19 });
}

#[test]
fn parse_program_reports_eof_span_when_expression_is_missing() {
    let src = "fn x()->u32{return";
    let tokens = lex(src).expect("lexing should succeed");

    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("parsing should fail at end of input");

    assert_eq!(
        err.message,
        "Expected one of [Int, Ident, Minus, Bang, Tilde, LParen], found end of input"
    );
    assert_eq!(err.span, Span { start: 18, end: 18 });
}

#[test]
fn parse_program_parses_return_ident() {
    let src = "fn x()->u32{return y;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Return(expr) => assert!(matches!(expr.as_ref(), Expr::Ident(s) if s == "y")),
        other => panic!("Expected return statement, got {:?}", other),
    }
}
