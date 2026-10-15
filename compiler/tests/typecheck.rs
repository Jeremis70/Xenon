//! Tests for type checking: AST to THIR.

use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::index::Idx;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::ids::DefId;
use xenonc::middle::target::TargetSpec;
use xenonc::middle::thir::pretty::thir_program_to_string;
use xenonc::middle::thir::{BindingKind, ExprKind, StmtKind, ThirProgram};
use xenonc::middle::typecheck::check_program;
use xenonc::types::{MAX_INT_WIDTH, Type};

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
fn mixed_sign_comparison_widens_to_a_wider_signed_type() {
    let dump = thir("fn f(i32 a, u32 b)->bool { return a == b; }");
    assert!(
        dump.contains("return Eq((a@b0 as i33 [IntToInt]), (b@b1 as i33 [IntToInt]));"),
        "{dump}"
    );
    let dump = thir("fn f(i32 a, u64 b)->bool { return a < b; }");
    assert!(
        dump.contains("return Lt((a@b0 as i65 [IntToInt]), (b@b1 as i65 [IntToInt]));"),
        "{dump}"
    );
}

#[test]
fn mixed_sign_comparison_with_wider_signed_operand_uses_it() {
    let dump = thir("fn f(i64 a, u32 b)->bool { return a < b; }");
    assert!(
        dump.contains("return Lt(a@b0, (b@b1 as i64 [IntToInt]));"),
        "{dump}"
    );
}

#[test]
fn mixed_sign_comparison_rejects_widening_beyond_the_supported_limit() {
    for (lhs, rhs) in [
        (format!("i{MAX_INT_WIDTH}"), format!("u{MAX_INT_WIDTH}")),
        (format!("u{MAX_INT_WIDTH}"), "isize".to_owned()),
    ] {
        for (lhs, rhs) in [(&lhs, &rhs), (&rhs, &lhs)] {
            let err = error(&format!("fn f({lhs} a, {rhs} b)->bool {{ return a < b; }}"));
            assert!(
                matches!(err, SemanticError::InvalidOperands { .. }),
                "{err}"
            );
            assert!(
                err.to_string().contains("requires an integer wider"),
                "{err}"
            );
        }
    }
}

#[test]
fn integer_comparison_can_widen_to_the_supported_limit() {
    let unsigned_width = MAX_INT_WIDTH - 1;
    let dump = thir(&format!(
        "fn f(i32 a, u{unsigned_width} b)->bool {{ return a == b; }}"
    ));
    assert!(
        dump.contains(&format!("as i{MAX_INT_WIDTH} [IntToInt]")),
        "{dump}"
    );
    check(&format!(
        "fn f(i{MAX_INT_WIDTH} a, u{unsigned_width} b)->bool {{ return a < b; }} \
         fn g(u32 a, u{MAX_INT_WIDTH} b)->bool {{ return a < b; }}"
    ))
    .expect("comparisons at the maximum width should type-check");
}

#[test]
fn reference_named_return_is_error() {
    let err = error("fn f()->&i32 r { return @r; }");
    assert!(
        matches!(err, SemanticError::ReferenceNamedReturn { .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn logical_operators_stay_logical() {
    let dump = thir("fn f(bool a, bool b)->bool { return a and b; }");
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

#[test]
fn address_literal_comparisons_take_the_pointer_type_in_either_order() {
    for comparison in ["p == @0x0", "@0x0 == p", "p != @0x0", "@0x0 != p"] {
        let source = format!("fn f(*i32 p)->bool {{ return {comparison}; }}");
        let program = check(&source).expect("address literal should adopt pointer type");
        let StmtKind::Return(expr) = &program.functions[DefId::new(0)].body.stmts[0].kind else {
            panic!("expected return");
        };
        let ExprKind::Binary { lhs, rhs, .. } = &expr.kind else {
            panic!("expected comparison");
        };
        let pointer_ty = Type::Pointer(Box::new(Type::Int(32)));
        assert_eq!(lhs.ty, pointer_ty, "{source}");
        assert_eq!(rhs.ty, pointer_ty, "{source}");
    }
}

#[test]
fn address_of_comparisons_take_the_returned_reference_type_in_either_order() {
    for comparison in ["reference(@x) == @x", "@x == reference(@x)"] {
        let source = format!(
            "fn reference(&i32 value)->&i32 {{ return @value; }} \
             fn f(i32 x)->bool {{ return {comparison}; }}"
        );
        let program = check(&source).expect("address-of should adopt reference flavor");
        let StmtKind::Return(expr) = &program.functions[DefId::new(1)].body.stmts[0].kind else {
            panic!("expected return");
        };
        let ExprKind::Binary { lhs, rhs, .. } = &expr.kind else {
            panic!("expected comparison");
        };
        let reference_ty = Type::Reference(Box::new(Type::Int(32)));
        assert_eq!(lhs.ty, reference_ty, "{source}");
        assert_eq!(rhs.ty, reference_ty, "{source}");
    }
}

#[test]
fn conditional_address_branches_take_the_other_branch_type() {
    for branches in [
        "p if c else @0x0",
        "@0x0 if c else p",
        "p if c else @x",
        "@x if c else p",
    ] {
        check(&format!(
            "fn f(bool c, *i32 p, i32 x)->bool {{ \
             return ({branches}) == p; }}"
        ))
        .expect("conditional branch should adopt pointer type without an enclosing expectation");
    }
    for branches in ["reference(@x) if c else @x", "@x if c else reference(@x)"] {
        check(&format!(
            "fn reference(&i32 value)->&i32 {{ return @value; }} \
             fn f(bool c, i32 x)->bool {{ \
             return ({branches}) == reference(@x); }}"
        ))
        .expect(
            "conditional branch should adopt reference flavor without an enclosing expectation",
        );
    }
}

#[test]
fn contextual_address_pairs_still_reject_mismatched_pointees() {
    for comparison in ["p == @x", "@x == p"] {
        let err = error(&format!(
            "fn f(*i32 p, u32 x)->bool {{ return {comparison}; }}"
        ));
        assert!(
            matches!(err, SemanticError::InvalidOperands { .. }),
            "{err}"
        );
    }
    for branches in ["p if c else @x", "@x if c else p"] {
        let err = error(&format!(
            "fn f(bool c, *i32 p, u32 x)->bool {{ return ({branches}) == p; }}"
        ));
        assert!(matches!(err, SemanticError::TypeMismatch { .. }), "{err}");
    }
}

#[test]
fn contextual_address_pairs_still_reject_pointer_ordering() {
    for comparison in ["p < @0x0", "@0x0 > p", "p <= @x", "@x >= p"] {
        let err = error(&format!(
            "fn f(*i32 p, i32 x)->bool {{ return {comparison}; }}"
        ));
        assert!(
            matches!(err, SemanticError::InvalidOperands { .. }),
            "{err}"
        );
    }
}

// ── Loops ─────────────────────────────────────────────────────────────────────

#[test]
fn loop_type_comes_from_break_value() {
    let dump = thir("fn f()->i32 { let i32 x = loop { break 3; }; return x; }");
    assert!(dump.contains("(loop: i32) loop {"), "{dump}");
    assert!(dump.contains("break 3_i32;"), "{dump}");
}

#[test]
fn reference_loop_type_from_context_is_error() {
    for expr in ["loop { }", "loop { break; }", "loop { break @value; }"] {
        let err = error(&format!("fn f(&i32 value)->&i32 {{ return {expr}; }}"));
        assert!(
            matches!(err, SemanticError::ReferenceLoop { ref ty, .. }
                if *ty == Type::Reference(Box::new(Type::Int(32)))),
            "unexpected error: {err}"
        );
        assert!(err.span().is_some(), "loop diagnostic must have a span");
    }
}

#[test]
fn reference_loop_type_from_break_value_is_error() {
    let err = error(
        "fn reference(&i32 value)->&i32 { return @value; } \
         fn f(i32 value)->i32 { loop { break reference(@value); } return 0; }",
    );
    assert!(matches!(err, SemanticError::ReferenceLoop { .. }), "{err}");
}

#[test]
fn non_reference_loop_types_are_preserved() {
    check(
        "fn f(*i32 p)->*i32 { return loop { break p; }; } \
           fn g()->bool { return loop { break true; }; }",
    )
    .expect("pointer and boolean loop results should remain supported");
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

#[test]
fn unless_runs_its_body_on_the_false_edge() {
    let dump = thir("fn f(bool c)->i32 { unless c { return 1; } return 0; }");
    assert!(
        dump.contains("    if c@b0 {\n    } else {\n        return 1_i32;\n    }"),
        "{dump}"
    );
}

#[test]
fn unless_requires_bool_condition() {
    let err = error("fn f()->i32 { unless 1 { return 1; } return 0; }");
    assert!(
        matches!(err, SemanticError::ConditionNotBool { .. }),
        "{err:?}"
    );
}
