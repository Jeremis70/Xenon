use num_bigint::BigInt;
use xenonc::frontend::ast::{BinOp, Binding, ExprKind, IncDecOp, Stmt, StmtKind, Type};
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
        StmtKind::Assign { target, value } => {
            assert!(matches!(&target.kind, ExprKind::Ident(s) if s == "x"));
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

// Compound assignment and increment/decrement

fn parse_single_assign(src: &str) -> Stmt {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let mut program = parser.parse_program().expect("parsing should succeed");
    program.functions.remove(0).body.remove(0)
}

/// Compound assignment keeps its own node: the operator is recorded on the
/// statement and the target appears exactly once, never rewritten into
/// `var = var <op> rhs`.
fn assert_compound(stmt: &Stmt, var: &str, expected_op: BinOp, rhs_val: i64) {
    match &stmt.kind {
        StmtKind::CompoundAssign { target, op, value } => {
            assert!(matches!(&target.kind, ExprKind::Ident(s) if s == var));
            assert_eq!(*op, expected_op);
            assert!(matches!(&value.kind, ExprKind::Int(v) if *v == bi(rhs_val)));
        }
        other => panic!("expected CompoundAssign stmt, got {:?}", other),
    }
}

#[test]
fn parse_compound_add_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x += 3;}");
    assert_compound(&stmt, "x", BinOp::Add, 3);
}

#[test]
fn parse_compound_sub_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x -= 1;}");
    assert_compound(&stmt, "x", BinOp::Sub, 1);
}

#[test]
fn parse_compound_mul_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x *= 2;}");
    assert_compound(&stmt, "x", BinOp::Mul, 2);
}

#[test]
fn parse_compound_div_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x /= 4;}");
    assert_compound(&stmt, "x", BinOp::Div, 4);
}

#[test]
fn parse_compound_mod_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x %= 7;}");
    assert_compound(&stmt, "x", BinOp::Mod, 7);
}

#[test]
fn parse_compound_bitand_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x &= 5;}");
    assert_compound(&stmt, "x", BinOp::BitwiseAnd, 5);
}

#[test]
fn parse_compound_bitor_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x |= 5;}");
    assert_compound(&stmt, "x", BinOp::BitwiseOr, 5);
}

#[test]
fn parse_compound_bitxor_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x ^= 5;}");
    assert_compound(&stmt, "x", BinOp::BitwiseXor, 5);
}

#[test]
fn parse_compound_lshift_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x <<= 1;}");
    assert_compound(&stmt, "x", BinOp::LShift, 1);
}

#[test]
fn parse_compound_rshift_assign_keeps_operator() {
    let stmt = parse_single_assign("fn f()->u32{x >>= 1;}");
    assert_compound(&stmt, "x", BinOp::RShift, 1);
}

#[test]
fn parse_increment_keeps_own_node() {
    let stmt = parse_single_assign("fn f()->u32{x++;}");
    match &stmt.kind {
        StmtKind::IncDec { target, op } => {
            assert!(matches!(&target.kind, ExprKind::Ident(s) if s == "x"));
            assert_eq!(*op, IncDecOp::Increment);
        }
        other => panic!("expected IncDec stmt, got {other:?}"),
    }
}

#[test]
fn parse_decrement_keeps_own_node() {
    let stmt = parse_single_assign("fn f()->u32{x--;}");
    match &stmt.kind {
        StmtKind::IncDec { target, op } => {
            assert!(matches!(&target.kind, ExprKind::Ident(s) if s == "x"));
            assert_eq!(*op, IncDecOp::Decrement);
        }
        other => panic!("expected IncDec stmt, got {other:?}"),
    }
}

#[test]
fn parse_increment_through_deref_keeps_single_place() {
    match parse_stmt("*p++;").kind {
        StmtKind::IncDec { target, op } => {
            assert!(matches!(&target.kind, ExprKind::Deref(_)));
            assert_eq!(op, IncDecOp::Increment);
        }
        other => panic!("expected IncDec, got {other:?}"),
    }
}

/// The parser must not invent a literal `1` that the user never wrote; `++`
/// records only the operator and its target.
#[test]
fn parse_increment_does_not_synthesize_a_literal() {
    let stmt = parse_single_assign("fn f()->u32{x++;}");
    let StmtKind::IncDec { target, .. } = &stmt.kind else {
        panic!("expected IncDec stmt, got {:?}", stmt.kind);
    };
    assert!(
        !matches!(&target.kind, ExprKind::Int(_) | ExprKind::BinOp { .. }),
        "`++` must not be rewritten into an arithmetic expression"
    );
}

/// A plain `=` still produces `Assign`, distinct from the compound form.
#[test]
fn parse_plain_assignment_is_not_compound() {
    let stmt = parse_single_assign("fn f()->u32{x = 3;}");
    match &stmt.kind {
        StmtKind::Assign { target, value } => {
            assert!(matches!(&target.kind, ExprKind::Ident(s) if s == "x"));
            assert!(matches!(&value.kind, ExprKind::Int(v) if *v == bi(3)));
        }
        other => panic!("expected Assign stmt, got {other:?}"),
    }
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
        function.returns,
        vec![Binding {
            name: None,
            ty: Type::UInt(32),
            default: None,
            span: Span::ZERO,
        }]
    );
    assert_eq!(function.body.len(), 1);

    assert!(matches!(
        &function.body[0].kind,
        StmtKind::Return(Some(expr)) if matches!(&expr.kind, ExprKind::Int(v) if *v == bi(42))
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
    let src = "fn x()->u32{return );}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("parsing should fail on an invalid return expression");
    assert_eq!(err.message, "Expected expression, found RParen");
    assert_eq!(err.span, Span { start: 19, end: 20 });
}

#[test]
fn parse_bare_return() {
    let body = parse_body("fn x()->u32{return;}");
    assert!(matches!(body[0].kind, StmtKind::Return(None)));
}

#[test]
fn parse_program_reports_eof_span_when_expression_is_missing() {
    let src = "fn x()->u32{return";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("parsing should fail at end of input");
    assert_eq!(err.message, "Expected Semicolon, found end of input");
    assert_eq!(err.span, Span { start: 18, end: 18 });
}

#[test]
fn parse_program_parses_return_ident() {
    let src = "fn x()->u32{return y;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0].kind {
        StmtKind::Return(Some(expr)) => {
            assert!(matches!(&expr.kind, ExprKind::Ident(s) if s == "y"))
        }
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
        StmtKind::Return(Some(expr)) => match &expr.kind {
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
        StmtKind::Return(Some(expr)) => match &expr.kind {
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
        StmtKind::Return(Some(expr)) => match &expr.kind {
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
        StmtKind::Return(Some(expr)) => match &expr.kind {
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
    assert_eq!(f.return_ty(), Type::Pointer(Box::new(Type::Int(32))));
}

// ── Pointer / Reference expressions ───────────────────────────────────────────

/// Parses a single statement from a function body.
fn parse_stmt(stmt_src: &str) -> Stmt {
    let src = format!("fn f()->u32{{ {stmt_src} }}");
    let tokens = lex(&src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    program.functions[0].body[0].clone()
}

#[test]
fn parse_address_of_produces_address_of_node() {
    match parse_stmt("let *i32 p = @x;").kind {
        StmtKind::VarDecl(binding) => {
            let default = binding
                .default
                .expect("declaration must have an initialiser");
            match default.kind {
                ExprKind::AddressOf(operand) => {
                    assert!(matches!(&operand.kind, ExprKind::Ident(s) if s == "x"));
                }
                other => panic!("expected AddressOf, got {other:?}"),
            }
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn parse_address_literal_produces_address_node() {
    match parse_stmt("let *u32 dev = @0xFF;").kind {
        StmtKind::VarDecl(binding) => {
            let default = binding
                .default
                .expect("declaration must have an initialiser");
            assert!(matches!(default.kind, ExprKind::Address(v) if v == bi(0xFF)));
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn parse_deref_assignment_target() {
    match parse_stmt("*p = 42;").kind {
        StmtKind::Assign { target, value } => {
            match target.kind {
                ExprKind::Deref(operand) => {
                    assert!(matches!(&operand.kind, ExprKind::Ident(s) if s == "p"));
                }
                other => panic!("expected Deref target, got {other:?}"),
            }
            assert!(matches!(&value.kind, ExprKind::Int(v) if *v == bi(42)));
        }
        other => panic!("expected Assign, got {other:?}"),
    }
}

#[test]
fn parse_double_deref_assignment_target() {
    match parse_stmt("**pp = 1;").kind {
        StmtKind::Assign { target, .. } => match target.kind {
            ExprKind::Deref(outer) => match &outer.kind {
                ExprKind::Deref(inner) => {
                    assert!(matches!(&inner.kind, ExprKind::Ident(s) if s == "pp"));
                }
                other => panic!("expected nested Deref, got {other:?}"),
            },
            other => panic!("expected Deref target, got {other:?}"),
        },
        other => panic!("expected Assign, got {other:?}"),
    }
}

/// `*p += 1;` keeps the dereference as a single target. Recording the place
/// once is what lets later stages evaluate it once.
#[test]
fn parse_compound_assignment_through_deref_keeps_single_place() {
    match parse_stmt("*p += 1;").kind {
        StmtKind::CompoundAssign { target, op, value } => {
            assert!(matches!(&target.kind, ExprKind::Deref(_)));
            assert_eq!(op, BinOp::Add);
            assert!(matches!(&value.kind, ExprKind::Int(v) if *v == bi(1)));
        }
        other => panic!("expected CompoundAssign, got {other:?}"),
    }
}

#[test]
fn parse_address_of_non_place_is_error() {
    let src = "fn f()->u32{ let *i32 p = @g(); return 0; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let err = parser
        .parse_program()
        .expect_err("`@` on a call result should be rejected");
    assert!(
        err.message.contains("addressable location"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn parse_assignment_to_address_of_is_error() {
    let src = "fn f()->u32{ @x = 1; }";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    parser
        .parse_program()
        .expect_err("`@x` is not an assignable place");
}

// Conditionals

fn parse_body(src: &str) -> Vec<Stmt> {
    let tokens = lex(src).expect("lexing should succeed");
    let mut program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    program.functions.remove(0).body
}

fn parse_error(src: &str) -> (String, Span) {
    let tokens = lex(src).expect("lexing should succeed");
    let err = Parser::new(&tokens)
        .parse_program()
        .expect_err("parsing should fail");
    (err.message, err.span)
}

#[test]
fn parse_if_else_if_else_chain() {
    let body = parse_body("fn f(bool a, bool b)->i32{ if a { } else if b { } else { return 1; } }");
    let StmtKind::If {
        inverted: false,
        else_branch: Some(else_branch),
        ..
    } = &body[0].kind
    else {
        panic!("expected if/else, got {:?}", body[0].kind);
    };
    assert!(matches!(
        &else_branch[0].kind,
        StmtKind::If {
            inverted: false,
            else_branch: Some(last),
            ..
        } if matches!(last[0].kind, StmtKind::Return(_))
    ));
}

#[test]
fn parse_unless_is_an_inverted_if_without_else() {
    let body = parse_body("fn f(bool a)->i32{ unless a { return 1; } }");
    assert!(matches!(
        &body[0].kind,
        StmtKind::If {
            inverted: true,
            then_branch,
            else_branch: None,
            ..
        } if matches!(then_branch[0].kind, StmtKind::Return(_))
    ));
}

#[test]
fn parse_unless_rejects_else() {
    let src = "fn f(bool a)->i32{ unless a { } else { } }";
    let (message, span) = parse_error(src);
    assert_eq!(message, "`unless` cannot have an `else` branch");
    assert_eq!(&src[span.start..span.end], "else");
}

#[test]
fn parse_else_unless_is_rejected() {
    let (message, _) = parse_error("fn f(bool a)->i32{ if a { } else unless a { } }");
    assert_eq!(message, "Expected LBrace, found Unless");
}

#[test]
fn parse_trailing_if_wraps_the_statement() {
    let src = "fn f(bool c)->i32{ return 1 if c; }";
    let body = parse_body(src);
    let StmtKind::If {
        inverted: false,
        condition,
        then_branch,
        else_branch: None,
    } = &body[0].kind
    else {
        panic!("expected if, got {:?}", body[0].kind);
    };
    assert!(matches!(&condition.kind, ExprKind::Ident(name) if name == "c"));
    assert_eq!(
        &src[then_branch[0].span.start..then_branch[0].span.end],
        "return 1"
    );
    assert_eq!(&src[body[0].span.start..body[0].span.end], "return 1 if c;");
}

#[test]
fn parse_trailing_unless_on_assignment() {
    let body = parse_body("fn f(bool c)->i32{ let i32 x = 0; x++ unless c; return x; }");
    assert!(matches!(
        &body[1].kind,
        StmtKind::If {
            inverted: true,
            then_branch,
            ..
        } if matches!(then_branch[0].kind, StmtKind::IncDec { .. })
    ));
}

#[test]
fn parse_trailing_if_on_valueless_break() {
    let body = parse_body("fn f(bool c)->i32{ loop { break if c; } return 0; }");
    let StmtKind::Expr(loop_expr) = &body[0].kind else {
        panic!("expected loop, got {:?}", body[0].kind);
    };
    let ExprKind::Loop { body } = &loop_expr.kind else {
        panic!("expected loop, got {:?}", loop_expr.kind);
    };
    assert!(matches!(
        &body[0].kind,
        StmtKind::If { then_branch, .. } if matches!(then_branch[0].kind, StmtKind::Break(None))
    ));
}

#[test]
fn parse_ternary_is_not_a_trailing_condition() {
    let body = parse_body("fn f(bool c)->i32{ return 1 if c else 2; }");
    assert!(matches!(
        &body[0].kind,
        StmtKind::Return(Some(value)) if matches!(value.kind, ExprKind::IfElse { .. })
    ));
}

#[test]
fn parse_ternary_without_else_is_rejected_in_expressions() {
    let (message, _) = parse_error("fn f(bool c)->i32{ let i32 x = 1 if c; return x; }");
    assert_eq!(message, "expected `else` to complete the `if` expression");
}

#[test]
fn parse_trailing_condition_after_ternary_guards_the_statement() {
    let body = parse_body("fn f(bool a, bool b)->i32{ return 1 if a else 2 if b; }");
    assert!(matches!(
        &body[0].kind,
        StmtKind::If { inverted: false, then_branch, .. }
            if matches!(&then_branch[0].kind, StmtKind::Return(Some(value))
                if matches!(value.kind, ExprKind::IfElse { .. }))
    ));
}

#[test]
fn parse_nested_trailing_conditions_in_linear_time() {
    // Every condition is parsed once; reparsing would double the work per level.
    let mut condition = "true".to_owned();
    for _ in 0..64 {
        condition = format!("loop {{ break true if {condition}; break false; }}");
    }
    let body = parse_body(&format!("fn f()->i32{{ return 1 if {condition}; }}"));
    assert!(matches!(body[0].kind, StmtKind::If { .. }));
}

// Tuples, patterns, and multiple results

fn parse_function(src: &str) -> xenonc::frontend::ast::Function {
    let tokens = lex(src).expect("lexing should succeed");
    let mut program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    program.functions.remove(0)
}

fn returned(src: &str) -> ExprKind {
    match &parse_body(src)[0].kind {
        StmtKind::Return(Some(value)) => value.kind.clone(),
        other => panic!("expected a return value, got {other:?}"),
    }
}

#[test]
fn parse_tuple_types() {
    let decl = |src: &str| match &parse_body(src)[0].kind {
        StmtKind::VarDecl(binding) => binding.ty.clone(),
        other => panic!("expected VarDecl, got {other:?}"),
    };
    assert_eq!(decl("fn f(){ let () u = (); }"), Type::unit());
    assert_eq!(decl("fn f(){ let (i32) x = 1; }"), Type::Int(32));
    assert_eq!(
        decl("fn f(){ let (i32,) x = (1,); }"),
        Type::Tuple(vec![Type::Int(32)])
    );
    assert_eq!(
        decl("fn f(){ let (i32, (bool, *u8)) x = y; }"),
        Type::Tuple(vec![
            Type::Int(32),
            Type::Tuple(vec![Type::Bool, Type::Pointer(Box::new(Type::UInt(8)))]),
        ])
    );
}

#[test]
fn parse_tuple_expressions_and_grouping() {
    assert!(matches!(returned("fn f(){ return (); }"), ExprKind::Tuple(e) if e.is_empty()));
    assert!(matches!(
        returned("fn f(){ return (x); }"),
        ExprKind::Ident(_)
    ));
    assert!(matches!(returned("fn f(){ return (x,); }"), ExprKind::Tuple(e) if e.len() == 1));
    assert!(matches!(returned("fn f(){ return (x, y, z); }"), ExprKind::Tuple(e) if e.len() == 3));
    assert!(matches!(returned("fn f(){ return x, y; }"), ExprKind::Tuple(e) if e.len() == 2));
}

#[test]
fn parse_field_access_binds_tighter_than_prefix_operators() {
    let ExprKind::UnaryOp { operand, .. } = returned("fn f(){ return -t.0.1; }") else {
        panic!("expected negation");
    };
    let ExprKind::Field { base, index: 1 } = &operand.kind else {
        panic!("expected `.1`, got {:?}", operand.kind);
    };
    assert!(matches!(&base.kind, ExprKind::Field { index: 0, .. }));
    assert!(matches!(
        returned("fn f(){ return g(x).0; }"),
        ExprKind::Field { index: 0, .. }
    ));
}

#[test]
fn parse_parallel_assignment() {
    for src in [
        "fn f(){ a, b = b, a; }",
        "fn f(){ (a, b) = (b, a); }",
        "fn f(){ t.0, t.1 = t.1, t.0; }",
        "fn f(){ _, b = g(); }",
        "fn f(){ (a, _), *p = g(); }",
    ] {
        let body = parse_body(src);
        let StmtKind::Assign { target, .. } = &body[0].kind else {
            panic!("expected assignment for {src}");
        };
        assert!(matches!(target.kind, ExprKind::Tuple(_)), "{src}");
    }
}

#[test]
fn parse_destructuring_let() {
    use xenonc::frontend::ast::Pattern;
    let body = parse_body("fn f(){ let i32 a, (bool b, _) = g(); }");
    let StmtKind::Destructure { pattern, .. } = &body[0].kind else {
        panic!("expected Destructure, got {:?}", body[0].kind);
    };
    let Pattern::Tuple(items, _) = pattern else {
        panic!("expected tuple pattern");
    };
    assert!(matches!(&items[0], Pattern::Binding(b) if b.ty == Type::Int(32)));
    assert!(
        matches!(&items[1], Pattern::Tuple(inner, _) if matches!(inner[1], Pattern::Wildcard(_)))
    );

    // A parenthesized single binding is just that binding.
    assert!(matches!(
        parse_body("fn f(){ let (i32 a) = 1; }")[0].kind,
        StmtKind::VarDecl(_)
    ));
    // A tuple type followed by a name is a plain declaration.
    assert!(matches!(
        &parse_body("fn f(){ let (i32, i32) p = (1, 2); }")[0].kind,
        StmtKind::VarDecl(b) if b.ty == Type::Tuple(vec![Type::Int(32), Type::Int(32)])
    ));
    assert!(matches!(
        parse_body("fn f(){ let (i32 a,) = t; }")[0].kind,
        StmtKind::Destructure {
            pattern: Pattern::Tuple(..),
            ..
        }
    ));
}

#[test]
fn parse_pattern_backtracking_reports_one_error() {
    // Neither a type nor a pattern: the pattern's error is reported.
    let (message, span) = parse_error("fn f(){ let (a) = 1; }");
    assert_eq!(message, "unknown type: `a`");
    assert_eq!(span, Span { start: 13, end: 14 });
    // A type without a name.
    let (message, span) = parse_error("fn f(){ let (i32, i32) = p; }");
    assert_eq!(
        message,
        "expected a variable name after the type `(i32, i32)`"
    );
    assert_eq!(span, Span { start: 23, end: 24 });
    // A pattern element without a type.
    let (message, span) = parse_error("fn f(){ let (i32 a, b) = p; }");
    assert_eq!(message, "unknown type: `b`");
    assert_eq!(span, Span { start: 20, end: 21 });
}

#[test]
fn parse_return_lists() {
    let named = parse_function("fn f() -> i32 q, i32 r { return; }");
    assert_eq!(named.returns.len(), 2);
    assert!(named.has_named_results());
    assert_eq!(
        named.return_ty(),
        Type::Tuple(vec![Type::Int(32), Type::Int(32)])
    );

    let unnamed = parse_function("fn f() -> i32, bool { return 1, true; }");
    assert!(!unnamed.has_named_results());
    assert_eq!(
        unnamed.return_ty(),
        Type::Tuple(vec![Type::Int(32), Type::Bool])
    );

    let single = parse_function("fn f() -> i32 r { return; }");
    assert_eq!(single.return_ty(), Type::Int(32));

    // No `->` and `-> ()` are the same signature.
    assert_eq!(parse_function("fn f() {}").return_ty(), Type::unit());
    assert_eq!(parse_function("fn f() -> () {}").return_ty(), Type::unit());

    let (message, _) = parse_error("fn f() -> i32 q, bool { return; }");
    assert_eq!(message, "name every result or none of them");
}

#[test]
fn parse_trailing_guard_on_a_list_guards_the_statement() {
    let body = parse_body("fn f(bool c){ return a, b if c; }");
    let StmtKind::If { then_branch, .. } = &body[0].kind else {
        panic!("expected the guard to wrap the statement");
    };
    assert!(matches!(
        &then_branch[0].kind,
        StmtKind::Return(Some(value)) if matches!(value.kind, ExprKind::Tuple(_))
    ));

    // A parenthesized conditional expression is an ordinary element.
    let ExprKind::Tuple(elems) = returned("fn f(bool c){ return a, (b if c else d); }") else {
        panic!("expected a tuple");
    };
    assert!(matches!(elems[1].kind, ExprKind::IfElse { .. }));
}

#[test]
fn parse_rejects_ambiguous_lists() {
    for (src, needle) in [
        (
            "fn f(bool c){ return a, b if c else d; }",
            "parenthesize the conditional",
        ),
        (
            "fn f(bool c){ return a if c else b, d; }",
            "parenthesize the conditional",
        ),
        (
            "fn f(bool c){ return a if c, b; }",
            "must end the statement",
        ),
        (
            "fn f(){ return x,; }",
            "one-element tuple needs parentheses",
        ),
        ("fn f(){ a, = 1; }", "one-element tuple needs parentheses"),
        (
            "fn f(){ let i32 a, = 1; }",
            "one-element pattern needs parentheses",
        ),
        (
            "fn f(){ a, b += 1; }",
            "compound assignment needs a single place",
        ),
        (
            "fn f(){ a += 1, 2; }",
            "compound assignment takes a single value",
        ),
        ("fn f(){ a, b; }", "list of values is only allowed"),
        ("fn f(){ a, 1 = 2, 3; }", "invalid assignment target"),
    ] {
        let (message, _) = parse_error(src);
        assert!(message.contains(needle), "{src}: {message}");
    }
}
