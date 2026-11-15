//! Tests for type checking: AST to THIR.

use xenonc::error::SemanticError;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::index::Idx;
use xenonc::middle::ids::DefId;
use xenonc::middle::target::TargetSpec;
use xenonc::middle::thir::pretty::thir_program_to_string;
use xenonc::middle::thir::{BindingKind, ExprKind, StmtKind, ThirProgram};
use xenonc::middle::typecheck::check_program;
use xenonc::types::{MAX_INT_WIDTH, Type};

const TARGET: TargetSpec = TargetSpec::new(64);

#[test]
fn arrays_infer_literal_types_from_typed_elements() {
    for comparison in [
        "[x, 0] == [0, 0]",
        "[0, 0] == [x, 0]",
        "[x, x] == [0, 0]",
        "([x, 0] if c else [0, 0]) == [0, 0]",
        "([x, 0], true) == ([0, 0], true)",
    ] {
        check(&format!(
            "fn f(u8 x, bool c)->bool {{ return {comparison}; }}"
        ))
        .expect("typed array elements should provide context for untyped literals");
    }
}

#[test]
fn arrays_index_reference_valued_calls_transparently() {
    check(
        "fn id(&[2]u8 a)->&[2]u8 { return @a; }
         fn f()->u8 { let [2]u8 a = [1, 2]; return id(@a)[0]; }",
    )
    .expect("reference-valued array calls should be indexable");
    assert!(matches!(
        error(
            "fn id(*[2]u8 a)->*[2]u8 { return a; }
               fn f()->u8 { let [2]u8 a = [1, 2]; return id(@a)[0]; }"
        ),
        SemanticError::IndexOfNonArray { .. }
    ));
}

#[test]
fn arrays_reject_overlapping_parallel_assignment_targets() {
    for (source, target) in [
        ("fn f([2]u8 a) { a, a[0] = [3, 4], 5; }", "a[0]"),
        ("fn f([2]u8 a) { a[0], a = 5, [3, 4]; }", "a"),
        ("fn f([2]u8 a) { a[1], a[1] = 3, 4; }", "a[1]"),
        ("fn f(&[2]u8 a) { a, a[0] = [3, 4], 5; }", "a[0]"),
        ("fn f([2][2]u8 a) { a[0], a[0][1] = [3, 4], 5; }", "a[0][1]"),
        ("fn f([2]u8 a, u8 i) { a, a[i] = [3, 4], 5; }", "a[?]"),
        ("fn f([2]u8 a, u8 i) { a[i], a = 5, [3, 4]; }", "a"),
        (
            "fn f([2][2]u8 a, u8 i) { a[0], a[0][i] = [3, 4], 5; }",
            "a[0][?]",
        ),
    ] {
        assert!(
            matches!(
                error(source),
                SemanticError::DuplicateAssignTarget { target: found, .. } if found == target
            ),
            "{source}"
        );
    }
    check("fn f([2]u8 a) { a[0], a[1] = 3, 4; a[0, 0] = 5; }")
        .expect("distinct parallel indices and repeated broadcast indices remain valid");
    check("fn f([2]u8 a, u8 i, u8 j) { a[i], a[j] = 3, 4; }")
        .expect("potentially overlapping dynamic indices are not proven duplicates");
}

#[test]
fn arrays_typecheck_nested_literals_places_and_references() {
    let dump = thir(
        "struct Cell { [2]u8 values; }
         fn make()->[2]u8 { return [1, 2]; }
         fn f()->u8 {
             let [2][2]u8 grid = [make(), [3, 4]];
             grid[0][1] += 1;
             let &[2]u8 row = @grid[0];
             row[0] = 5;
             let [0]bool empty = [];
             let [2]&u8 refs = [@row[0], @row[1]];
             refs[1] = 6;
             let Cell cell = Cell(values = row);
             return cell.values[1];
         }",
    );
    assert!(dump.contains("[2][2]u8"), "{dump}");
    assert!(dump.contains("[0]bool"), "{dump}");
}

#[test]
fn arrays_reject_invalid_shapes_and_indices() {
    assert!(matches!(
        error("fn f() { let [2]u8 a = [1]; }"),
        SemanticError::ArrayLengthMismatch {
            expected: 2,
            found: 1,
            ..
        }
    ));
    assert!(matches!(
        error("fn f() { let [2]u8 a = [1, true]; }"),
        SemanticError::TypeMismatch { .. }
    ));
    for index in ["2", "-1"] {
        assert!(matches!(
            error(&format!(
                "fn f() {{ let [2]u8 a = [1, 2]; a[{index}] = 0; }}"
            )),
            SemanticError::ArrayIndexOutOfBounds { .. }
        ));
    }
    assert!(matches!(
        error("fn f() { let [2]u8 a = [1, 2]; a[true] = 0; }"),
        SemanticError::InvalidArrayIndex { .. }
    ));
    assert!(matches!(
        error("fn f() { let u8 a = 1; a[0] = 0; }"),
        SemanticError::IndexOfNonArray { .. }
    ));
    assert!(matches!(
        error("fn f() { let [2]u8 a = [1, 2]; let u8 b = a[0, 1]; }"),
        SemanticError::MultiIndexAsValue { .. }
    ));
    assert!(matches!(
        error("struct Recursive { [1]Recursive values; }"),
        SemanticError::RecursiveStruct { .. }
    ));
    assert!(matches!(
        error("struct Invalid { [1]&u8 values; }"),
        SemanticError::ReferenceField { .. }
    ));
}

/// Parses, folds, and type-checks `src`, as the pipeline does.
fn check(src: &str) -> Result<ThirProgram, SemanticError> {
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
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
    assert_eq!(function.named_returns.len(), 1);
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

#[test]
fn zero_sized_integers_only_hold_zero() {
    thir("fn f()->i0 { let u0 a = 0; let i0 b = -0; return b; }");
    for src in ["fn f()->u0 { return 1; }", "fn f()->i0 { return -1; }"] {
        let err = error(src);
        assert!(
            matches!(err, SemanticError::LiteralOutOfRange { .. }),
            "{src}: {err:?}"
        );
    }
}

// ── Tuples ────────────────────────────────────────────────────────────────────

#[test]
fn tuple_literal_elements_take_their_expected_types() {
    let dump = thir("fn f()->i32 { let (u8, i64) p = (1, 2); return 0; }");
    assert!(
        dump.contains("let p@b0: (u8, i64) = (1_u8, 2_i64);"),
        "{dump}"
    );
    let dump = thir("fn f()->i32 { let u8 a, i16 b = 1, 2; return 0; }");
    assert!(dump.contains("let a@b0: u8 = 1_u8;"), "{dump}");
    assert!(dump.contains("let b@b1: i16 = 2_i16;"), "{dump}");
}

#[test]
fn tuple_literal_out_of_range_names_the_variable() {
    assert!(matches!(
        error("fn f()->i32 { let u8 a, u8 b = 1, 300; return 0; }"),
        SemanticError::ConstantOutOfRange { ref name, .. } if name == "b"
    ));
}

#[test]
fn destructuring_evaluates_every_value_before_declaring() {
    // The new `x` and `y` are initialized from the old `y` and `x`.
    let dump =
        thir("fn f()->i32 { let i32 x = 1; let i32 y = 2; let i32 x, i32 y = y, x; return x; }");
    assert!(dump.contains("let x@b2: i32 = y@b1;"), "{dump}");
    assert!(dump.contains("let y@b3: i32 = x@b0;"), "{dump}");
}

#[test]
fn destructuring_a_call_goes_through_a_temporary() {
    let dump = thir(
        "fn g()->(i32, (bool, u8)) { return 1, (true, 2); }\n\
         fn f()->i32 { let i32 a, (bool b, _) = g(); return a; }",
    );
    assert!(
        dump.contains("let _@b0: (i32, (bool, u8)) = g();"),
        "{dump}"
    );
    assert!(dump.contains("let a@b1: i32 = _@b0.0;"), "{dump}");
    assert!(dump.contains("let b@b2: bool = _@b0.1.0;"), "{dump}");
}

#[test]
fn parallel_assignment_evaluates_values_before_storing() {
    let dump = thir("fn f()->i32 { let i32 a = 1; let i32 b = 2; a, b = b, a; return a; }");
    let swap = [
        "let _@b2: i32 = b@b1;",
        "let _@b3: i32 = a@b0;",
        "a@b0 = _@b2;",
        "b@b1 = _@b3;",
    ];
    let mut rest = dump.as_str();
    for needle in swap {
        let at = rest
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} in {dump}"));
        rest = &rest[at + needle.len()..];
    }
}

#[test]
fn element_places_are_assignable() {
    let dump = thir("fn f()->i32 { let (i32, i32) t = (1, 2); t.0 += 5; t.1 = t.0; return t.1; }");
    assert!(dump.contains("t@b0.0 Add= 5_i32;"), "{dump}");
    assert!(dump.contains("t@b0.1 = t@b0.0;"), "{dump}");
}

#[test]
fn reference_elements_are_dereferenced_on_access() {
    let dump = thir("fn f(&i32 r)->i32 { let (&i32, i32) t = (@r, 1); return t.0; }");
    assert!(dump.contains("return (*t@b1.0);"), "{dump}");
}

#[test]
fn tuple_equality_type_checks() {
    let dump = thir("fn f((i32, bool) t)->bool { return t == (1, true); }");
    assert!(dump.contains("return Eq(t@b0, (1_i32, true));"), "{dump}");
    assert!(matches!(
        error("fn f((i32, bool) t)->bool { return t < t; }"),
        SemanticError::InvalidOperands { .. }
    ));
    assert!(matches!(
        error("fn f((i32, bool) t, (i64, bool) u)->bool { return t == u; }"),
        SemanticError::InvalidOperands { .. }
    ));
}

#[test]
fn bare_return_returns_the_named_results() {
    let dump = thir("fn f()->i32 q, i32 r { q = 1; r = 2; return; }");
    assert!(dump.contains("return (q@b0, r@b1);"), "{dump}");
    let dump = thir("fn f()->i32 r { r = 1; return; }");
    assert!(dump.contains("return r@b0;"), "{dump}");
    let dump = thir("fn f() { return; }");
    assert!(dump.contains("return ();"), "{dump}");
}

#[test]
fn unit_spellings_are_identical() {
    // No `->` and `-> ()` produce the same signature, and `return;`,
    // `return ();`, and falling off the end mean the same.
    let implicit = thir("fn f() { return; }");
    let explicit = thir("fn f() -> () { return (); }");
    assert_eq!(implicit, explicit);
    let program = check("fn f() {}\nfn g() -> () {}").expect("should type-check");
    assert_eq!(program.functions[DefId::new(0)].return_ty, Type::unit());
    assert_eq!(program.functions[DefId::new(1)].return_ty, Type::unit());
}

#[test]
fn bare_return_with_a_shadowed_result_is_error() {
    assert!(matches!(
        error("fn f()->i32 r { r = 1; if true { let i32 r = 2; return; } return; }"),
        SemanticError::ShadowedNamedResult { ref name, .. } if name == "r"
    ));
    // Once the shadowing variable is out of scope, `return;` is fine.
    thir("fn f()->i32 r { r = 1; if true { let i32 r = 2; } return; }");
}

#[test]
fn tuple_errors() {
    type Check = fn(&SemanticError) -> bool;
    let cases: &[(&str, Check)] = &[
        ("fn f()->i32 { return 1, 2; }", |e| {
            matches!(
                e,
                SemanticError::TupleArityMismatch {
                    expected: 1,
                    found: 2,
                    ..
                }
            )
        }),
        (
            "fn f()->i32 { let i32 a, i32 b = 1, 2, 3; return a; }",
            |e| {
                matches!(
                    e,
                    SemanticError::TupleArityMismatch {
                        expected: 2,
                        found: 3,
                        ..
                    }
                )
            },
        ),
        (
            "fn g()->(i32, i32) { return 1, 2; } fn f()->i32 { let i32 a, i32 b, i32 c = g(); return a; }",
            |e| {
                matches!(
                    e,
                    SemanticError::TupleArityMismatch {
                        expected: 3,
                        found: 2,
                        ..
                    }
                )
            },
        ),
        (
            "fn f()->i32 { let (i32, i32) t = (1, 2); return t.2; }",
            |e| matches!(e, SemanticError::TupleIndexOutOfRange { index: 2, .. }),
        ),
        ("fn f()->i32 { let i32 x = 1; return x.0; }", |e| {
            matches!(e, SemanticError::NotATuple { .. })
        }),
        ("fn f(*(i32, i32) p)->i32 { return p.0; }", |e| {
            matches!(e, SemanticError::NotATuple { .. })
        }),
        ("fn f()->i32 { return; }", |e| {
            matches!(e, SemanticError::BareReturnWithoutNamedResults { .. })
        }),
        (
            "fn f()->i32 { let i32 a = 1; a, a = 1, 2; return a; }",
            |e| matches!(e, SemanticError::DuplicateAssignTarget { .. }),
        ),
        (
            "fn f()->i32 { let (i32, i32) t = (1, 2); t, t.0 = (3, 4), 5; return 0; }",
            |e| matches!(e, SemanticError::DuplicateAssignTarget { .. }),
        ),
        (
            "fn f()->i32 { let i32 a, (bool a, _) = 1, (true, 2); return a; }",
            |e| matches!(e, SemanticError::DuplicatePatternBinding { .. }),
        ),
        ("fn f()->i32 { return _; }", |e| {
            matches!(e, SemanticError::WildcardAsValue { .. })
        }),
        ("fn f()->i32 { let i32 a, bool b = 1, 2; return a; }", |e| {
            matches!(e, SemanticError::TypeMismatch { .. })
        }),
        ("fn f()->&i32 q, i32 r { return; }", |e| {
            matches!(e, SemanticError::ReferenceNamedReturn { .. })
        }),
        (
            "fn f(&i32 r)->i32 { let (&i32, i32) t = loop { break (@r, 1); }; return 0; }",
            |e| matches!(e, SemanticError::ReferenceLoop { .. }),
        ),
    ];
    for (src, check) in cases {
        let found = error(src);
        assert!(check(&found), "{src}: unexpected error {found:?}");
    }
}

#[test]
fn pointer_to_tuple_needs_an_explicit_dereference() {
    thir("fn f(*(i32, i32) p)->i32 { (*p).1 = 3; return (*p).0; }");
}

/// A conditional of tuple literals takes its type from the other operand,
/// like a conditional of scalar literals does.
#[test]
fn conditional_tuple_literals_take_the_other_operands_type() {
    let dump = thir("fn f(bool c, (u8, u8) t)->bool { return t == ((1, 2) if c else (3, 4)); }");
    assert!(dump.contains("(1_u8, 2_u8)"), "{dump}");
    assert!(dump.contains("(3_u8, 4_u8)"), "{dump}");
}

/// `_` leaves its element unconstrained without losing the expected types
/// of the other elements, through conditionals and loops.
#[test]
fn discarded_elements_keep_the_other_expectations() {
    for src in [
        "fn f(bool c) { let u8 a, _ = (1, true) if c else (2, false); }",
        "fn f(bool c) { let u8 a = 0; a, _ = (1, true) if c else (2, false); }",
        "fn f() { let u8 a, _ = loop { break (1, true); }; }",
        "fn f(bool c) { let (u8 a, _), _ = ((1, 2), 3) if c else ((4, 5), 6); }",
        "fn f(bool c, (u8, i64) t) { let u8 a, _ = t if c else (1, 2); }",
    ] {
        let dump = thir(src);
        assert!(dump.contains("1_u8"), "{src}:\n{dump}");
    }
}

// ── Casts ─────────────────────────────────────────────────────────────────────

#[test]
fn casts_classify_their_conversion() {
    for (from, to, kind) in [
        ("i32", "u8", "IntToInt"),
        ("u8", "usize", "IntToInt"),
        ("i64", "f32", "IntToFloat"),
        ("f64", "u16", "FloatToInt"),
        ("f64", "f16", "FloatToFloat"),
        ("f16", "bf16", "FloatToFloat"),
        ("*u8", "*u32", "PtrToPtr"),
        ("*u8", "usize", "PtrToInt"),
        ("usize", "*u8", "IntToPtr"),
    ] {
        let dump = thir(&format!("fn f({from} a)->{to} {{ return a as {to}; }}"));
        assert!(
            dump.contains(&format!("return (a@b0 as {to} [{kind}]);")),
            "{from} as {to}:\n{dump}"
        );
    }
}

#[test]
fn cast_to_the_same_type_is_the_operand() {
    let dump = thir("fn f(i32 a)->i32 { return a as i32; }");
    assert!(dump.contains("return a@b0;"), "{dump}");
}

#[test]
fn casts_without_a_conversion_are_errors() {
    for (from, to, note) in [
        ("bool", "u8", Some("`bool` has no casts")),
        ("u8", "bool", Some("`bool` has no casts")),
        ("*u8", "u64", Some("cast through `usize`")),
        ("u64", "*u8", Some("cast through `usize`")),
        ("*u8", "isize", Some("cast through `usize`")),
        ("*u8", "f64", None),
        ("(i32, i32)", "i64", None),
        ("i32", "(i32,)", None),
    ] {
        let err = error(&format!("fn f({from} a) {{ let {to} b = a as {to}; }}"));
        match err {
            SemanticError::InvalidCast {
                note: found_note, ..
            } => match note {
                Some(note) => {
                    assert!(
                        found_note.is_some_and(|n| n.contains(note)),
                        "{from} as {to}"
                    )
                }
                None => assert_eq!(found_note, None, "{from} as {to}"),
            },
            other => panic!("{from} as {to}: expected InvalidCast, got {other:?}"),
        }
    }
}

#[test]
fn references_cannot_be_cast_to() {
    let err = error("fn f(i32 a) { let i32 x = 0; let &i32 r = a as &i32; }");
    assert!(
        matches!(err, SemanticError::InvalidCast { note: Some(note), .. } if note.contains("references")),
        "{err:?}"
    );
}

#[test]
fn untyped_literal_operand_takes_the_target_type() {
    let dump = thir("fn f()->u8 { return 5 as u8; }");
    assert!(dump.contains("return 5_u8;"), "{dump}");
    let dump = thir("fn f()->f32 { return 1.5 as f32; }");
    assert!(!dump.contains("FloatToFloat"), "{dump}");
}

#[test]
fn untyped_literal_operand_is_range_checked_against_the_target() {
    for src in [
        "fn f()->u8 { return 300 as u8; }",
        "fn f()->u32 { return -1 as u32; }",
    ] {
        assert!(
            matches!(error(src), SemanticError::LiteralOutOfRange { .. }),
            "{src}"
        );
    }
}

#[test]
fn literal_of_another_kind_keeps_its_default_type() {
    let dump = thir("fn f()->i32 { return 2.5 as i32; }");
    assert!(dump.contains("as i32 [FloatToInt]"), "{dump}");
    let dump = thir("fn f()->f32 { return 3 as f32; }");
    assert!(dump.contains("as f32 [IntToFloat]"), "{dump}");
}

// ── Structs ───────────────────────────────────────────────────────────────────

const RECT: &str = "struct Rect { f64 width; f64 height; }\n";

/// A source and what its type error must look like.
type ErrorCase = (&'static str, fn(&SemanticError) -> bool);

/// The error of `src` checked after the declaration of `Rect`.
fn rect_error(src: &str) -> SemanticError {
    error(&format!("{RECT}{src}"))
}

#[test]
fn struct_construction_any_field_order() {
    let dump = thir(&format!(
        "{RECT}fn f()->Rect {{ return Rect(height = 4.0, width = 3.0); }}"
    ));
    // Arguments stay in source order; each takes the type of its field.
    assert!(
        dump.contains("Rect(height = 4.0_f64, width = 3.0_f64)"),
        "{dump}"
    );
    assert!(dump.contains("-> Rect"), "{dump}");
    // No fields, no arguments.
    check("struct Empty {}\nfn f()->Empty { return Empty(); }").expect("should check");
}

#[test]
fn field_read_and_write() {
    let dump = thir(&format!(
        "{RECT}fn f(Rect r)->f64 {{ r.height = 2.0; r.width += 1.0; return r.height; }}"
    ));
    assert!(dump.contains("r@b0.1 = 2.0_f64"), "{dump}");
    assert!(dump.contains("return r@b0.1"), "{dump}");
    // A field of a value that is no place can be read, not written.
    check(&format!(
        "{RECT}fn g()->Rect {{ return Rect(width = 1.0, height = 2.0); }}\n\
         fn f()->f64 {{ return g().width; }}"
    ))
    .expect("should check");
}

#[test]
fn nested_field_places() {
    let src = "struct Point { i32 x; i32 y; }\n\
               struct Line { Point from; (Point, i32) end; }\n\
               fn f(Line l, (Point, bool) t)->i32 {\n\
                   l.from.x = 1; l.end.0.y += 2; l.end.1++; t.0.x = 3;\n\
                   return l.from.x + l.end.0.y + t.0.x;\n\
               }";
    let dump = thir(src);
    assert!(dump.contains("l@b0.0.0 = 1_i32"), "{dump}");
    assert!(dump.contains("t@b1.0.0 = 3_i32"), "{dump}");
}

#[test]
fn overlapping_struct_targets_rejected() {
    for body in [
        "r, r.width = r, 1.0;",
        "r.width, r.width = 1.0, 2.0;",
        "r.width, r = 1.0, r;",
    ] {
        let error = rect_error(&format!("fn f(Rect r) {{ {body} }}"));
        assert!(
            matches!(error, SemanticError::DuplicateAssignTarget { .. }),
            "{body}: {error}"
        );
    }
    // The target is named as written, by field name and tuple index.
    let error = error("struct P { (i32, i32) pair; }\nfn f(P p) { p.pair.1, p.pair = 1, (2, 3); }");
    assert_eq!(
        error.to_string(),
        "`p.pair` is assigned more than once in this assignment"
    );
    let error = rect_error("fn f(Rect r) { r.width, r.width = 1.0, 2.0; }");
    assert_eq!(
        error.to_string(),
        "`r.width` is assigned more than once in this assignment"
    );
    // Distinct fields do not overlap.
    check(&format!(
        "{RECT}fn f(Rect r) {{ r.width, r.height = r.height, r.width; }}"
    ))
    .expect("should check");
}

#[test]
fn struct_equality() {
    check(&format!(
        "{RECT}fn f(Rect a, Rect b)->bool {{ return a == b and a != Rect(width = 1.0, height = 2.0); }}"
    ))
    .expect("should check");
    let error = rect_error("fn f(Rect a, Rect b)->bool { return a < b; }");
    assert!(
        error
            .to_string()
            .contains("structs only support `==` and `!=`"),
        "{error}"
    );
    // Same fields, different structs: different types.
    let error = self::error(
        "struct A { i32 x; }\nstruct B { i32 x; }\nfn f(A a, B b)->bool { return a == b; }",
    );
    assert!(
        matches!(error, SemanticError::InvalidOperands { .. }),
        "{error}"
    );
    let error = rect_error("fn f(Rect a)->u8 { return a as u8; }");
    assert!(
        matches!(error, SemanticError::InvalidCast { .. }),
        "{error}"
    );
}

#[test]
fn struct_through_reference_auto_derefs() {
    let dump = thir(&format!(
        "{RECT}fn f(&Rect r)->f64 {{ r.width = 2.0; return r.width; }}"
    ));
    assert!(dump.contains("(*r@b0).0 = 2.0_f64"), "{dump}");
}

#[test]
fn field_on_pointer_is_rejected() {
    let error = rect_error("fn f(*Rect p)->f64 { return p.width; }");
    assert!(
        matches!(&error, SemanticError::NoSuchMember { name, found, .. }
            if name == "width" && found == "*Rect"),
        "{error}"
    );
    assert_eq!(error.to_string(), "`*Rect` has no member `width`");
    check(&format!(
        "{RECT}fn f(*Rect p)->f64 {{ return (*p).width; }}"
    ))
    .expect("should check");
}

#[test]
fn member_errors() {
    let error = rect_error("fn f(Rect r)->f64 { return r.depth; }");
    assert!(
        matches!(&error, SemanticError::NoSuchMember { name, found, .. }
            if name == "depth" && found == "Rect"),
        "{error}"
    );
    let error = rect_error("fn f(i32 x)->i32 { return x.width; }");
    assert!(
        matches!(error, SemanticError::NoSuchMember { .. }),
        "{error}"
    );
    // A tuple index on a struct, and a name on a tuple.
    let error = rect_error("fn f(Rect r)->f64 { return r.0; }");
    assert!(matches!(error, SemanticError::NotATuple { .. }), "{error}");
    let error = rect_error("fn f((i32, i32) t)->i32 { return t.width; }");
    assert!(
        matches!(error, SemanticError::NoSuchMember { .. }),
        "{error}"
    );
}

#[test]
fn construction_errors() {
    let cases: [ErrorCase; 8] = [
        (
            "return Rect(width = 1.0);",
            |e| matches!(e, SemanticError::MissingField { name, .. } if name == "height"),
        ),
        (
            "return Rect();",
            |e| matches!(e, SemanticError::MissingField { name, .. } if name == "width"),
        ),
        (
            "return Rect(width = 1.0, height = 2.0, depth = 3.0);",
            |e| matches!(e, SemanticError::UnknownField { name, .. } if name == "depth"),
        ),
        (
            "return Rect(width = 1.0, width = 2.0, height = 3.0);",
            |e| matches!(e, SemanticError::DuplicateFieldInit { name, .. } if name == "width"),
        ),
        ("return Rect(1.0, 2.0);", |e| {
            matches!(e, SemanticError::PositionalStructArgs { .. })
        }),
        ("return Rect(width = true, height = 2.0);", |e| {
            matches!(e, SemanticError::TypeMismatch { .. })
        }),
        (
            "return g(x = 1);",
            |e| matches!(e, SemanticError::NamedArgsOnFunction { name, .. } if name == "g"),
        ),
        (
            "return Nope(x = 1);",
            |e| matches!(e, SemanticError::UndefinedFunction { name, .. } if name == "Nope"),
        ),
    ];
    for (body, expected) in cases {
        let error = rect_error(&format!("fn g(i32 x)->Rect {{ {body} }}"));
        assert!(expected(&error), "{body}: {error:?}");
    }
    assert_eq!(
        rect_error("fn f()->Rect { return Rect(1.0, 2.0); }").to_string(),
        "`Rect` is a struct: name its fields, as in `Rect(field = value)`"
    );
}

#[test]
fn struct_declaration_errors() {
    let cases: [ErrorCase; 9] = [
        (
            "struct A {}\nstruct A {}",
            |e| matches!(e, SemanticError::DuplicateStruct { name, .. } if name == "A"),
        ),
        (
            "struct A {}\nfn A() {}",
            |e| matches!(e, SemanticError::NameConflict { name, .. } if name == "A"),
        ),
        (
            "fn A() {}\nstruct A {}",
            |e| matches!(e, SemanticError::NameConflict { name, .. } if name == "A"),
        ),
        (
            "struct i32 {}",
            |e| matches!(e, SemanticError::BuiltinTypeName { name, .. } if name == "i32"),
        ),
        (
            "struct bool {}",
            |e| matches!(e, SemanticError::BuiltinTypeName { name, .. } if name == "bool"),
        ),
        (
            "struct A { i32 x; bool x; }",
            |e| matches!(e, SemanticError::DuplicateMember { name, .. } if name == "x"),
        ),
        (
            "struct A { &i32 r; }",
            |e| matches!(e, SemanticError::ReferenceField { name, .. } if name == "r"),
        ),
        (
            "struct A { (i32, &bool) t; }",
            |e| matches!(e, SemanticError::ReferenceField { name, .. } if name == "t"),
        ),
        (
            "struct A { Missing m; }",
            |e| matches!(e, SemanticError::UnknownType { name, .. } if name == "Missing"),
        ),
    ];
    for (src, expected) in cases {
        let error = error(src);
        assert!(expected(&error), "{src}: {error:?}");
    }
}

#[test]
fn unknown_type_is_a_semantic_error() {
    let src = "fn f() { let foo x = 1; }";
    let error = self::error(src);
    assert_eq!(error.to_string(), "unknown type `foo`");
    let span = error.span().expect("span");
    assert_eq!(&src[span.start..span.end], "foo");
    for src in [
        "fn f(Missing m) {}",
        "fn f() -> *Missing { return @0x0; }",
        "fn f(i32 x) -> i32 { return x as Missing; }",
        "fn f((i32, Missing) t) {}",
        "fn f() { let i32 a, Missing b = 1, 2; }",
    ] {
        let error = self::error(src);
        assert!(
            matches!(&error, SemanticError::UnknownType { name, .. } if name == "Missing"),
            "{src}: {error:?}"
        );
    }
}

#[test]
fn mutually_recursive_structs_rejected() {
    for src in [
        "struct A { A a; }",
        "struct A { B b; }\nstruct B { A a; }",
        "struct A { (i32, B) t; }\nstruct B { i32 x; A a; }",
    ] {
        let error = self::error(src);
        assert!(
            matches!(error, SemanticError::RecursiveStruct { .. }),
            "{src}: {error:?}"
        );
    }
    assert_eq!(
        error("struct A { A a; }").to_string(),
        "struct `A` contains itself; use a pointer `*A` to refer to another `A`"
    );
}

#[test]
fn struct_via_pointer_may_be_recursive() {
    check(
        "struct Node { i32 value; *Node next; }\n\
         struct A { *B b; }\nstruct B { *A a; (i32, *B) t; }\n\
         fn f(*Node n)->i32 { return (*(*n).next).value; }",
    )
    .expect("should check");
}

#[test]
fn structs_may_be_declared_in_any_order() {
    check(
        "fn f(Outer o)->i32 { return o.inner.x; }\n\
         struct Outer { Inner inner; }\n\
         struct Inner { i32 x; }",
    )
    .expect("should check");
}

#[test]
fn local_named_like_struct_wins() {
    // `Rect` here is the parameter, a tuple, not the struct.
    let dump = thir(&format!(
        "{RECT}fn f((i32, i32) Rect)->i32 {{ return Rect.0; }}"
    ));
    assert!(dump.contains("return Rect@b0.0"), "{dump}");
}

#[test]
fn identifiers_starting_with_i_or_u_name_structs() {
    check(
        "struct user { i32 id; }\nstruct item { user owner; }\nstruct u { i32 index; }\n\
         struct index { u inner; }\n\
         fn f(item item, index i, u8 u)->i32 { let user user = item.owner; return user.id + i.inner.index; }",
    )
    .expect("should check");
    // Built-in integer names are still `i` or `u` followed by digits only.
    assert_eq!("i32".parse::<Type>().ok(), Some(Type::Int(32)));
    assert!("i3x".parse::<Type>().is_err() && "u".parse::<Type>().is_err());
}

#[test]
fn zero_sized_fields_and_empty_struct_equality() {
    check(
        "struct Empty {}\nstruct Unit { () nothing; u0 zero; Empty e; }\n\
         fn f(Unit a, Empty x, Empty y)->bool {\n\
             let Unit b = Unit(nothing = (), zero = 0, e = Empty());\n\
             return a == b and x == y;\n\
         }",
    )
    .expect("should check");
}

// ── Methods ───────────────────────────────────────────────────────────────────

const RECT_IMPL: &str = "struct Rect { f64 width; f64 height; }\n\
    impl Rect {\n\
        fn scaled(self, f64 k) -> Rect { return Rect(width = self.width * k, height = self.height * k); }\n\
        fn scale(&self, f64 k) { self.width *= k; self.height *= k; }\n\
        fn unit() -> Rect { return Rect(width = 1.0, height = 1.0); }\n\
    }\n";

fn method_error(src: &str) -> SemanticError {
    error(&format!("{RECT_IMPL}{src}"))
}

#[test]
fn method_desugars_to_call_with_receiver() {
    let dump = thir(&format!(
        "{RECT_IMPL}fn f(Rect r, &Rect s) -> Rect {{ r.scale(2.0); s.scale(3.0); return r.scaled(4.0); }}"
    ));
    // The methods are functions named after their struct.
    assert!(
        dump.contains("fn Rect.scaled(self@b0: Rect, k@b1: f64) -> Rect"),
        "{dump}"
    );
    assert!(
        dump.contains("fn Rect.scale(self@b0: &Rect, k@b1: f64)"),
        "{dump}"
    );
    assert!(dump.contains("(*self@b0).0"), "{dump}");
    // `&self` takes the address of the receiver; `self` takes its value.
    assert!(dump.contains("Rect.scale(&r@b0, 2.0_f64)"), "{dump}");
    assert!(dump.contains("Rect.scale(&(*s@b1), 3.0_f64)"), "{dump}");
    assert!(dump.contains("return Rect.scaled(r@b0, 4.0_f64)"), "{dump}");
}

#[test]
fn associated_function_call() {
    let dump = thir(&format!(
        "{RECT_IMPL}fn f() -> f64 {{ return Rect.unit().scaled(2.0).width; }}"
    ));
    assert!(
        dump.contains("return Rect.scaled(Rect.unit(), 2.0_f64).0"),
        "{dump}"
    );
    // A `&self` method works on a value that is no place.
    check(&format!("{RECT_IMPL}fn f() {{ Rect.unit().scale(2.0); }}")).expect("should check");
}

#[test]
fn methods_and_free_functions_have_separate_names() {
    check(&format!(
        "{RECT_IMPL}fn scale(f64 k) -> f64 {{ return k; }}\n\
         fn f(Rect r) -> f64 {{ r.scale(scale(2.0)); return r.width; }}"
    ))
    .expect("should check");
    // A method is not in scope as a function, nor a function as a method.
    let error = method_error("fn f() { scaled(2.0); }");
    assert!(
        matches!(error, SemanticError::UndefinedFunction { .. }),
        "{error}"
    );
}

#[test]
fn several_impl_blocks_share_one_namespace() {
    check(
        "impl A { fn one(self) -> i32 { return self.two(); } }\n\
         struct A { i32 x; }\n\
         impl A { fn two(self) -> i32 { return self.x; } }",
    )
    .expect("should check");
    let cases: [ErrorCase; 3] = [
        (
            "struct A {}\nimpl A { fn f() {} }\nimpl A { fn f(self) {} }",
            |e| matches!(e, SemanticError::DuplicateMember { name, .. } if name == "f"),
        ),
        (
            "struct A { i32 x; }\nimpl A { fn x(self) {} }",
            |e| matches!(e, SemanticError::DuplicateMember { name, .. } if name == "x"),
        ),
        (
            "impl Missing { fn f() {} }",
            |e| matches!(e, SemanticError::UnknownImplTarget { name, .. } if name == "Missing"),
        ),
    ];
    for (src, expected) in cases {
        let error = error(src);
        assert!(expected(&error), "{src}: {error:?}");
    }
    let error = error("impl i32 { fn f() {} }");
    assert!(
        matches!(error, SemanticError::UnknownImplTarget { .. }),
        "{error}"
    );
}

#[test]
fn method_call_errors() {
    let cases: [ErrorCase; 11] = [
        (
            "fn f() { Rect.scale(2.0); }",
            |e| matches!(e, SemanticError::MethodNeedsReceiver { name, .. } if name == "scale"),
        ),
        (
            "fn f(Rect r) { r.unit(); }",
            |e| matches!(e, SemanticError::NotAMethod { name, .. } if name == "unit"),
        ),
        (
            "fn f(Rect r) { r.width(); }",
            |e| matches!(e, SemanticError::NotAMethod { name, .. } if name == "width"),
        ),
        (
            "fn f(Rect r) { r.nope(); }",
            |e| matches!(e, SemanticError::NoSuchMember { name, .. } if name == "nope"),
        ),
        (
            "fn f() { Rect.nope(); }",
            |e| matches!(e, SemanticError::NoSuchMember { name, .. } if name == "nope"),
        ),
        (
            "fn f(i32 x) { x.scale(2.0); }",
            |e| matches!(e, SemanticError::NoSuchMember { name, found, .. } if name == "scale" && found == "i32"),
        ),
        (
            "fn f(*Rect p) { p.scale(2.0); }",
            |e| matches!(e, SemanticError::NoSuchMember { found, .. } if found == "*Rect"),
        ),
        ("fn f(Rect r) { r.scale(); }", |e| {
            matches!(
                e,
                SemanticError::ArgumentCountMismatch {
                    expected: 1,
                    got: 0,
                    ..
                }
            )
        }),
        ("fn f(Rect r) { r.scale(true); }", |e| {
            matches!(e, SemanticError::TypeMismatch { .. })
        }),
        (
            "fn f(Rect r) -> f64 { return r.scale; }",
            |e| matches!(e, SemanticError::MethodNotCalled { name, .. } if name == "scale"),
        ),
        (
            "fn f() { Missing.unit(); }",
            |e| matches!(e, SemanticError::UndefinedVariable { name, .. } if name == "Missing"),
        ),
    ];
    for (body, expected) in cases {
        let error = method_error(body);
        assert!(expected(&error), "{body}: {error:?}");
    }
    assert_eq!(
        method_error("fn f() { Rect.scale(2.0); }").to_string(),
        "`scale` is used on a value of type `Rect`, not on `Rect` itself"
    );
    check(&format!("{RECT_IMPL}fn f(*Rect p) {{ (*p).scale(2.0); }}")).expect("should check");
}

#[test]
fn self_outside_a_method_is_rejected() {
    for src in [
        "fn f() -> i32 { return self.x; }",
        "struct A { i32 x; }\nimpl A { fn f() -> i32 { return self.x; } }",
        "fn f() { self.go(); }",
    ] {
        let error = error(src);
        assert!(
            matches!(error, SemanticError::SelfOutsideMethod { .. }),
            "{src}: {error:?}"
        );
    }
    assert_eq!(
        error("fn f() { self.go(); }").to_string(),
        "`self` is only available in a method"
    );
}

#[test]
fn local_named_like_struct_hides_associated_functions() {
    // `Rect` is the parameter here, so `Rect.unit()` is a method call on it.
    let error = method_error("fn f(Rect Rect) { Rect.unit(); }");
    assert!(matches!(error, SemanticError::NotAMethod { .. }), "{error}");
}

#[test]
fn reserved_struct_name() {
    let error = error("struct _xe {}");
    assert!(
        matches!(error, SemanticError::ReservedName { .. }),
        "{error}"
    );
}

// ── Properties ────────────────────────────────────────────────────────────────

const SHAPE: &str = "struct Size { f64 w; f64 h; }\n\
    struct Shape { Size size; }\n\
    impl Shape {\n\
        f64 area { return self.size.w * self.size.h; }\n\
        Size doubled { return Size(w = self.size.w * 2.0, h = self.size.h * 2.0); }\n\
        f64 twice { return self.area * 2.0; }\n\
        fn unit() -> Shape { return Shape(size = Size(w = 1.0, h = 1.0)); }\n\
    }\n";

fn shape_error(src: &str) -> SemanticError {
    error(&format!("{SHAPE}{src}"))
}

#[test]
fn property_read_desugars_to_getter_call() {
    let dump = thir(&format!(
        "{SHAPE}fn f(Shape s, &Shape r) -> f64 {{ return s.area + r.area + Shape.unit().area; }}"
    ));
    // A getter is a function taking a reference to the value.
    assert!(
        dump.contains("fn Shape.area.get(self@b0: &Shape) -> f64"),
        "{dump}"
    );
    assert!(dump.contains("Shape.area.get(&s@b0)"), "{dump}");
    assert!(dump.contains("Shape.area.get(&(*r@b1))"), "{dump}");
    assert!(dump.contains("Shape.area.get(&Shape.unit())"), "{dump}");
    // A property reads other properties, of `self` and of its results.
    assert!(
        dump.contains("Mul(Shape.area.get(&(*self@b0)), 2.0_f64)"),
        "{dump}"
    );
    check(&format!(
        "{SHAPE}fn f(Shape s) -> f64 {{ return s.doubled.w + s.twice; }}"
    ))
    .expect("should check");
}

#[test]
fn property_is_not_assignable() {
    for body in [
        "s.area = 1.0;",
        "s.area += 1.0;",
        "s.area++;",
        "let *f64 p = @s.area;",
        "s.area, s.size.w = 1.0, 2.0;",
    ] {
        let error = shape_error(&format!("fn f(Shape s) {{ {body} }}"));
        assert!(
            matches!(&error, SemanticError::PropertyNotAssignable { name, .. } if name == "area"),
            "{body}: {error:?}"
        );
    }
    // Nor is a field of the value a property computes: it names the property.
    let error = shape_error("fn f(Shape s) { s.doubled.w = 1.0; }");
    assert!(
        matches!(&error, SemanticError::PropertyNotAssignable { name, .. } if name == "doubled"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`doubled` is a computed property of `Shape`: it cannot be assigned or have its address taken"
    );
    // A stored field next to it still is.
    check(&format!("{SHAPE}fn f(Shape s) {{ s.size.w = s.area; }}")).expect("should check");
}

#[test]
fn property_is_read_without_parentheses() {
    let error = shape_error("fn f(Shape s) -> f64 { return s.area(); }");
    assert!(
        matches!(&error, SemanticError::PropertyCalled { name, .. } if name == "area"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "`area` is a property of `Shape`: read it without parentheses, as `.area`"
    );
    let error = shape_error("fn f() -> f64 { return Shape.area; }");
    assert!(
        matches!(error, SemanticError::MethodNeedsReceiver { .. }),
        "{error:?}"
    );
    let error = shape_error("fn f(*Shape p) -> f64 { return p.area; }");
    assert!(
        matches!(error, SemanticError::NoSuchMember { .. }),
        "{error:?}"
    );
}

#[test]
fn property_shares_the_member_namespace() {
    let cases: [ErrorCase; 4] = [
        (
            "struct A { i32 x; }\nimpl A { i32 x { return 1; } }",
            |e| matches!(e, SemanticError::DuplicateMember { name, .. } if name == "x"),
        ),
        (
            "struct A {}\nimpl A { i32 x { return 1; } fn x(self) {} }",
            |e| matches!(e, SemanticError::DuplicateMember { name, .. } if name == "x"),
        ),
        (
            "struct A {}\nimpl A { Missing x { return 1; } }",
            |e| matches!(e, SemanticError::UnknownType { name, .. } if name == "Missing"),
        ),
        ("struct A {}\nimpl A { i32 x { return true; } }", |e| {
            matches!(
                e,
                SemanticError::TypeMismatch { .. } | SemanticError::ReturnTypeMismatch { .. }
            )
        }),
    ];
    for (src, expected) in cases {
        let error = error(src);
        assert!(expected(&error), "{src}: {error:?}");
    }
}

#[test]
fn overlapping_targets_are_found_through_a_reference() {
    // A `&T` variable denotes what it refers to, so its elements have
    // known paths, as in a `&self` method.
    for src in [
        "struct A { i32 x; i32 w; }\nimpl A { fn f(&self) { self.x, self.x = 1, 2; } }",
        "struct A { i32 x; }\nstruct B { A a; }\n\
         impl B { fn f(&self) { self.a.x, self.a = 1, A(x = 5); } }",
        "fn f(&(i32, i32) t) { t.0, t.0 = 1, 2; }",
        "fn f(&(i32, i32) t) { t, t.1 = (1, 2), 3; }",
    ] {
        let error = error(src);
        assert!(
            matches!(error, SemanticError::DuplicateAssignTarget { .. }),
            "{src}: {error:?}"
        );
    }
    assert_eq!(
        error("struct A { i32 x; }\nimpl A { fn f(&self) { self.x, self.x = 1, 2; } }").to_string(),
        "`self.x` is assigned more than once in this assignment"
    );
    // Distinct fields, and places behind an explicit pointer, still pass.
    check(
        "struct A { i32 x; i32 w; }\nimpl A { fn f(&self) { self.x, self.w = self.w, self.x; } }\n\
         fn g(*A p, *A q) { (*p).x, (*q).x = 1, 2; }",
    )
    .expect("should check");
}

#[test]
fn backend_prefixes_cannot_name_structs() {
    for name in ["_xe", "llvm"] {
        let error = error(&format!("struct {name} {{}}"));
        assert!(
            matches!(&error, SemanticError::ReservedName { name: found, .. } if found == name),
            "{name}: {error:?}"
        );
    }
    // Only as a struct name: members are prefixed with it.
    check("fn llvm() {}\nstruct A { i32 llvm; }\nimpl A { fn trap() {} }").expect("should check");
}

#[test]
fn shared_let_names_have_the_first_type() {
    let dump = thir("fn f()->i64 { let i64 a, b = 1, 2; return a + b; }");
    assert!(dump.contains("let a@b0: i64 = 1_i64"), "{dump}");
    assert!(dump.contains("let b@b1: i64 = 2_i64"), "{dump}");
    // A value of another type is a mismatch, as with a spelled-out type.
    let error = error("fn f() { let i32 a, b = 1, true; }");
    assert!(
        matches!(error, SemanticError::TypeMismatch { .. }),
        "{error}"
    );
}

// ── `repeat` and `for` ────────────────────────────────────────────────────────

#[test]
fn repeat_count_rules() {
    assert!(check("fn f(i32 n) { repeat n { } repeat 3 { } }").is_ok());
    assert!(matches!(
        error("fn f() { repeat true { } }"),
        SemanticError::NonIntegerRepeatCount { .. }
    ));
    assert!(matches!(
        error("fn f() { repeat -1 { } }"),
        SemanticError::NegativeRepeatCount { .. }
    ));
}

/// Whether `src` fails to type-check.
fn error_is_some(src: &str) -> bool {
    check(src).is_err()
}

#[test]
fn for_range_types() {
    // The loop variable takes the type of the typed bound.
    assert!(check("fn f(u8 n) { for i in 0 until n { let u8 a = i; } }").is_ok());
    assert!(check("fn f(u8 n) { for i in n to 9 { let u8 a = i; } }").is_ok());
    assert!(check("fn f() { for i16 i in 0 to 9 { let i16 a = i; } }").is_ok());
    assert!(check("fn f() { for u8 _ in 0 to 9 { } }").is_ok());
    assert!(matches!(
        error("fn f() { for i in 0 to 9 { } }"),
        SemanticError::RangeTypeUnknown { .. }
    ));
    assert!(matches!(
        error("fn f(bool b) { for i in b to b { } }"),
        SemanticError::NonIntegerRange { .. }
    ));
    assert!(matches!(
        error("fn f() { for f64 i in 0 to 9 { } }"),
        SemanticError::NonIntegerRange { .. }
    ));
    assert!(matches!(
        error("fn f() { for u8 i in 0 to 9 by 0 { } }"),
        SemanticError::ZeroRangeStep { .. }
    ));
    // Bounds of two different types.
    assert!(error_is_some("fn f(u8 a, i32 b) { for i in a to b { } }"));
    // A literal bound that does not fit the loop type.
    assert!(error_is_some("fn f() { for u8 i in 0 to 300 { } }"));
}

#[test]
fn for_loop_names_are_read_only() {
    for stmt in ["i = 1;", "i += 1;", "i++;", "i, a = 1, 2;"] {
        let src = format!("fn f() {{ let u8 a = 0; for u8 i in 0 to 9 {{ {stmt} }} }}");
        assert!(
            matches!(error(&src), SemanticError::AssignToLoopName { .. }),
            "{stmt}"
        );
    }
}

#[test]
fn for_scoping() {
    // Not visible after the loop, nor in its own bounds.
    assert!(error_is_some(
        "fn f() { for u8 i in 0 to 9 { } let u8 a = i; }"
    ));
    assert!(error_is_some("fn f() { for u8 i in 0 to i { } }"));
    assert!(check("fn f(u8 i) { for j in 0 until i { for j in 0 until j { } } }").is_ok());
    // A bound may read an outer variable the loop variable then shadows.
    assert!(check("fn f(u8 i) { for i in 0 until i { } }").is_ok());
}

#[test]
fn for_step_must_fit() {
    assert!(matches!(
        error("fn f() { for u8 i in 0 to 9 by 300 { } }"),
        SemanticError::LiteralOutOfRange { .. }
    ));
    assert!(check("fn f(i32 a, i32 b) { for i in a to b by -7 { } }").is_ok());
}

#[test]
fn for_array_rules() {
    assert!(check("fn f([4]u8 a) { for x in a { let u8 b = x; } }").is_ok());
    assert!(check("fn f([4]u8 a) { for u8 x in a { } for _ in a { } }").is_ok());
    assert!(check("fn f([2][3]u8 g) { for row in g { for x in row { let u8 b = x; } } }").is_ok());
    assert!(check("fn f(&[4]u8 a) { for x in a { let u8 b = x; } }").is_ok());
    assert!(matches!(
        error("fn f([4]u8 a) { for i32 x in a { } }"),
        SemanticError::LoopVariableType { .. }
    ));
    assert!(matches!(
        error("fn f(u8 a) { for x in a { } }"),
        SemanticError::NotIterable { .. }
    ));
    assert!(matches!(
        error("fn f([4]u8 a) { for x in a { x = 1; } }"),
        SemanticError::AssignToLoopName { .. }
    ));
    assert!(matches!(
        error("fn f([2][3]u8 g) { for row in g { row[0] = 1; } }"),
        SemanticError::AssignToLoopName { .. }
    ));
    // The array itself stays assignable while it is walked.
    assert!(check("fn f([4]u8 a) { for x in a { a[0] = x; } }").is_ok());
}

#[test]
fn for_clause_rules() {
    assert!(check("fn f([4]u8 a) { for x in a with i { let usize j = i; } }").is_ok());
    assert!(
        check("fn f([4]u8 a) { for x in a with u8 i where x > i with k { let usize j = k; } }")
            .is_ok()
    );
    assert!(check("fn f() { for u8 x in 0 to 9 where x % 2 == 0 with n { } }").is_ok());
    assert!(check("fn f([0]u8 a) { for x in a with i where i > 0 { } }").is_ok());
    // A `where` sees the names before it, not those after.
    assert!(matches!(
        error("fn f([4]u8 a) { for x in a where k > 0 with k { } }"),
        SemanticError::UndefinedVariable { .. }
    ));
    assert!(matches!(
        error("fn f([4]u8 a) { for x in a where x { } }"),
        SemanticError::ConditionNotBool { .. }
    ));
    assert!(matches!(
        error("fn f([4]u8 a) { for x in a with bool i { } }"),
        SemanticError::NonIntegerCounter { .. }
    ));
    assert!(matches!(
        error("fn f([4]u8 a) { for x in a with i { i += 1; } }"),
        SemanticError::AssignToLoopName { .. }
    ));
    // Counters are not visible after the loop.
    assert!(error_is_some(
        "fn f([4]u8 a) { for x in a with i { } let usize j = i; }"
    ));
}
