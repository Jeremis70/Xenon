use num_bigint::BigInt;
use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::validate::{validate_entry_point, validate_program};

/// Helper: parse and validate a Xenon source string.
fn validate_src(src: &str) -> Result<(), SemanticError> {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");
    validate_program(&program)
}

// ── Out-of-range constants ────────────────────────────────────────────────────

/// `u2` can hold 0..3; assigning 10 must be a hard error.
#[test]
fn u2_out_of_range_literal_is_error() {
    let err = validate_src("fn f()->u2 { let u2 x = 10; return x; }")
        .expect_err("expected ConstantOutOfRange");
    assert!(
        matches!(err, SemanticError::ConstantOutOfRange { ref name, ref value, .. } if name == "x" && *value == BigInt::from(10)),
        "unexpected error: {err}"
    );
}

/// `u2` can hold 0..3; 3 is within range and must succeed.
#[test]
fn u2_in_range_literal_is_ok() {
    validate_src("fn f()->u2 { let u2 x = 3; return x; }").expect("u2 x = 3 should be valid");
}

/// `i2` can hold -2..1; -2 is within range and must succeed.
#[test]
fn i2_negative_in_range_is_ok() {
    validate_src("fn f()->i2 { let i2 x = -2; return x; }").expect("i2 x = -2 should be valid");
}

/// `i2` can hold -2..1; 2 is out of range and must be an error.
#[test]
fn i2_out_of_range_literal_is_error() {
    let err = validate_src("fn f()->i2 { let i2 x = 2; return x; }")
        .expect_err("expected ConstantOutOfRange");
    assert!(
        matches!(err, SemanticError::ConstantOutOfRange { ref name, ref value, .. } if name == "x" && *value == BigInt::from(2)),
        "unexpected error: {err}"
    );
}

#[test]
fn bool_literal_and_return_ok() {
    validate_src("fn f()->bool { return false; }").expect("bool return");
}

#[test]
fn while_condition_must_be_bool() {
    let err =
        validate_src("fn f(u32 x)->u32 { while x { } return 0; }").expect_err("non-bool condition");
    assert!(
        matches!(err, SemanticError::ConditionNotBool { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn logical_ops_require_bool_operands() {
    let err = validate_src("fn f(u32 a, u32 b)->u32 { return a && b; }").expect_err("int && int");
    assert!(
        matches!(err, SemanticError::InvalidOperands { .. }),
        "unexpected error: {err}"
    );
}

// ── Entry point validation ────────────────────────────────────────────────────

/// Helper: parse, fold, validate entry point.
fn validate_entry(src: &str) -> Result<(), SemanticError> {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");
    validate_entry_point(&program)
}

#[test]
fn entry_point_valid() {
    validate_entry("#[entry] fn main()->i32 { return 0; }").expect("valid entry");
}

#[test]
fn entry_point_custom_name() {
    validate_entry("#[entry] fn start()->i32 { return 0; }").expect("valid entry with custom name");
}

#[test]
fn no_entry_point_is_error() {
    let err = validate_entry("fn f()->i32 { return 0; }").expect_err("no entry");
    assert!(
        matches!(err, SemanticError::NoEntryPoint),
        "unexpected error: {err}"
    );
}

#[test]
fn multiple_entry_points_is_error() {
    let err =
        validate_entry("#[entry] fn a()->i32 { return 0; } #[entry] fn b()->i32 { return 1; }")
            .expect_err("multiple entries");
    assert!(
        matches!(err, SemanticError::MultipleEntryPoints { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn entry_with_params_is_error() {
    let err = validate_entry("#[entry] fn main(i32 x)->i32 { return x; }").expect_err("has params");
    assert!(
        matches!(err, SemanticError::EntryWithParams { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn entry_wrong_return_type_is_error() {
    let err =
        validate_entry("#[entry] fn main()->u32 { return 0; }").expect_err("wrong return type");
    assert!(
        matches!(err, SemanticError::EntryWrongReturn { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn unknown_attribute_is_error() {
    let err =
        validate_entry("#[foobar] fn main()->i32 { return 0; }").expect_err("unknown attribute");
    assert!(
        matches!(err, SemanticError::UnknownAttribute { ref name, .. } if name == "foobar"),
        "unexpected error: {err}"
    );
}
