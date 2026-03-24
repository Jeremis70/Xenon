use xenonc::frontend::ast::{BinOp, Binding, Expr, Stmt, Type};
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::frontend::tokens::Span;

// ── Variable declarations ────────────────────────────────────────────────────

#[test]
fn parse_var_decl_produces_correct_name_type_and_value() {
    let src = "fn f()->u32{u32 x = 5;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("x"));
            assert_eq!(binding.ty, Type::UInt(32));
            assert!(matches!(binding.default.as_deref(), Some(Expr::Int(5))));
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
        Stmt::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("count"));
            assert_eq!(binding.ty, Type::Int(64));
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
    assert_eq!(
        function.return_type,
        Binding {
            name: None,
            ty: Type::UInt(32),
            default: None,
        }
    );
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
            Binding {
                name: Some("x".to_string()),
                ty: Type::UInt(32),
                default: None,
            },
            Binding {
                name: Some("y".to_string()),
                ty: Type::UInt(64),
                default: None,
            },
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

// ── Function calls ────────────────────────────────────────────────────────────

#[test]
fn parse_call_no_args() {
    let src = "fn f()->u32{ return foo(); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Return(expr) => match expr.as_ref() {
            Expr::Call { name, args } => {
                assert_eq!(name, "foo");
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_call_single_arg() {
    let src = "fn f()->u32{ return inc(1); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Return(expr) => match expr.as_ref() {
            Expr::Call { name, args } => {
                assert_eq!(name, "inc");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], Expr::Int(1)));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_call_multiple_args() {
    let src = "fn f()->u32{ return add(1, 2, 3); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Return(expr) => match expr.as_ref() {
            Expr::Call { name, args } => {
                assert_eq!(name, "add");
                assert_eq!(args.len(), 3);
                assert!(matches!(args[0], Expr::Int(1)));
                assert!(matches!(args[1], Expr::Int(2)));
                assert!(matches!(args[2], Expr::Int(3)));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_call_arg_can_be_expression() {
    // Arguments are full expressions, not just literals.
    let src = "fn f()->u32{ return twice(x + 1); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::Return(expr) => match expr.as_ref() {
            Expr::Call { name, args } => {
                assert_eq!(name, "twice");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::BinOp { op: BinOp::Add, .. }));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_call_as_rhs_of_var_decl() {
    let src = "fn f()->u32{ u32 y = compute(5); return y; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Stmt::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("y"));
            assert_eq!(binding.ty, Type::UInt(32));
            assert!(
                matches!(binding.default.as_deref(), Some(Expr::Call { name, .. }) if name == "compute")
            );
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_multiple_functions_in_program() {
    let src = "fn add(u32 a, u32 b)->u32{ return a; } fn main()->u32{ return add(1, 2); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.functions[0].name, "add");
    assert_eq!(program.functions[0].params.len(), 2);
    assert_eq!(program.functions[1].name, "main");
    assert!(program.functions[1].params.is_empty());

    match &program.functions[1].body[0] {
        Stmt::Return(expr) => assert!(matches!(
            expr.as_ref(),
            Expr::Call { name, args } if name == "add" && args.len() == 2
        )),
        other => panic!("expected Return(Call), got {:?}", other),
    }
}

#[test]
fn parse_function_with_no_params_has_empty_params() {
    let src = "fn f()->u32{ return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert!(program.functions[0].params.is_empty());
}

// ── If statements ─────────────────────────────────────────────────────────────

/// Helper: parse a Xenon source string and return the first statement of the
/// first function body.
fn parse_first_stmt(src: &str) -> Stmt {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let mut program = parser.parse_program().expect("parsing should succeed");
    program.functions.remove(0).body.remove(0)
}

#[test]
fn parse_if_only_produces_correct_ast() {
    let stmt = parse_first_stmt("fn f()->u32{ if x { return 1; } }");
    match stmt {
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(condition.as_ref(), Expr::Ident(s) if s == "x"));
            assert_eq!(then_branch.len(), 1);
            assert!(matches!(then_branch[0], Stmt::Return(_)));
            assert!(else_branch.is_none());
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn parse_if_else_produces_both_branches() {
    let stmt = parse_first_stmt("fn f()->u32{ if x { return 1; } else { return 2; } }");
    match stmt {
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(condition.as_ref(), Expr::Ident(s) if s == "x"));
            assert_eq!(then_branch.len(), 1);
            let else_stmts = else_branch.expect("else branch should be present");
            assert_eq!(else_stmts.len(), 1);
            assert!(matches!(else_stmts[0], Stmt::Return(_)));
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn parse_else_if_is_nested_if_inside_else_branch() {
    // `else if` must be represented as an Stmt::If nested inside the else branch.
    let stmt = parse_first_stmt(
        "fn f()->u32{ if x { return 1; } else if y { return 2; } else { return 3; } }",
    );
    match stmt {
        Stmt::If { else_branch, .. } => {
            let else_stmts = else_branch.expect("outer else branch should be present");
            assert_eq!(
                else_stmts.len(),
                1,
                "else branch should hold a single nested if"
            );
            assert!(
                matches!(else_stmts[0], Stmt::If { .. }),
                "else branch should contain Stmt::If for else-if chain"
            );
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn parse_multiple_else_if_clauses_form_nested_chain() {
    // `if a {} else if b {} else if c {} else {}` must produce a fully nested chain:
    //   Stmt::If { else: Some([Stmt::If { else: Some([Stmt::If { else: Some([...]) }]) }]) }
    let stmt = parse_first_stmt(
        "fn f()->u32{ if a { return 1; } else if b { return 2; } else if c { return 3; } else { return 4; } }",
    );
    // Depth-0: outer if
    let Stmt::If { else_branch, .. } = stmt else {
        panic!("expected Stmt::If at depth 0");
    };
    let d0 = else_branch.expect("depth-0 else branch should be present");
    assert_eq!(d0.len(), 1);

    // Depth-1: first else-if
    let Stmt::If { else_branch, .. } = &d0[0] else {
        panic!("expected Stmt::If at depth 1");
    };
    let d1 = else_branch
        .as_ref()
        .expect("depth-1 else branch should be present");
    assert_eq!(d1.len(), 1);

    // Depth-2: second else-if, whose else branch is the final else block
    let Stmt::If {
        else_branch,
        then_branch,
        ..
    } = &d1[0]
    else {
        panic!("expected Stmt::If at depth 2");
    };
    assert_eq!(
        then_branch.len(),
        1,
        "depth-2 then branch should have one stmt"
    );
    let d2 = else_branch
        .as_ref()
        .expect("depth-2 else branch (the final else) should be present");
    assert_eq!(d2.len(), 1, "final else branch should have one stmt");
    assert!(matches!(d2[0], Stmt::Return(_)), "final else should return");
}

#[test]
fn parse_if_condition_can_be_comparison_expr() {
    let stmt = parse_first_stmt("fn f()->u32{ if x == 1 { return 0; } }");
    match stmt {
        Stmt::If { condition, .. } => {
            assert!(
                matches!(condition.as_ref(), Expr::BinOp { op: BinOp::Eq, .. }),
                "condition should be an Eq comparison, got {:?}",
                condition
            );
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn parse_if_then_branch_can_have_multiple_statements() {
    let stmt = parse_first_stmt("fn f()->u32{ if x { u32 a = 1; u32 b = 2; return a; } }");
    match stmt {
        Stmt::If { then_branch, .. } => {
            assert_eq!(then_branch.len(), 3);
            assert!(matches!(then_branch[0], Stmt::VarDecl(_)));
            assert!(matches!(then_branch[1], Stmt::VarDecl(_)));
            assert!(matches!(then_branch[2], Stmt::Return(_)));
        }
        other => panic!("expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn parse_if_without_else_and_without_closing_brace_is_error() {
    let src = "fn f()->u32{ if x { return 1; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    // The function body's closing `}` is missing — parsing must not succeed.
    // (The `if` block's `}` is consumed as the if body; the outer `}` is then absent.)
    let result = parser.parse_program();
    assert!(
        result.is_err(),
        "expected parse error for missing closing brace, got {:?}",
        result
    );
}
