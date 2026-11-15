use num_bigint::BigInt;
use xenonc::frontend::ast::{
    BinOp, Binding, ExprKind, ForClause, ForHeader, ForIterable, Function, ImplMember, IncDecOp,
    Pattern, Program, Receiver, Stmt, StmtKind, TypeExpr, TypeExprKind,
};
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::frontend::tokens::Span;
use xenonc::types::Type;

fn bi(n: i64) -> BigInt {
    BigInt::from(n)
}

/// The syntax of the built-in scalar type `ty`.
fn prim(ty: Type) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Primitive(ty),
        span: Span::ZERO,
    }
}

/// The types after `->`, in order.
fn result_types(function: &Function) -> Vec<TypeExpr> {
    function.returns.iter().map(|r| r.ty.clone()).collect()
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
fn unknown_identifier_is_a_named_type() {
    // Only the type checker knows which names are types.
    let src = "fn f()->u32{let foo x = 1;}";
    let ty = match &parse_body(src)[0].kind {
        StmtKind::VarDecl(binding) => binding.ty.clone(),
        other => panic!("expected VarDecl, got {other:?}"),
    };
    assert_eq!(ty.kind, TypeExprKind::Named("foo".to_owned()));
    assert_eq!(&src[ty.span.start..ty.span.end], "foo");
    assert_eq!(ty.to_string(), "foo");
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
            ty: prim(Type::UInt(32)),
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
                ty: prim(Type::UInt(32)),
                default: None,
                span: Span::ZERO
            },
            Binding {
                name: Some("y".to_string()),
                ty: prim(Type::UInt(64)),
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
    assert_eq!(result_types(f), [Type::Pointer(Box::new(Type::Int(32)))]);
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
    // Keep this recursion stress test independent of the platform's test-thread stack.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            // Every condition is parsed once; reparsing would double the work per level.
            let mut condition = "true".to_owned();
            for _ in 0..64 {
                condition = format!("loop {{ break true if {condition}; break false; }}");
            }
            let body = parse_body(&format!("fn f()->i32{{ return 1 if {condition}; }}"));
            assert!(matches!(body[0].kind, StmtKind::If { .. }));
        })
        .expect("spawn parser stress-test thread")
        .join()
        .expect("parser stress test should pass");
}

// Tuples, patterns, and multiple results

fn parse_function(src: &str) -> Function {
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
    assert_eq!(message, "expected a type before `a`");
    assert_eq!(span, Span { start: 13, end: 14 });
    // A type without a name.
    let (message, span) = parse_error("fn f(){ let (i32, i32) = p; }");
    assert_eq!(
        message,
        "expected a variable name after the type `(i32, i32)`"
    );
    assert_eq!(span, Span { start: 23, end: 24 });
    // A pattern element without a type.
    // A name that cannot share a type, since none comes first.
    let (message, span) = parse_error("fn f(){ let (_, b) = p; }");
    assert_eq!(message, "expected a type before `b`");
    assert_eq!(span, Span { start: 16, end: 17 });
}

#[test]
fn parse_return_lists() {
    let named = parse_function("fn f() -> i32 q, i32 r { return; }");
    assert_eq!(named.returns.len(), 2);
    assert!(named.has_named_results());
    assert_eq!(result_types(&named), [Type::Int(32), Type::Int(32)]);

    let unnamed = parse_function("fn f() -> i32, bool { return 1, true; }");
    assert!(!unnamed.has_named_results());
    assert_eq!(result_types(&unnamed), [Type::Int(32), Type::Bool]);

    let single = parse_function("fn f() -> i32 r { return; }");
    assert_eq!(result_types(&single), [Type::Int(32)]);

    // No `->` means no results; `-> ()` spells the same signature out.
    assert!(parse_function("fn f() {}").returns.is_empty());
    assert_eq!(
        result_types(&parse_function("fn f() -> () {}")),
        [Type::unit()]
    );

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

fn field_path(expr: &ExprKind) -> Vec<u32> {
    match expr {
        ExprKind::Field { base, index } => {
            let mut path = field_path(&base.kind);
            path.push(*index);
            path
        }
        _ => Vec::new(),
    }
}

#[test]
fn parse_nested_tuple_indices() {
    for src in [
        "fn f(){ return t.0.12; }",
        "fn f(){ return t. 0 . 12; }",
        // The float `0.` ends at the second `.`; the next index follows it.
        "fn f(){ return t.0. 12; }",
        "fn f(){ return t.0.// comment\n12; }",
    ] {
        assert_eq!(field_path(&returned(src)), vec![0, 12], "{src}");
    }
}

#[test]
fn parse_rejects_invalid_tuple_indices() {
    for (src, bad) in [
        ("fn f(){ return t.0x1; }", "0x1"),
        ("fn f(){ return t.0b1; }", "0b1"),
        ("fn f(){ return t.0o7; }", "0o7"),
        ("fn f(){ return t.01; }", "01"),
        ("fn f(){ return t.1e3; }", "1e3"),
        ("fn f(){ return t.1i32; }", "1i32"),
        ("fn f(){ return t.0.1e2; }", "0.1e2"),
        ("fn f(){ return t.0.01; }", ".01"),
    ] {
        let (message, span) = parse_error(src);
        assert!(message.contains("invalid tuple index"), "{src}: {message}");
        // `.01` locates the second index, after the first one's `.`.
        let (start, len) = match bad.strip_prefix('.') {
            Some(rest) => (src.find(bad).unwrap() + 1, rest.len()),
            None => (src.find(bad).unwrap(), bad.len()),
        };
        assert_eq!(
            span,
            Span {
                start,
                end: start + len
            },
            "{src}"
        );
    }
}

#[test]
fn type_expressions_carry_spans() {
    let src = "fn f(*i32 p) {}";
    let f = parse_function(src);
    let ty = &f.params[0].ty;
    assert_eq!(*ty, Type::Pointer(Box::new(Type::Int(32))));
    assert_eq!(&src[ty.span.start..ty.span.end], "*i32");
}

// Structs

#[test]
fn let_without_type_has_a_targeted_message() {
    let (message, span) = parse_error("fn f(){ let x = 5; }");
    assert_eq!(message, "expected a type before `x`");
    assert_eq!(span, Span { start: 12, end: 13 });
    let (message, _) = parse_error("fn f(x) {}");
    assert_eq!(message, "expected a type before `x`");
    let (message, _) = parse_error("fn f(){ let *i32 = p; }");
    assert_eq!(message, "expected a variable name after the type `*i32`");
}

#[test]
fn struct_declaration_parses() {
    let src = "struct Rect { f64 width; *Rect next; }\nstruct Empty {}\nfn f() {}";
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    assert_eq!(program.functions.len(), 1);
    assert_eq!(program.structs.len(), 2);
    let rect = &program.structs[0];
    assert_eq!(rect.name, "Rect");
    assert_eq!(&src[rect.name_span.start..rect.name_span.end], "Rect");
    assert_eq!(
        &src[rect.span.start..rect.span.end],
        "struct Rect { f64 width; *Rect next; }"
    );
    let fields: Vec<_> = rect.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(fields, ["width", "next"]);
    assert_eq!(rect.fields[0].ty, Type::Float64);
    assert_eq!(rect.fields[1].ty.to_string(), "*Rect");
    assert_eq!(
        &src[rect.fields[1].span.start..rect.fields[1].span.end],
        "*Rect next"
    );
    assert!(program.structs[1].fields.is_empty());
}

#[test]
fn malformed_struct_declarations_are_rejected() {
    for src in [
        "struct { f64 x; }",
        "struct Rect { f64 x }",
        "struct Rect { f64 x, f64 y }",
        "struct Rect { x; }",
        "struct Rect { f64 x = 1.0; }",
        "struct Rect",
        "#[entry] struct Rect {}",
        "fn f() { struct Rect {} }",
    ] {
        let tokens = lex(src).expect("lexing should succeed");
        assert!(Parser::new(&tokens).parse_program().is_err(), "{src}");
    }
}

#[test]
fn member_access_parses_as_place() {
    let expr = returned("fn f(){ return r.size.width; }");
    let ExprKind::Member { base, name, .. } = &expr else {
        panic!("expected Member, got {expr:?}");
    };
    assert_eq!(name, "width");
    assert!(matches!(&base.kind, ExprKind::Member { name, .. } if name == "size"));

    // A member of a place is a place, so it can be assigned and stepped.
    let body =
        parse_body("fn f(){ r.width = 1; r.size.width += 2; r.count++; a, r.width = 1, 2; }");
    assert!(matches!(&body[0].kind, StmtKind::Assign { target, .. }
        if matches!(&target.kind, ExprKind::Member { .. })));
    assert!(matches!(&body[1].kind, StmtKind::CompoundAssign { .. }));
    assert!(matches!(&body[2].kind, StmtKind::IncDec { .. }));
    assert!(matches!(&body[3].kind, StmtKind::Assign { .. }));
    // A member of a value is not a place.
    let (message, _) = parse_error("fn f(){ g().width = 1; }");
    assert!(message.contains("assign"), "{message}");
}

#[test]
fn member_name_span_covers_the_name() {
    let src = "fn f(){ return r.width; }";
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    let StmtKind::Return(Some(expr)) = &program.functions[0].body[0].kind else {
        panic!("expected return");
    };
    let ExprKind::Member { name_span, .. } = &expr.kind else {
        panic!("expected Member");
    };
    assert_eq!(&src[name_span.start..name_span.end], "width");
    assert_eq!(&src[expr.span.start..expr.span.end], "r.width");
}

#[test]
fn tuple_index_then_member() {
    // The lexer reads `0.` as a float; the member name follows it.
    let expr = returned("fn f(){ return t.0.width; }");
    let ExprKind::Member { base, name, .. } = &expr else {
        panic!("expected Member, got {expr:?}");
    };
    assert_eq!(name, "width");
    assert!(matches!(&base.kind, ExprKind::Field { index: 0, .. }));

    let expr = returned("fn f(){ return r.pair.1; }");
    assert!(matches!(&expr, ExprKind::Field { index: 1, base }
        if matches!(&base.kind, ExprKind::Member { name, .. } if name == "pair")));
}

#[test]
fn member_needs_a_name_or_an_index() {
    for src in [
        "fn f(){ return r.; }",
        "fn f(){ return r.(x); }",
        "fn f(){ return r.if; }",
    ] {
        let (message, _) = parse_error(src);
        assert_eq!(
            message, "expected a member name or a tuple index after `.`",
            "{src}"
        );
    }
}

#[test]
fn named_call_parses() {
    let src = "fn f(){ return Rect(width = 1 + 2, height = g(x)); }";
    let expr = returned(src);
    let ExprKind::NamedCall { name, args } = &expr else {
        panic!("expected NamedCall, got {expr:?}");
    };
    assert_eq!(name, "Rect");
    let names: Vec<_> = args.iter().map(|arg| arg.name.as_str()).collect();
    assert_eq!(names, ["width", "height"]);
    assert!(matches!(&args[0].value.kind, ExprKind::BinOp { .. }));
    assert!(matches!(&args[1].value.kind, ExprKind::Call { .. }));
    assert_eq!(
        &src[args[1].name_span.start..args[1].name_span.end],
        "height"
    );

    // `==` does not name an argument, and no arguments is a plain call.
    assert!(matches!(
        returned("fn f(){ return g(a == b); }"),
        ExprKind::Call { .. }
    ));
    assert!(matches!(
        returned("fn f(){ return Empty(); }"),
        ExprKind::Call { .. }
    ));
}

#[test]
fn mixed_named_and_positional_args_rejected() {
    for src in [
        "fn f(){ return Rect(width = 1, 2); }",
        "fn f(){ return Rect(1, height = 2); }",
    ] {
        let (message, _) = parse_error(src);
        assert_eq!(
            message, "named and positional arguments cannot be mixed",
            "{src}"
        );
    }
}

// Impl blocks

fn parse_program(src: &str) -> Program {
    let tokens = lex(src).expect("lexing should succeed");
    Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed")
}

/// The functions of the first impl block of `src`.
fn impl_functions(src: &str) -> Vec<Function> {
    let mut program = parse_program(src);
    program
        .impls
        .remove(0)
        .members
        .into_iter()
        .filter_map(|member| match member {
            ImplMember::Function(function) => Some(function),
            ImplMember::Property(_) => None,
        })
        .collect()
}

#[test]
fn impl_block_parses() {
    let src =
        "struct Rect {}\nimpl Rect { fn a() {} fn b(i32 x) -> i32 { return x; } }\nimpl Rect {}";
    let program = parse_program(src);
    assert!(program.functions.is_empty());
    assert_eq!(program.impls.len(), 2);
    let block = &program.impls[0];
    assert_eq!(block.target, "Rect");
    assert_eq!(&src[block.target_span.start..block.target_span.end], "Rect");
    assert!(src[block.span.start..block.span.end].starts_with("impl Rect { fn a()"));
    let names: Vec<_> = impl_functions(src).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["a", "b"]);
    assert!(program.impls[1].members.is_empty());
}

#[test]
fn receiver_forms_parse() {
    let functions = impl_functions(
        "impl Rect { fn a(self) {} fn b(&self, f64 k) {} fn c() {} fn d(self, i32 x, i32 y) {} }",
    );
    assert!(matches!(functions[0].receiver, Some(Receiver::Value(_))));
    assert!(functions[0].params.is_empty());
    assert!(matches!(
        functions[1].receiver,
        Some(Receiver::Reference(_))
    ));
    assert_eq!(functions[1].params.len(), 1);
    assert!(functions[2].receiver.is_none());
    assert_eq!(functions[3].params.len(), 2);
    // A free function has no receiver.
    assert!(parse_function("fn f(i32 x) {}").receiver.is_none());
}

#[test]
fn method_call_parses() {
    let expr = returned("fn f(){ return r.scaled(2, x).area(); }");
    let ExprKind::MethodCall(outer) = &expr else {
        panic!("expected MethodCall, got {expr:?}");
    };
    assert_eq!(outer.name, "area");
    assert!(outer.args.is_empty());
    let ExprKind::MethodCall(inner) = &outer.receiver.kind else {
        panic!("expected MethodCall, got {:?}", outer.receiver);
    };
    assert_eq!(inner.name, "scaled");
    assert_eq!(inner.args.len(), 2);
    assert!(matches!(&inner.receiver.kind, ExprKind::Ident(name) if name == "r"));

    // A call is a statement, a value, and never a place.
    let body = parse_body("fn f(){ r.scale(2); t.0.reset(); }");
    assert!(
        matches!(&body[0].kind, StmtKind::Expr(e) if matches!(e.kind, ExprKind::MethodCall(_)))
    );
    assert!(matches!(&body[1].kind, StmtKind::Expr(_)));
    // Nor is a member of its result.
    let (message, _) = parse_error("fn f(){ Rect.unit().width = 1; }");
    assert!(message.contains("invalid assignment target"), "{message}");
    let (message, _) = parse_error("fn f(){ r.scale(2) = 1; }");
    assert!(message.contains("invalid assignment target"), "{message}");
    let (message, _) = parse_error("fn f(){ r.scale(k = 2); }");
    assert_eq!(
        message,
        "named arguments are only allowed when building a struct"
    );
}

#[test]
fn self_is_an_expression_and_a_statement_start() {
    let functions = impl_functions(
        "impl Rect { fn f(&self) -> f64 { self.width = 1.0; self.reset(); return self.width; } }",
    );
    let body = &functions[0].body;
    assert!(matches!(&body[0].kind, StmtKind::Assign { target, .. }
        if matches!(&target.kind, ExprKind::Member { base, .. }
            if matches!(&base.kind, ExprKind::Ident(name) if name == "self"))));
    assert!(matches!(&body[1].kind, StmtKind::Expr(_)));
    assert!(matches!(&body[2].kind, StmtKind::Return(Some(_))));
}

#[test]
fn self_parameter_outside_impl_rejected() {
    let message = "`self` is only allowed as the first parameter of a method";
    for src in [
        "fn f(self) {}",
        "fn f(&self) {}",
        "impl Rect { fn f(i32 x, self) {} }",
        "impl Rect { fn f(self, &self) {} }",
    ] {
        assert_eq!(parse_error(src).0, message, "{src}");
    }
}

#[test]
fn self_as_binding_name_rejected() {
    let message = "`self` is a keyword and cannot name a variable";
    for src in [
        "fn f() { let i32 self = 1; }",
        "fn f(i32 self) {}",
        "impl Rect { fn f(Rect self) {} }",
        "fn f() -> i32 self { return; }",
        "struct A { i32 self; }",
    ] {
        assert_eq!(parse_error(src).0, message, "{src}");
    }
}

#[test]
fn malformed_impl_blocks_are_rejected() {
    for src in [
        "impl { }",
        "impl Rect { i32 x; }",
        "impl Rect { fn f() {} ",
        "impl Rect { #[entry] fn f() {} }",
        "#[entry] impl Rect {}",
        "impl Rect { struct A {} }",
        "fn f() { impl Rect {} }",
    ] {
        let tokens = lex(src).expect("lexing should succeed");
        assert!(Parser::new(&tokens).parse_program().is_err(), "{src}");
    }
}

#[test]
fn expression_nodes_stay_small() {
    // The parser recurses once per nesting level with several expressions
    // on each frame, so the size of a node bounds how deep source can nest.
    let size = std::mem::size_of::<ExprKind>();
    assert!(size <= 56, "ExprKind is {size} bytes");
}

#[test]
fn property_declaration_parses() {
    let src = "impl Rect {\n    f64 area { return self.width * self.height; }\n    fn f() {}\n    (i32, *Rect) pair { return (1, @self); }\n}";
    let program = parse_program(src);
    let members = &program.impls[0].members;
    assert_eq!(members.len(), 3);
    let ImplMember::Property(area) = &members[0] else {
        panic!("expected a property, got {:?}", members[0]);
    };
    assert_eq!(area.name, "area");
    assert_eq!(area.ty, Type::Float64);
    assert_eq!(area.body.len(), 1);
    assert_eq!(&src[area.name_span.start..area.name_span.end], "area");
    assert_eq!(
        &src[area.span.start..area.span.end],
        "f64 area { return self.width * self.height; }"
    );
    assert!(matches!(&members[1], ImplMember::Function(f) if f.name == "f"));
    assert!(matches!(&members[2], ImplMember::Property(p) if p.name == "pair"));
}

#[test]
fn malformed_properties_are_rejected() {
    for src in [
        "impl Rect { f64 area; }",
        "impl Rect { f64 area() { return 1.0; } }",
        "impl Rect { f64 { return 1.0; } }",
        "impl Rect { area { return 1.0; } }",
        "impl Rect { f64 self { return 1.0; } }",
        "impl Rect { f64 area = 1.0; }",
        "f64 area { return 1.0; }",
    ] {
        let tokens = lex(src).expect("lexing should succeed");
        assert!(Parser::new(&tokens).parse_program().is_err(), "{src}");
    }
}

#[test]
fn member_named_like_an_exponent_after_a_tuple_index() {
    // `0.e1` is not a float with an exponent: it is `.0` then `.e1`.
    for (src, expected) in [
        ("fn f(){ return t.0.e1; }", "e1"),
        ("fn f(){ return t.0.E2; }", "E2"),
        ("fn f(){ return t.0.e1x; }", "e1x"),
    ] {
        let expr = returned(src);
        let ExprKind::Member { base, name, .. } = &expr else {
            panic!("{src}: expected Member, got {expr:?}");
        };
        assert_eq!(name, expected);
        assert!(
            matches!(&base.kind, ExprKind::Field { index: 0, .. }),
            "{src}"
        );
    }
    let body = parse_body("fn f(){ t.0.e2 = 5; t.1.e3(); }");
    assert!(matches!(&body[0].kind, StmtKind::Assign { .. }));
    assert!(matches!(&body[1].kind, StmtKind::Expr(_)));
    // An exponent still follows fraction digits.
    assert!(matches!(returned("fn f(){ return 1.5e3; }"), ExprKind::Float(v) if v == 1500.0));
    assert!(matches!(returned("fn f(){ return 2.; }"), ExprKind::Float(v) if v == 2.0));
}

// Shared types in `let` lists

/// The pattern of the destructuring `let` that is the first statement.
fn let_pattern(src: &str) -> Pattern {
    match parse_body(src).remove(0).kind {
        StmtKind::Destructure { pattern, .. } => pattern,
        other => panic!("expected a destructuring let, got {other:?}"),
    }
}

/// The name and type of every variable `pattern` declares, in order, with
/// `_` for a discarded value.
fn declared(pattern: &Pattern, out: &mut Vec<String>) {
    match pattern {
        Pattern::Binding(b) => out.push(format!("{} {}", b.ty, b.name.as_deref().unwrap_or("?"))),
        Pattern::Wildcard(_) => out.push("_".to_owned()),
        Pattern::Tuple(items, _) => items.iter().for_each(|item| declared(item, out)),
    }
}

fn declarations(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    declared(&let_pattern(src), &mut out);
    out
}

#[test]
fn let_names_share_the_first_type() {
    assert_eq!(
        declarations("fn f(){ let i32 a, b = 0, 1; }"),
        ["i32 a", "i32 b"]
    );
    assert_eq!(
        declarations("fn f(){ let *Rect p, q, r = x, y, z; }"),
        ["*Rect p", "*Rect q", "*Rect r"]
    );
    assert_eq!(
        declarations("fn f(){ let [2][2]u8 a, b = x, y; }"),
        ["[2][2]u8 a", "[2][2]u8 b"]
    );
    // `_` discards a value and takes no type either way.
    assert_eq!(
        declarations("fn f(){ let i32 a, _, c = t; }"),
        ["i32 a", "_", "i32 c"]
    );
    // Every name typed still works, also with a type the others repeat.
    assert_eq!(
        declarations("fn f(){ let i32 a, bool b, i32 c = t; }"),
        ["i32 a", "bool b", "i32 c"]
    );
}

#[test]
fn nested_let_lists_share_within_themselves() {
    assert_eq!(
        declarations("fn f(){ let (i32 a, b) = p; }"),
        ["i32 a", "i32 b"]
    );
    // The inner list has its own first type; the outer names share `i32`.
    assert_eq!(
        declarations("fn f(){ let i32 a, (bool b, c), d = t; }"),
        ["i32 a", "bool b", "bool c", "i32 d"]
    );
    assert_eq!(
        declarations("fn f(){ let i32 a, (bool b, u8 c) = t; }"),
        ["i32 a", "bool b", "u8 c"]
    );
}

#[test]
fn shared_name_span_is_the_name() {
    let src = "fn f(){ let i32 a, bcd = 0, 1; }";
    let Pattern::Tuple(items, _) = let_pattern(src) else {
        panic!("expected a list");
    };
    let Pattern::Binding(second) = &items[1] else {
        panic!("expected a binding");
    };
    assert_eq!(&src[second.span.start..second.span.end], "bcd");
    assert_eq!(&src[second.ty.span.start..second.ty.span.end], "i32");
}

#[test]
fn let_lists_do_not_mix_shared_and_own_types() {
    let message =
        "either every name has its own type, or only the first one does and the others share it";
    for src in [
        "fn f(){ let i32 a, bool b, c = t; }",
        "fn f(){ let i32 a, b, bool c = t; }",
        "fn f(){ let (i32 a, b, bool c) = t; }",
        "fn f(){ let i32 a, _, b, bool c = t; }",
    ] {
        assert_eq!(parse_error(src).0, message, "{src}");
    }
    let src = "fn f(){ let i32 a, bool b, c = t; }";
    let (_, span) = parse_error(src);
    assert_eq!(&src[span.start..span.end], "c");
    let src = "fn f(){ let i32 a, b, bool c = t; }";
    let (_, span) = parse_error(src);
    assert_eq!(&src[span.start..span.end], "bool c");
}

#[test]
fn a_shared_name_needs_a_typed_first_name() {
    for (src, name) in [
        ("fn f(){ let x = 5; }", "x"),
        ("fn f(){ let a, i32 b = 0, 1; }", "a"),
        ("fn f(){ let _, b = t; }", "b"),
        ("fn f(){ let (i32 a, i32 b), c = t; }", "c"),
    ] {
        assert_eq!(
            parse_error(src).0,
            format!("expected a type before `{name}`"),
            "{src}"
        );
    }
    // Sharing is for `let` lists only.
    assert_eq!(
        parse_error("fn f(i32 a, b) {}").0,
        "expected a type before `b`"
    );
    assert!(
        lex("fn f() -> i32 q, r { return; }")
            .map(|tokens| Parser::new(&tokens).parse_program().is_err())
            .expect("lexing should succeed")
    );
    assert_eq!(
        parse_error("struct A { i32 a, b; }").0,
        "Expected Semicolon, found Comma"
    );
}

// `for` and `repeat`

/// The statements of the body of the only function of `src`.
fn body_of(src: &str) -> Vec<Stmt> {
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    program.functions[0].body.clone()
}

/// The header of the `for` loop that is the first statement of `src`.
fn for_header(src: &str) -> ForHeader {
    match body_of(src).remove(0).kind {
        StmtKind::For { header, .. } => header,
        other => panic!("expected For, got {other:?}"),
    }
}

/// Whether `src` fails to parse.
fn parse_fails(src: &str) -> bool {
    let tokens = lex(src).expect("lexing should succeed");
    Parser::new(&tokens).parse_program().is_err()
}

#[test]
fn for_header_forms() {
    let header = for_header("fn f() { for x in xs { } }");
    assert_eq!(header.ty, None);
    assert_eq!(header.name.as_deref(), Some("x"));
    assert!(
        matches!(&header.iterable, ForIterable::Value(e) if matches!(&e.kind, ExprKind::Ident(n) if n == "xs"))
    );
    assert!(header.clauses.is_empty());

    let header = for_header("fn f() { for u8 x in xs { } }");
    assert_eq!(header.ty, Some(prim(Type::UInt(8))));
    assert_eq!(header.name.as_deref(), Some("x"));

    let header = for_header("fn f() { for _ in 0 until 3 { } }");
    assert_eq!(header.ty, None);
    assert_eq!(header.name, None);
    match &header.iterable {
        ForIterable::Range(range) => {
            assert!(!range.inclusive);
            assert!(range.step.is_none());
            assert!(matches!(&range.start.kind, ExprKind::Int(v) if *v == bi(0)));
            assert!(matches!(&range.end.kind, ExprKind::Int(v) if *v == bi(3)));
        }
        other => panic!("expected a range, got {other:?}"),
    }

    let header = for_header("fn f() { for [2]u8 row in grid { } }");
    assert!(matches!(
        header.ty.as_ref().map(|t| &t.kind),
        Some(TypeExprKind::Array(_, 2))
    ));
    assert_eq!(header.name.as_deref(), Some("row"));

    let header = for_header("fn f() { for i in a to b by -2 { } }");
    match &header.iterable {
        ForIterable::Range(range) => {
            assert!(range.inclusive);
            let step = range.step.as_ref().expect("a step");
            assert_eq!(step.magnitude, bi(2));
            assert!(step.negative);
        }
        other => panic!("expected a range, got {other:?}"),
    }

    let header = for_header("fn f() { for i in a + 1 until b * 2 by 4 { } }");
    match &header.iterable {
        ForIterable::Range(range) => {
            assert!(matches!(
                &range.start.kind,
                ExprKind::BinOp { op: BinOp::Add, .. }
            ));
            assert!(matches!(
                &range.end.kind,
                ExprKind::BinOp { op: BinOp::Mul, .. }
            ));
            let step = range.step.as_ref().expect("a step");
            assert_eq!(step.magnitude, bi(4));
            assert!(!step.negative);
        }
        other => panic!("expected a range, got {other:?}"),
    }
}

#[test]
fn for_clauses() {
    let header = for_header("fn f() { for x in xs with i where x > i with u8 k where k < 3 { } }");
    assert_eq!(header.clauses.len(), 4);
    assert!(matches!(&header.clauses[0], ForClause::With { ty: None, name, .. } if name == "i"));
    assert!(matches!(&header.clauses[1], ForClause::Where(_)));
    assert!(
        matches!(&header.clauses[2], ForClause::With { ty: Some(ty), name, .. } if name == "k" && *ty == prim(Type::UInt(8)))
    );
    assert!(matches!(&header.clauses[3], ForClause::Where(_)));
}

#[test]
fn repeat_statement() {
    let body = body_of("fn f() { repeat 10 { } }");
    assert!(matches!(
        &body[0].kind,
        StmtKind::Repeat { count, body } if matches!(&count.kind, ExprKind::Int(v) if *v == bi(10)) && body.is_empty()
    ));
    let body = body_of("fn f() { repeat n + 1 { g(); } }");
    assert!(matches!(
        &body[0].kind,
        StmtKind::Repeat { count, body } if matches!(&count.kind, ExprKind::BinOp { .. }) && body.len() == 1
    ));
}

#[test]
fn until_keeps_loop_meaning() {
    let body = body_of("fn f() { until done { } do { } until done for i in 0 until n { } }");
    assert_eq!(body.len(), 3);
    for stmt in &body[..2] {
        assert!(matches!(
            &stmt.kind,
            StmtKind::Expr(e) if matches!(&e.kind, ExprKind::CondLoop { inverted: true, .. })
        ));
    }
    assert!(matches!(&body[2].kind, StmtKind::For { .. }));
}

#[test]
fn for_header_errors() {
    for src in [
        "fn f() { for in xs { } }",
        "fn f() { for x xs { } }",
        "fn f() { for i in 0 to 9 by n { } }",
        "fn f() { for i in 0 to { } }",
        "fn f() { for x in xs with { } }",
        "fn f() { repeat { } }",
    ] {
        assert!(parse_fails(src), "{src} should not parse");
    }
}

#[test]
fn reserved_words_are_not_names() {
    for src in [
        "fn f() { let i32 to = 1; }",
        "fn f() { let i32 by = 1; }",
        "fn f() { let i32 in = 1; }",
        "fn f() { let i32 with = 1; }",
        "fn f() { let i32 where = 1; }",
        "fn repeat() { }",
        "struct S { i32 in; }",
    ] {
        assert!(parse_fails(src), "{src} should not parse");
    }
    assert!(!parse_fails("fn f() { let i32 step = 1; }"));
}
