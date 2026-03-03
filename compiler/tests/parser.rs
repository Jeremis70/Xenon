use xenonc::ast::Expr;
use xenonc::lexer::lex;
use xenonc::parser::Parser;
use xenonc::tokens::Span;

#[test]
fn parse_program_parses_minimal_function() {
    let src = "fn x()->u32{return 42;}";
    let tokens = lex(src).expect("lexing should succeed");

    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    assert_eq!(program.functions.len(), 1);
    let function = &program.functions[0];
    assert_eq!(function.name, "x");
    assert_eq!(function.return_type, "u32");
    assert_eq!(function.body.len(), 1);

    assert!(matches!(
        &function.body[0],
        Expr::Return(expr) if matches!(expr.as_ref(), Expr::Int(42))
    ));
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
        "Expected integer or identifier, found Semicolon"
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

    assert_eq!(err.message, "Expected expression, found end of input");
    assert_eq!(err.span, Span { start: 18, end: 18 });
}

#[test]
fn parse_program_parses_return_ident() {
    let src = "fn x()->u32{return y;}";
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");

    match &program.functions[0].body[0] {
        Expr::Return(expr) => assert!(matches!(expr.as_ref(), Expr::Ident(s) if s == "y")),
        other => panic!("Expected return statement, got {:?}", other),
    }
}
