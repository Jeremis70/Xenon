use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::target::TargetSpec;
use xenonc::middle::typecheck::{check_program, validate_entry_point};

fn validate_entry(source: &str) -> Result<(), SemanticError> {
    let tokens = lex(source).expect("source should lex");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("source should parse");
    let thir = check_program(&program, &TargetSpec::host())?;
    validate_entry_point(&thir).map(|_| ())
}

#[test]
fn accepts_u8_entry_with_custom_name() {
    validate_entry("#[entry] fn start() -> u8 { return 0; }").expect("valid entry");
}

#[test]
fn accepts_unit_entry_with_or_without_return_type() {
    validate_entry("#[entry] fn main() {}").expect("valid entry");
    validate_entry("#[entry] fn main() -> () { return; }").expect("valid entry");
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
        validate_entry("#[entry] fn main(u8 value) -> u8 { return value; }"),
        Err(SemanticError::EntryWithParams { .. })
    ));
}

#[test]
fn rejects_wrong_entry_return_type() {
    assert!(matches!(
        validate_entry("#[entry] fn main() -> u32 { return 0; }"),
        Err(SemanticError::EntryWrongReturn { .. })
    ));
    // Wider exit codes would be truncated to 8 bits by POSIX systems.
    assert!(matches!(
        validate_entry("#[entry] fn main() -> i32 { return 0; }"),
        Err(SemanticError::EntryWrongReturn { ref found, .. }) if *found == xenonc::types::Type::Int(32)
    ));
}

#[test]
fn rejects_unknown_function_attributes() {
    assert!(matches!(
        validate_entry("#[unknown] fn main() -> i32 { return 0; }"),
        Err(SemanticError::UnknownAttribute { .. })
    ));
}
