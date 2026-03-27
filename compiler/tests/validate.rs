use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::validate::validate_program;

/// Helper: parse and validate a Xenon source string.
fn validate_src(src: &str) -> Result<(), SemanticError> {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program);
    validate_program(&program)
}

// ── Out-of-range constants ────────────────────────────────────────────────────

/// `u2` can hold 0..3; assigning 10 must be a hard error.
#[test]
fn u2_out_of_range_literal_is_error() {
    let err = validate_src("fn f()->u2 { u2 x = 10; return x; }")
        .expect_err("expected ConstantOutOfRange");
    assert!(
        matches!(err, SemanticError::ConstantOutOfRange { ref name, value: 10, .. } if name == "x"),
        "unexpected error: {err}"
    );
}

/// `u2` can hold 0..3; 3 is within range and must succeed.
#[test]
fn u2_in_range_literal_is_ok() {
    validate_src("fn f()->u2 { u2 x = 3; return x; }").expect("u2 x = 3 should be valid");
}

/// `i2` can hold -2..1; -2 is within range and must succeed.
#[test]
fn i2_negative_in_range_is_ok() {
    validate_src("fn f()->i2 { i2 x = -2; return x; }").expect("i2 x = -2 should be valid");
}

/// `i2` can hold -2..1; 2 is out of range and must be an error.
#[test]
fn i2_out_of_range_literal_is_error() {
    let err = validate_src("fn f()->i2 { i2 x = 2; return x; }")
        .expect_err("expected ConstantOutOfRange");
    assert!(
        matches!(err, SemanticError::ConstantOutOfRange { ref name, value: 2, .. } if name == "x"),
        "unexpected error: {err}"
    );
}
