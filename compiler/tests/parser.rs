use num_bigint::BigInt;
use xenonc::frontend::ast::{BinOp, Binding, ExprKind, Stmt, StmtKind, Type};
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::frontend::tokens::Span;

fn bi(n: i64) -> BigInt {
    BigInt::from(n)
}

// Variable declarations

#[test]
fn parse_var_decl_produces_correct_name_type_and_value() {
    let src = "fn f()->u32{let u32 x = 5;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("x"));
            assert_eq!(binding.ty, Type::UInt(32));
            assert!(
                matches!(&binding.default.as_deref().map(|e| &e.kind), Some(ExprKind::Int(v)) if *v == bi(5))
            );
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_accepts_signed_integer_type() {
    let src = "fn f()->i32{let i64 count = 0;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("count"));
            assert_eq!(binding.ty, Type::Int(64));
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_rejects_unknown_type() {
    let src = "fn f()->u32{let foo x = 1;}";
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

// Plain assignment

#[test]
fn parse_plain_assignment_produces_assign_stmt() {
    let src = "fn f()->u32{x = 10;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Assign { name, value } => {
            assert_eq!(name, "x");
            assert!(matches!(&value.kind, ExprKind::Int(v) if *v == bi(10)));
        }
        other => panic!("expected Assign, got {:?}", other),
    }
}

#[test]
fn parse_assignment_to_call_result_is_error() {
    // Statement parsing is expression-first: `foo() = 1;` parses `foo()` as
    // an expression, then rejects it as an assignment target since it isn't
    // a bare identifier.
    let src = "fn f()->u32{ foo() = 1; return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("assigning to a call result should fail");
    assert!(
        err.message.contains("invalid assignment target"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn parse_bare_expression_statement_is_allowed() {
    // Expression-first statement parsing allows any expression starting
    // with an identifier (not just calls) as a statement when it isn't
    // followed by an assignment operator.
    let src = "fn f()->u32{ x + 2; return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Expr(expr) => {
            assert!(matches!(&expr.kind, ExprKind::BinOp { op: BinOp::Add, .. }));
        }
        other => panic!("expected Expr, got {:?}", other),
    }
}

// Compound assignment desugaring

fn parse_single_assign(src: &str) -> Stmt {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let mut program = parser.parse_program().expect("parsing should succeed");
    program.functions.remove(0).body.remove(0)
}

fn assert_desugared(stmt: &Stmt, var: &str, expected_op: BinOp, rhs_val: i64) {
    match &stmt.kind {
        StmtKind::Assign { name, value } => {
            assert_eq!(name, var);
            match &value.kind {
                ExprKind::BinOp { lhs, op, rhs } => {
                    assert!(matches!(&lhs.kind, ExprKind::Ident(s) if s == var));
                    assert_eq!(*op, expected_op);
                    assert!(matches!(&rhs.kind, ExprKind::Int(v) if *v == bi(rhs_val)));
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
            span: Span::ZERO,
        }
    );
    assert_eq!(function.body.len(), 1);

    assert!(matches!(
        &function.body[0].kind,
        StmtKind::Return(expr) if matches!(&expr.kind, ExprKind::Int(v) if *v == bi(42))
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
                span: Span::ZERO
            },
            Binding {
                name: Some("y".to_string()),
                ty: Type::UInt(64),
                default: None,
                span: Span::ZERO
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
        "Expected expression, found Semicolon"
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
        "expected expression, found end of input"
    );
    assert_eq!(err.span, Span { start: 18, end: 18 });
}

#[test]
fn parse_program_parses_return_ident() {
    let src = "fn x()->u32{return y;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Return(expr) => assert!(matches!(&expr.kind, ExprKind::Ident(s) if s == "y")),
        other => panic!("Expected return statement, got {:?}", other),
    }
}

// Function calls

#[test]
fn parse_call_no_args() {
    let src = "fn f()->u32{ return foo(); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Return(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
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

    match &program.functions[0].body[0].kind {
        StmtKind::Return(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "inc");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].kind, ExprKind::Int(v) if *v == bi(1)));
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

    match &program.functions[0].body[0].kind {
        StmtKind::Return(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "add");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, ExprKind::Int(v) if *v == bi(1)));
                assert!(matches!(&args[1].kind, ExprKind::Int(v) if *v == bi(2)));
                assert!(matches!(&args[2].kind, ExprKind::Int(v) if *v == bi(3)));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_call_arg_can_be_expression() {
    let src = "fn f()->u32{ return twice(x + 1); }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Return(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "twice");
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    &args[0].kind,
                    ExprKind::BinOp { op: BinOp::Add, .. }
                ));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

// Call statements

#[test]
fn parse_call_stmt_no_args() {
    let src = "fn f()->u32{ foo(); return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "foo");
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Expr(Call), got {:?}", other),
    }
}

#[test]
fn parse_call_stmt_with_args() {
    let src = "fn f()->u32{ log(1, 2); return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "log");
                assert_eq!(args.len(), 2);
                assert!(matches!(&args[0].kind, ExprKind::Int(v) if *v == bi(1)));
                assert!(matches!(&args[1].kind, ExprKind::Int(v) if *v == bi(2)));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Expr(Call), got {:?}", other),
    }
}

#[test]
fn parse_call_stmt_arg_can_be_expression() {
    let src = "fn f()->u32{ sink(x + 1); return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Call { name, args } => {
                assert_eq!(name, "sink");
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    &args[0].kind,
                    ExprKind::BinOp { op: BinOp::Add, .. }
                ));
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Expr(Call), got {:?}", other),
    }
}

#[test]
fn parse_call_stmt_can_appear_multiple_times_in_body() {
    let src = "fn f()->u32{ a(); b(); return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    let body = &program.functions[0].body;
    assert_eq!(body.len(), 3);
    assert!(
        matches!(&body[0].kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::Call { name, .. } if name == "a"))
    );
    assert!(
        matches!(&body[1].kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::Call { name, .. } if name == "b"))
    );
}

#[test]
fn parse_call_as_rhs_of_var_decl() {
    let src = "fn f()->u32{ let u32 y = compute(5); return y; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("y"));
            assert_eq!(binding.ty, Type::UInt(32));
            assert!(
                matches!(&binding.default.as_deref().map(|e| &e.kind), Some(ExprKind::Call { name, .. }) if name == "compute")
            );
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

// ── Attributes ────────────────────────────────────────────────────────────────

#[test]
fn parse_entry_attribute_on_function() {
    let src = "#[entry] fn main()->i32 { return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.name, "main");
    assert_eq!(f.attributes.len(), 1);
    assert_eq!(f.attributes[0].name, "entry");
}

#[test]
fn parse_multiple_attributes_on_function() {
    let src = "#[entry] #[inline] fn start()->i32 { return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    let f = &program.functions[0];
    assert_eq!(f.attributes.len(), 2);
    assert_eq!(f.attributes[0].name, "entry");
    assert_eq!(f.attributes[1].name, "inline");
}

#[test]
fn parse_function_without_attributes_has_empty_vec() {
    let src = "fn f()->i32 { return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert!(program.functions[0].attributes.is_empty());
}

#[test]
fn parse_attribute_not_followed_by_fn_is_error() {
    let src = "#[entry] 42";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser.parse_program().expect_err("should fail");
    assert!(
        err.message
            .contains("attributes must be followed by a function definition"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn parse_attribute_at_eof_is_error() {
    let src = "#[entry]";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser.parse_program().expect_err("should fail");
    assert!(
        err.message
            .contains("attributes must be followed by a function definition"),
        "unexpected error: {}",
        err.message
    );
}

/// Dots are illegal in Xenon identifiers, so a user can never define a function
/// whose name would collide with an internal mangled name like `_xe.main`.
/// The parser rejects it because `fn _xe` parses the name as `_xe`, then the
/// `.` is unexpected where `(` is required.
#[test]
fn function_name_with_dot_is_rejected() {
    let src = "fn _xe.main()->i32 { return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("dotted name should fail to parse");
    assert!(
        err.message.contains("LParen"),
        "expected a 'LParen' error, got: {}",
        err.message
    );
}

// ── Pointer / Reference types ─────────────────────────────────────────────────

#[test]
fn parse_var_decl_pointer_type() {
    let src = "fn f()->u32{ let *i32 p = 0; return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("p"));
            assert_eq!(binding.ty, Type::Pointer(Box::new(Type::Int(32))));
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_reference_type() {
    let src = "fn f()->u32{ let &i32 r = 0; return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("r"));
            assert_eq!(binding.ty, Type::Reference(Box::new(Type::Int(32))));
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_var_decl_nested_pointer_reference() {
    let src = "fn f()->u32{ let *&i32 p = 0; return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::VarDecl(binding) => {
            assert_eq!(binding.name.as_deref(), Some("p"));
            assert_eq!(
                binding.ty,
                Type::Pointer(Box::new(Type::Reference(Box::new(Type::Int(32)))))
            );
        }
        other => panic!("expected VarDecl, got {:?}", other),
    }
}

#[test]
fn parse_function_with_pointer_param() {
    let src = "fn f(*i32 p)->u32{ return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    let f = &program.functions[0];
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.params[0].name.as_deref(), Some("p"));
    assert_eq!(f.params[0].ty, Type::Pointer(Box::new(Type::Int(32))));
}

#[test]
fn parse_pointer_return_type() {
    let src = "fn f()->*i32{ return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    let f = &program.functions[0];
    assert_eq!(f.return_type.ty, Type::Pointer(Box::new(Type::Int(32))));
}
