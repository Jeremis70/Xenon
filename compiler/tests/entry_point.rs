use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::typecheck::validate_entry_point;

fn validate_entry(source: &str) -> Result<(), SemanticError> {
    let tokens = lex(source).expect("source should lex");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("source should parse");
    let program = fold_constants(program).expect("constants should fold");
    validate_entry_point(&program)
}

#[test]
fn accepts_i32_entry_with_custom_name() {
    validate_entry("#[entry] fn start() -> i32 { return 0; }").expect("valid entry");
}

#[test]
fn requires_an_entry_function() {
    assert!(matches!(
        validate_entry("fn f() -> i32 { return 0; }"),
        Err(SemanticError::NoEntryPoint)
    ));
}

#[test]
fn rejects_multiple_entry_functions() {
    assert!(matches!(
        validate_entry(
            "#[entry] fn first() -> i32 { return 0; } #[entry] fn second() -> i32 { return 1; }"
        ),
        Err(SemanticError::MultipleEntryPoints { .. })
    ));
}

#[test]
fn rejects_entry_parameters() {
    assert!(matches!(
        validate_entry("#[entry] fn main(i32 value) -> i32 { return value; }"),
        Err(SemanticError::EntryWithParams { .. })
    ));
}

#[test]
fn rejects_wrong_entry_return_type() {
    assert!(matches!(
        validate_entry("#[entry] fn main() -> u32 { return 0; }"),
        Err(SemanticError::EntryWrongReturn { .. })
    ));
}

#[test]
fn rejects_unknown_function_attributes() {
    assert!(matches!(
        validate_entry("#[unknown] fn main() -> i32 { return 0; }"),
        Err(SemanticError::UnknownAttribute { .. })
    ));
}
