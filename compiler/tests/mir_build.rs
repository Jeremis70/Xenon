//! Tests for MIR construction: THIR to built MIR.

use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::index::Idx;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::ids::DefId;
use xenonc::middle::mir::pretty::mir_program_to_string;
use xenonc::middle::mir::{MirPhase, MirProgram, TerminatorKind, build_mir, verify_program};
use xenonc::middle::target::TargetSpec;
use xenonc::middle::typecheck::check_program;

const TARGET: TargetSpec = TargetSpec::new(64);

/// Runs the front end and type checking on `src`, then builds and verifies
/// its MIR.
fn build(src: &str) -> MirProgram {
    let tokens = lex(src).expect("lexing should succeed");
    let program = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    let program = fold_constants(program).expect("folding should succeed");
    let thir = check_program(&program, &TARGET).expect("type checking should succeed");
    let mir = build_mir(&thir).expect("MIR construction should succeed");
    if let Err(errors) = verify_program(&mir, &TARGET) {
        panic!(
            "built MIR fails verification:\n{errors}\n{}",
            mir_program_to_string(&mir)
        );
    }
    mir
}

/// The MIR dump of `src`.
fn mir(src: &str) -> String {
    mir_program_to_string(&build(src))
}

/// Asserts that `needles` appear in `dump` in this order.
#[track_caller]
fn assert_in_order(dump: &str, needles: &[&str]) {
    let mut rest = dump;
    for needle in needles {
        match rest.find(needle) {
            Some(at) => rest = &rest[at + needle.len()..],
            None => panic!("`{needle}` not found (in order) in:\n{dump}"),
        }
    }
}

// ── Whole programs ────────────────────────────────────────────────────────────

#[test]
fn example_program_builds_and_verifies() {
    let src = include_str!("../../tests/main.xe");
    let program = build(src);
    assert_eq!(program.entry(), Some(DefId::new(0)));
    assert_eq!(program.bodies().len(), 2);
    assert!(program.bodies().all(|body| body.phase() == MirPhase::Built));
}

#[test]
fn def_ids_and_signatures_match_thir() {
    let program = build("fn a(i32 x, bool y)->u8 { return 1; } fn b()->i64 { return 2; }");
    let a = program.decl(DefId::new(0)).expect("`a` is declared");
    let b = program.decl(DefId::new(1)).expect("`b` is declared");
    assert_eq!(a.name, "a");
    assert_eq!(a.sig.inputs.len(), 2);
    assert_eq!(b.name, "b");
    assert_eq!(program.entry(), None);
}

#[test]
fn every_construct_builds_verified_mir() {
    // `build` verifies each program.
    for src in [
        "fn f(i32 a)->i32 { return -a; }",
        "fn f(u8 a)->u8 { return ~a >> 1 << 2; }",
        "fn f(bool a, bool b)->bool { return !(a ^^ b) || a && b; }",
        "fn f(f64 a)->f64 { let f64 b = a / 2.0; b -= 1.0; return b * a; }",
        "fn f(i32 a, i64 b)->bool { return a < b; }",
        "fn f(i32 a, u32 b)->bool { return a != b; }",
        "fn f(*i32 p)->i32 { *p = *p % 3; return *p; }",
        "fn f(&i32 r)->i32 { r *= 2; r++; return r; }",
        "fn f()->i32 { let i32 x = 1; let &i32 r = @x; let *i32 p = @x; return r + *p; }",
        "fn f(i32 a)->i32 { return 1 if a > 0 else 2 if a < 0 else 0; }",
        "fn f(i32 a)->i32 { if a > 0 { a = 1; } else if a < 0 { a = 2; } return a; }",
        "fn f(i32 a)->i32 { while a > 0 { a--; } return a; }",
        "fn f(i32 a)->i32 { until a == 0 { a--; } return a; }",
        "fn f(i32 a)->i32 { do { a--; } while a > 0 return a; }",
        "fn f(i32 a)->i32 { do { a--; } until a < 0 return a; }",
        "fn f(i32 a)->i32 { let i32 x = loop { if a > 9 { break a; } a++; }; return x; }",
        "fn f(i32 a)->i32 { return loop { break loop { break a; }; }; }",
        "fn f(i32 a)->i32 { loop { if a > 3 { break; } a++; continue; } return a; }",
        "fn f()->i32 r { r = 5; return r; }",
        "fn f(i32 n)->i32 { return 1 if n < 2 else n * f(n - 1); }",
        "fn f()->usize { let usize x = 3; let isize y = -1; return x; }",
    ] {
        build(src);
    }
}

#[test]
fn small_program_snapshot() {
    let dump = mir("#[entry] fn main()->i32 { let i32 x = 0; while x < 10 { x++; } return x; }");
    let expected = "\
// MIR for `main` (phase: built)
fn main() -> i32 {
    let i32 _0;
    let i32 _1;
    let i64 _2;
    let bool _3;
    debug x => _1;

    bb0: {
        StorageLive(_1);
        _1 = const 0_i32;
        _2 = const 0_i64;
        goto -> bb1;
    }

    bb1: {
        _3 = Lt(copy _1, const 10_i32);
        switchInt(copy _3) -> [0: bb3, otherwise: bb2];
    }

    bb2: {
        _1 = Add(copy _1, const 1_i32);
        goto -> bb1;
    }

    bb3: {
        _0 = copy _1;
        StorageDead(_1);
        return;
    }
}
";
    assert_eq!(dump, expected);
}

// ── Expressions ───────────────────────────────────────────────────────────────

#[test]
fn logical_operators_lower_to_eager_bitwise_operators() {
    let dump = mir("fn f(bool a, bool b)->bool { return a && b || a ^^ b; }");
    assert!(dump.contains("BitAnd(copy _1, copy _2)"), "{dump}");
    assert!(dump.contains("BitXor(copy _1, copy _2)"), "{dump}");
    assert!(dump.contains("_0 = BitOr("), "{dump}");
}

#[test]
fn calls_write_their_destination_and_continue() {
    let dump = mir("fn g(i32 x)->i32 { return x; } fn f()->i32 { return g(1) + 2; }");
    assert_in_order(
        &dump,
        &[
            "_1 = g(const 1_i32) -> bb1;",
            "bb1: {",
            "_0 = Add(copy _1, const 2_i32);",
        ],
    );
}

#[test]
fn conditional_expression_writes_both_branches() {
    let dump = mir("fn f(bool c)->i32 { return 1 if c else 2; }");
    assert_in_order(
        &dump,
        &[
            "switchInt(copy _1) -> [0: bb2, otherwise: bb1];",
            "bb1: {",
            "_0 = const 1_i32;",
            "goto -> bb3;",
            "bb2: {",
            "_0 = const 2_i32;",
            "goto -> bb3;",
            "bb3: {",
            "return;",
        ],
    );
}

#[test]
fn comparison_widening_is_an_explicit_cast() {
    let dump = mir("fn f(i32 a, i64 b)->bool { return a < b; }");
    assert_in_order(
        &dump,
        &["= copy _1 as i64 (IntToInt);", "_0 = Lt(copy _3, copy _2);"],
    );
}

#[test]
fn references_and_pointers_lower_to_deref_projections() {
    let dump = mir("fn f(&i32 r, *i32 p)->i32 { r = *p; return r; }");
    assert!(dump.contains("(*_1) = copy (*_2);"), "{dump}");
    assert!(dump.contains("_0 = copy (*_1);"), "{dump}");
}

#[test]
fn address_of_keeps_its_flavor() {
    let dump = mir("fn f()->i32 { let i32 x = 1; let *i32 p = @x; let &i32 r = @x; return x; }");
    assert!(dump.contains("_2 = @raw _1;"), "{dump}");
    assert!(dump.contains("_3 = @ref _1;"), "{dump}");
}

// ── Evaluation order ──────────────────────────────────────────────────────────

#[test]
fn place_operand_is_read_before_a_later_call() {
    // `h` may write through `p`: the left operand must be read first.
    let dump = mir("fn h(*i32 p)->i32 { return *p + h(p); }");
    assert_in_order(
        &dump,
        &[
            "_2 = copy (*_1);",
            "_3 = h(copy _1) -> bb1;",
            "_0 = Add(copy _2, copy _3);",
        ],
    );
}

#[test]
fn place_operand_without_later_effects_is_not_copied() {
    let dump = mir("fn f(*i32 p, i32 a)->i32 { return *p + a; }");
    assert!(dump.contains("_0 = Add(copy (*_1), copy _2);"), "{dump}");
}

#[test]
fn assignment_target_pointer_is_fixed_before_the_value() {
    let dump = mir("fn h(*i32 p)->i32 { *p = h(p); return 0; }");
    assert_in_order(
        &dump,
        &[
            "_2 = copy _1;",
            "_3 = h(copy _1) -> bb1;",
            "(*_2) = copy _3;",
        ],
    );
}

#[test]
fn compound_assignment_reads_the_old_value_after_the_operand() {
    let dump = mir("fn h(*i32 p)->i32 { *p += h(p); return 0; }");
    assert_in_order(
        &dump,
        &[
            "_2 = copy _1;",
            "_3 = h(copy _1) -> bb1;",
            "(*_2) = Add(copy (*_2), copy _3);",
        ],
    );
}

#[test]
fn call_arguments_are_evaluated_left_to_right() {
    let dump =
        mir("fn g(i32 a, i32 b)->i32 { return a; } fn f(i32 x)->i32 { return g(x, g(x, 1)); }");
    assert_in_order(
        &dump,
        &[
            "_2 = copy _1;",
            "_3 = g(copy _1, const 1_i32) -> bb1;",
            "_0 = g(copy _2, copy _3) -> bb2;",
        ],
    );
}

// ── Control flow ──────────────────────────────────────────────────────────────

#[test]
fn if_without_else_joins_without_an_extra_block() {
    let dump = mir("fn f(i32 a)->i32 { if a > 0 { a = 0; } return a; }");
    assert_in_order(
        &dump,
        &[
            "switchInt(copy _2) -> [0: bb2, otherwise: bb1];",
            "bb1: {",
            "goto -> bb3;",
            "bb2: {",
            "goto -> bb3;",
            "bb3: {",
            "_0 = copy _1;",
        ],
    );
}

#[test]
fn if_with_diverging_branches_diverges() {
    let dump = mir("fn f(bool c)->i32 { if c { return 1; } else { return 2; } }");
    assert!(!dump.contains("end_of_body"), "{dump}");
    assert_eq!(dump.matches("return;").count(), 2, "{dump}");
}

#[test]
fn statements_after_return_are_not_lowered() {
    let dump = mir("fn f(i32 a)->i32 { return 1; a = 2; return a; }");
    assert_eq!(dump.matches("return;").count(), 1, "{dump}");
    assert!(!dump.contains("const 2_i32"), "{dump}");
}

#[test]
fn falling_off_the_end_is_end_of_body() {
    let dump = mir("fn f(i32 a)->i32 { a = 1; }");
    assert!(dump.contains("end_of_body;"), "{dump}");
}

#[test]
fn infinite_loop_without_break_diverges() {
    let dump = mir("fn f()->i32 { loop { } }");
    assert!(!dump.contains("end_of_body"), "{dump}");
    assert!(dump.contains("goto -> bb1;"), "{dump}");
}

#[test]
fn loop_value_comes_from_break() {
    let dump = mir("fn f(i32 a)->i32 { return loop { if a > 3 { break a; } a++; }; }");
    assert_in_order(
        &dump,
        &[
            "_0 = const 0_i32;",
            "goto -> bb1;",
            "_0 = copy _1;",
            "goto -> bb4;",
        ],
    );
}

#[test]
fn until_loop_swaps_the_switch_targets() {
    let dump = mir("fn f(i32 a)->i32 { until a == 0 { a--; } return a; }");
    assert!(
        dump.contains("switchInt(copy _3) -> [0: bb2, otherwise: bb3];"),
        "{dump}"
    );
}

#[test]
fn do_while_tests_the_condition_after_the_body() {
    let dump = mir("fn f(i32 a)->i32 { do { a--; } while a > 0 return a; }");
    assert_in_order(
        &dump,
        &[
            "bb1: {",
            "_1 = Sub(copy _1, const 1_i32);",
            "goto -> bb2;",
            "bb2: {",
            "Gt(copy _1, const 0_i32);",
            "-> [0: bb3, otherwise: bb1];",
        ],
    );
}

#[test]
fn do_loop_whose_body_always_breaks_has_no_latch() {
    let dump = mir("fn f(i32 a)->i32 { do { break; } while a > 0 return a; }");
    assert!(!dump.contains("Gt("), "{dump}");
}

#[test]
fn break_and_continue_end_storage_of_inner_scopes() {
    let dump = mir(
        "fn f(i32 a)->i32 { while a > 0 { let i32 t = a; if t == 3 { continue; } \
         if t == 5 { break; } a--; } return a; }",
    );
    // `t` dies on the `continue` edge, the `break` edge, and the back edge.
    assert_eq!(dump.matches("StorageDead(_4);").count(), 3, "{dump}");
}

#[test]
fn return_ends_storage_of_every_scope() {
    let dump = mir("fn f()->i32 { let i32 x = 1; if true { let i32 y = 2; return y; } return x; }");
    assert_in_order(
        &dump,
        &[
            "_0 = copy _2;",
            "StorageDead(_2);",
            "StorageDead(_1);",
            "return;",
        ],
    );
}

#[test]
fn named_return_value_is_a_zero_initialized_local() {
    let dump = mir("fn f()->i32 r { r += 2; return r; }");
    assert_in_order(
        &dump,
        &[
            "debug r => _1;",
            "StorageLive(_1);",
            "_1 = const 0_i32;",
            "_1 = Add(copy _1, const 2_i32);",
            "_0 = copy _1;",
            "StorageDead(_1);",
            "return;",
        ],
    );
}

#[test]
fn built_bodies_contain_only_built_terminators() {
    let program = build("fn f(i32 a)->i32 { while a > 0 { a--; } }");
    let body = program.body(DefId::new(0)).expect("`f` has a body");
    let has_end = body
        .basic_blocks()
        .iter()
        .any(|block| matches!(block.terminator.kind, TerminatorKind::EndOfBody));
    assert!(has_end);
}
