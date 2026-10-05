//! Tests for type checking: AST to THIR.

use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::index::Idx;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::ids::DefId;
use xenonc::middle::target::TargetSpec;
use xenonc::middle::thir::pretty::thir_program_to_string;
use xenonc::middle::thir::{BindingKind, ThirProgram};
use xenonc::middle::typecheck::check_program;

const TARGET: TargetSpec = TargetSpec::new(64);

/// Parses, folds, and type-checks `src`, as the pipeline does.
fn check(src: &str) -> Result<ThirProgram, SemanticError> {
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    let program = fold_constants(program).expect("folding should succeed");
    check_program(&program, &TARGET)
}

/// The THIR dump of `src`, which must type-check.
fn thir(src: &str) -> String {
    match check(src) {
        Ok(program) => thir_program_to_string(&program),
        Err(error) => panic!("type checking failed: {error}"),
    }
}

/// The error `src` fails to type-check with.
fn error(src: &str) -> SemanticError {
    match check(src) {
        Ok(program) => panic!(
            "expected a type error, got:\n{}",
            thir_program_to_string(&program)
        ),
        Err(error) => error,
    }
}

// ── Program structure ─────────────────────────────────────────────────────────

#[test]
fn functions_get_def_ids_in_source_order() {
    let program = check("fn a()->i32 { return 1; } #[entry] fn b()->i32 { return a(); }")
        .expect("program should type-check");
    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.functions[DefId::new(0)].name, "a");
    assert_eq!(program.functions[DefId::new(1)].name, "b");
    assert_eq!(program.entry, Some(DefId::new(1)));
}

#[test]
fn program_without_entry_has_none() {
    let program = check("fn a()->i32 { return 1; }").expect("program should type-check");
    assert_eq!(program.entry, None);
}

#[test]
fn duplicate_function_is_error() {
    let err = error("fn a()->i32 { return 1; } fn a()->i32 { return 2; }");
    assert!(
        matches!(err, SemanticError::DuplicateFunction { ref name, .. } if name == "a"),
        "unexpected error: {err}"
    );
}

#[test]
fn calls_resolve_functions_declared_later() {
    let dump = thir("fn a()->i32 { return b(); } fn b()->i32 { return 1; }");
    assert!(dump.contains("return b();"), "{dump}");
}

#[test]
fn undefined_function_is_error() {
    let err = error("fn a()->i32 { return b(); }");
    assert!(
        matches!(err, SemanticError::UndefinedFunction { ref name, .. } if name == "b"),
        "unexpected error: {err}"
    );
}

#[test]
fn argument_count_mismatch_is_error() {
    let err = error("fn a(i32 x)->i32 { return a(); }");
    assert!(
        matches!(err, SemanticError::ArgumentCountMismatch { .. }),
        "unexpected error: {err}"
    );
}

// ── Bindings and scopes ───────────────────────────────────────────────────────

#[test]
fn bindings_record_their_kind() {
    let program = check("fn f(i32 a)->i32 r { let i32 x = a; return x; }")
        .expect("program should type-check");
    let function = &program.functions[DefId::new(0)];
    let kinds: Vec<BindingKind> = function.bindings.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BindingKind::Param,
            BindingKind::NamedReturn,
            BindingKind::Local
        ]
    );
    assert!(function.named_return.is_some());
}

#[test]
fn shadowing_creates_a_new_binding() {
    let dump = thir("fn f()->i32 { let i32 x = 1; let i32 x = x + 1; return x; }");
    assert!(dump.contains("let x@b1: i32 = Add(x@b0, 1_i32);"), "{dump}");
    assert!(dump.contains("return x@b1;"), "{dump}");
}

#[test]
fn declarations_do_not_leak_out_of_blocks() {
    let err = error("fn f()->i32 { if true { let i32 x = 1; } return x; }");
    assert!(
        matches!(err, SemanticError::UndefinedVariable { ref name, .. } if name == "x"),
        "unexpected error: {err}"
    );
}

#[test]
fn shadowing_in_a_block_ends_with_the_block() {
    let dump = thir("fn f()->i32 { let i32 x = 1; if true { let i32 x = 2; } return x; }");
    assert!(dump.contains("return x@b0;"), "{dump}");
}

// ── Literals ──────────────────────────────────────────────────────────────────

#[test]
fn literals_take_the_contextual_type() {
    let dump = thir("fn f()->u8 { let u8 x = 3; return x + 4; }");
    assert!(dump.contains("let x@b0: u8 = 3_u8;"), "{dump}");
    assert!(dump.contains("return Add(x@b0, 4_u8);"), "{dump}");
}

#[test]
fn literal_without_context_defaults_to_i64() {
    let dump = thir("fn f()->i32 { loop { break 5; } return 0; }");
    assert!(dump.contains("(loop: i64) loop {"), "{dump}");
    assert!(dump.contains("break 5_i64;"), "{dump}");
}

#[test]
fn literal_operand_adopts_the_other_operand_type() {
    let dump = thir("fn f(u16 a)->bool { return 3 < a; }");
    assert!(dump.contains("return Lt(3_u16, a@b0);"), "{dump}");
}

#[test]
fn out_of_range_initializer_names_the_variable() {
    let err = error("fn f()->u2 { let u2 x = 10; return x; }");
    assert!(
        matches!(err, SemanticError::ConstantOutOfRange { ref name, .. } if name == "x"),
        "unexpected error: {err}"
    );
}

#[test]
fn out_of_range_literal_operand_is_error() {
    let err = error("fn f(u8 b)->bool { return b < 300; }");
    assert!(
        matches!(err, SemanticError::LiteralOutOfRange { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn negative_literal_fits_signed_minimum() {
    let dump = thir("fn f()->i8 { return -128; }");
    assert!(dump.contains("return -128_i8;"), "{dump}");
}

#[test]
fn float_literal_takes_float_type() {
    let dump = thir("fn f()->f32 { let f32 x = 1.5; return x * 2.0; }");
    assert!(dump.contains("Mul(x@b0, 2_f32)") || dump.contains("Mul(x@b0, 2.0_f32)"));
}

// ── Operators and conversions ─────────────────────────────────────────────────

#[test]
fn mixed_integer_arithmetic_is_error() {
    let err = error("fn f(i32 a, i64 b)->i64 { return a + b; }");
    assert!(
        matches!(
            err,
            SemanticError::InvalidOperands { .. } | SemanticError::TypeMismatch { .. }
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn mixed_integer_comparison_widens_explicitly() {
    let dump = thir("fn f(i32 a, i64 b)->bool { return a < b; }");
    assert!(
        dump.contains("return Lt((a@b0 as i64 [IntToInt]), b@b1);"),
        "{dump}"
    );
}

#[test]
fn mixed_sign_comparison_of_equal_width_widens_to_unsigned() {
    let dump = thir("fn f(i32 a, u32 b)->bool { return a == b; }");
    assert!(
        dump.contains("return Eq((a@b0 as u32 [IntToInt]), b@b1);"),
        "{dump}"
    );
}

#[test]
fn logical_operators_stay_logical() {
    let dump = thir("fn f(bool a, bool b)->bool { return a && b; }");
    assert!(dump.contains("LogicalAnd(a@b0, b@b1)"), "{dump}");
}

#[test]
fn condition_must_be_bool() {
    let err = error("fn f(i32 a)->i32 { if a { return 1; } return 0; }");
    assert!(
        matches!(err, SemanticError::ConditionNotBool { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn increment_becomes_compound_assignment() {
    let dump = thir("fn f(i32 a)->i32 { a++; return a; }");
    assert!(dump.contains("a@b0 Add= 1_i32;"), "{dump}");
}

// ── Indirection ───────────────────────────────────────────────────────────────

#[test]
fn references_are_dereferenced_explicitly() {
    let dump = thir("fn f(&i32 r)->i32 { r = r + 1; return r; }");
    assert!(dump.contains("(*r@b0) = Add((*r@b0), 1_i32);"), "{dump}");
    assert!(dump.contains("return (*r@b0);"), "{dump}");
}

#[test]
fn address_of_records_the_pointer_flavor() {
    let dump = thir("fn f()->i32 { let i32 x = 1; let *i32 p = @x; let &i32 r = @x; return x; }");
    assert!(dump.contains("let p@b1: *i32 = &raw x@b0;"), "{dump}");
    assert!(dump.contains("let r@b2: &i32 = &x@b0;"), "{dump}");
}

#[test]
fn pointers_require_explicit_dereference() {
    let dump = thir("fn f(*i32 p)->i32 { *p += 2; return *p; }");
    assert!(dump.contains("(*p@b0) Add= 2_i32;"), "{dump}");
}

// ── Loops ─────────────────────────────────────────────────────────────────────

#[test]
fn loop_type_comes_from_break_value() {
    let dump = thir("fn f()->i32 { let i32 x = loop { break 3; }; return x; }");
    assert!(dump.contains("(loop: i32) loop {"), "{dump}");
    assert!(dump.contains("break 3_i32;"), "{dump}");
}

#[test]
fn conflicting_break_types_are_error() {
    let err = error(
        "fn f(bool c)->i32 { let i32 x = 0; loop { if c { break x; } break true; } return 0; }",
    );
    assert!(
        matches!(err, SemanticError::BreakTypeConflict { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn break_outside_loop_is_error() {
    let err = error("fn f()->i32 { break; return 0; }");
    assert!(
        matches!(err, SemanticError::BreakOutsideLoop { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn continue_outside_loop_is_error() {
    let err = error("fn f()->i32 { continue; return 0; }");
    assert!(
        matches!(err, SemanticError::ContinueOutsideLoop { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn conditional_loops_record_placement_and_polarity() {
    let dump = thir(
        "fn f(i32 a)->i32 { while a > 0 { a--; } until a == 5 { a++; } \
         do { a--; } until a < 0 return a; }",
    );
    assert!(dump.contains("while Gt(a@b0, 0_i32) {"), "{dump}");
    assert!(dump.contains("until Eq(a@b0, 5_i32) {"), "{dump}");
    assert!(dump.contains("} until Lt(a@b0, 0_i32);"), "{dump}");
}

// ── Dump format ───────────────────────────────────────────────────────────────

#[test]
fn thir_dump_snapshot() {
    let dump = thir("#[entry] fn main()->i32 { let i32 x = 0; while x < 10 { x++; } return x; }");
    let expected = "\
// THIR for `main` (fn0)
#[entry] fn main() -> i32 {
    let x@b0: i32 = 0_i32;
    (loop: i64) while Lt(x@b0, 10_i32) {
        x@b0 Add= 1_i32;
    };
    return x@b0;
}
";
    assert_eq!(dump, expected);
}
