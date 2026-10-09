//! Tests for MIR reachability, flow checking, liveness, and runtime checks.

use std::collections::BTreeSet;

use xenonc::index::Idx;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::ids::DefId;
use xenonc::middle::mir::analysis::{
    AnalysisErrorKind, OverflowMode, RuntimeCheckPolicy, analyze_liveness, analyze_program,
    normalize_runtime_checks, reachable_blocks,
};
use xenonc::middle::mir::pretty::mir_program_to_string;
use xenonc::middle::mir::{
    AssertKind, BasicBlock, BodyBuilder, Constant, FnDecl, FnSig, IndirectionKind, Local,
    LocalDecl, MirPhase, MirProgram, Operand, Rvalue, SourceInfo, SwitchTargets, TerminatorKind,
    build_mir,
};
use xenonc::middle::target::TargetSpec;
use xenonc::middle::typecheck::check_program;
use xenonc::source::Span;
use xenonc::types::Type;
use xenonc::{frontend::lexer::lex, frontend::parser::Parser};

const TARGET: TargetSpec = TargetSpec::new(64);
const SI: SourceInfo = SourceInfo::outermost(Span::ZERO);

fn built(src: &str) -> MirProgram {
    let tokens = lex(src).expect("lexing should succeed");
    let ast = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    let ast = fold_constants(ast).expect("constant folding should succeed");
    let thir = check_program(&ast, &TARGET).expect("type checking should succeed");
    build_mir(&thir).expect("MIR construction should succeed")
}

fn checked(src: &str) -> MirProgram {
    let mut program = built(src);
    analyze_program(&mut program, &TARGET).expect("flow analysis should succeed");
    program
}

fn wrapping() -> RuntimeCheckPolicy {
    RuntimeCheckPolicy {
        overflow: OverflowMode::Wrapping,
    }
}

fn checked_overflow() -> RuntimeCheckPolicy {
    RuntimeCheckPolicy {
        overflow: OverflowMode::Checked,
    }
}

fn error(src: &str) -> AnalysisErrorKind {
    let mut program = built(src);
    let errors = analyze_program(&mut program, &TARGET).expect_err("expected flow error");
    errors
        .0
        .into_iter()
        .next()
        .expect("expected at least one analysis error")
        .kind
}

#[test]
fn reachable_fallthrough_is_a_missing_return() {
    assert!(matches!(
        error("fn f()->i32 { let i32 x = 0; }"),
        AnalysisErrorKind::MissingReturn
    ));
}

#[test]
fn constant_switches_prune_impossible_control_flow_edges() {
    let mut program = built("fn f()->i32 { if true { return 1; } }");
    analyze_program(&mut program, &TARGET)
        .expect("the false edge of the constant condition is unreachable");
    assert_eq!(
        program.body(DefId::new(0)).expect("body").phase(),
        MirPhase::Checked
    );
}

#[test]
fn unreachable_fallthrough_does_not_require_a_return() {
    let mut program = built("fn f()->i32 { return 1; }");
    let def_id = DefId::new(0);
    let body = program.body_mut(def_id).expect("function body");
    let unreachable = body
        .basic_blocks_mut()
        .push(xenonc::middle::mir::BasicBlockData {
            statements: Vec::new(),
            terminator: xenonc::middle::mir::Terminator {
                source_info: SI,
                kind: TerminatorKind::EndOfBody,
            },
        });
    assert!(!reachable_blocks(body).contains(&unreachable));
    analyze_program(&mut program, &TARGET).expect("unreachable fallthrough is harmless");
    assert_eq!(
        program.body(def_id).expect("body").phase(),
        MirPhase::Checked
    );
    assert!(matches!(
        program.body(def_id).expect("body").basic_blocks()[unreachable]
            .terminator
            .kind,
        TerminatorKind::Unreachable
    ));
}

#[test]
fn infinite_loop_is_diverging_not_fallthrough() {
    let mut program = built("fn f()->i32 { loop { } }");
    analyze_program(&mut program, &TARGET).expect("infinite loop diverges");
    assert_eq!(
        program.body(DefId::new(0)).expect("body").phase(),
        MirPhase::Checked
    );
}

#[test]
fn a_return_must_initialize_its_return_place() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let builder = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), []);
    let mut builder = builder;
    builder.terminate(BasicBlock::new(0), SI, TerminatorKind::Return);
    program
        .set_body(builder.finish().expect("well-formed body"))
        .expect("declared function");
    assert!(matches!(
        analyze_program(&mut program, &TARGET)
            .expect_err("return is not initialized")
            .0[0]
            .kind,
        AnalysisErrorKind::UninitializedReturn
    ));
}

#[test]
fn uninitialized_local_read_is_rejected() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let mut builder = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), []);
    let local = builder.new_local(LocalDecl::named(Type::Int(32), "x", SI));
    builder.storage_live(BasicBlock::new(0), SI, local);
    builder.push_assign(
        BasicBlock::new(0),
        SI,
        Local::new(0).into(),
        Rvalue::Use(Operand::Copy(local.into())),
    );
    builder.terminate(BasicBlock::new(0), SI, TerminatorKind::Return);
    program
        .set_body(builder.finish().expect("well-formed body"))
        .expect("declared function");
    let errors = analyze_program(&mut program, &TARGET).expect_err("x was never initialized");
    assert!(matches!(
        errors.0[0].kind,
        AnalysisErrorKind::UninitializedLocal { ref name, .. } if name == "x"
    ));
}

#[test]
fn initialization_at_a_join_must_hold_on_every_edge() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![Type::Bool],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let arg = Local::new(1);
    let mut builder = BodyBuilder::new(
        def_id,
        Span::ZERO,
        Type::Int(32),
        [LocalDecl::named(Type::Bool, "c", SI)],
    );
    let value = builder.new_local(LocalDecl::named(Type::Int(32), "x", SI));
    let then_block = builder.new_block();
    let else_block = builder.new_block();
    let join = builder.new_block();
    builder.storage_live(BasicBlock::new(0), SI, value);
    builder.terminate(
        BasicBlock::new(0),
        SI,
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(arg.into()),
            targets: SwitchTargets::bool(then_block, else_block),
        },
    );
    builder.push_assign(
        then_block,
        SI,
        value.into(),
        Rvalue::Use(Operand::constant(Constant::int(1, Type::Int(32)))),
    );
    builder.goto(then_block, SI, join);
    builder.goto(else_block, SI, join);
    builder.push_assign(
        join,
        SI,
        Local::new(0).into(),
        Rvalue::Use(Operand::Copy(value.into())),
    );
    builder.terminate(join, SI, TerminatorKind::Return);
    program
        .set_body(builder.finish().expect("well-formed body"))
        .expect("declared function");
    assert!(matches!(
        analyze_program(&mut program, &TARGET)
            .expect_err("x is not initialized on the else edge")
            .0[0]
            .kind,
        AnalysisErrorKind::UninitializedLocal { .. }
    ));
}

#[test]
fn call_destination_is_initialized_only_on_normal_return() {
    let program = checked(
        "fn callee()->i32 { return 4; } fn caller()->i32 { let i32 x = callee(); return x; }",
    );
    let caller = program.body(DefId::new(1)).expect("caller body");
    assert!(caller.basic_blocks().iter().any(|block| {
        matches!(
            block.terminator.kind,
            TerminatorKind::Call {
                target: Some(_),
                ..
            }
        )
    }));
}

#[test]
fn liveness_reaches_a_fixed_point_through_a_loop() {
    let program = built("fn f(i32 x)->i32 { while x > 0 { x--; } return x; }");
    let body = program.body(DefId::new(0)).expect("body");
    let liveness = analyze_liveness(body);
    let header = BasicBlock::new(1);
    let x = Local::new(1);
    assert!(liveness.is_live_in(header, x));
    assert!(liveness.is_live_out(header, x));
}

#[test]
fn liveness_tracks_places_and_address_takes() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![],
            output: Type::Int(32),
        },
        span: Span::ZERO,
    });
    let mut builder = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), []);
    let local = builder.new_local(LocalDecl::named(Type::Int(32), "x", SI));
    let pointer = builder.new_temp(Type::Pointer(Box::new(Type::Int(32))), Span::ZERO);
    builder.storage_live(BasicBlock::new(0), SI, local);
    builder.push_assign(
        BasicBlock::new(0),
        SI,
        pointer.into(),
        Rvalue::AddressOf(IndirectionKind::Pointer, local.into()),
    );
    builder.push_assign(
        BasicBlock::new(0),
        SI,
        Local::new(0).into(),
        Rvalue::Use(Operand::constant(Constant::int(0, Type::Int(32)))),
    );
    builder.terminate(BasicBlock::new(0), SI, TerminatorKind::Return);
    program
        .set_body(builder.finish().expect("well-formed body"))
        .expect("declared function");
    let body = program.body(DefId::new(0)).expect("body");
    let liveness = analyze_liveness(body);
    let entry = BasicBlock::new(0);
    assert!(liveness.is_live_in(entry, local));
}

#[test]
fn wrapping_integer_operations_have_no_overflow_assert() {
    let mut program = checked("fn f(i32 a, i32 b)->i32 { return a + b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(!dump.contains("Overflow(Add)"), "{dump}");
    assert_eq!(
        program.body(DefId::new(0)).expect("body").phase(),
        MirPhase::Runtime
    );
}

#[test]
fn checked_add_has_explicit_overflow_guard() {
    let mut program = checked("fn f(i32 a, i32 b)->i32 { return a + b; }");
    normalize_runtime_checks(&mut program, &TARGET, checked_overflow())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(dump.contains("OverflowsAdd"), "{dump}");
    assert!(dump.contains("assert(!copy"), "{dump}");
}

#[test]
fn checked_negation_has_explicit_overflow_guard() {
    let mut program = checked("fn f(i32 a)->i32 { return -a; }");
    normalize_runtime_checks(&mut program, &TARGET, checked_overflow())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(dump.contains("OverflowsSub(const 0_i32"), "{dump}");
    assert!(dump.contains("attempt to subtract with overflow"), "{dump}");
}

#[test]
fn signed_division_checks_zero_and_minimum_overflow() {
    let mut program = checked("fn f(i32 a, i32 b)->i32 { return a / b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(dump.contains("attempt to divide by zero"), "{dump}");
    assert!(dump.contains("attempt to divide with overflow"), "{dump}");
    assert!(dump.contains("Div(copy"), "{dump}");
}

#[test]
fn unsigned_remainder_checks_zero_without_signed_overflow() {
    let mut program = checked("fn f(u32 a, u32 b)->u32 { return a % b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(
        dump.contains("attempt to calculate the remainder"),
        "{dump}"
    );
    assert!(!dump.contains("attempt to divide with overflow"), "{dump}");
}

#[test]
fn signed_remainder_checks_minimum_overflow() {
    let mut program = checked("fn f(i32 a, i32 b)->i32 { return a % b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(
        dump.contains("attempt to calculate remainder with overflow"),
        "{dump}"
    );
}

#[test]
fn shifts_check_negative_and_too_large_amounts_in_a_wide_type() {
    let mut program = checked("fn f(i32 a, i2 b)->i32 { return a << b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(dump.contains("attempt to shift out of range"), "{dump}");
    assert!(dump.contains("as u6 (IntToInt)"), "{dump}");
}

#[test]
fn float_division_does_not_get_integer_guards() {
    let mut program = checked("fn f(f32 a, f32 b)->f32 { return a / b; }");
    normalize_runtime_checks(&mut program, &TARGET, checked_overflow())
        .expect("runtime normalization should work");
    let dump = mir_program_to_string(&program);
    assert!(!dump.contains("DivisionByZero"), "{dump}");
    assert!(!dump.contains("SignedDivisionOverflow"), "{dump}");
}

#[test]
fn runtime_assertions_split_blocks_and_verify() {
    let mut program = checked("fn f(i32 a, i32 b)->i32 { return a / b; }");
    normalize_runtime_checks(&mut program, &TARGET, wrapping())
        .expect("runtime normalization verifies");
    let body = program.body(DefId::new(0)).expect("body");
    assert!(body.basic_blocks().len() >= 3);
    assert!(body.basic_blocks().iter().any(|block| {
        matches!(
            block.terminator.kind,
            TerminatorKind::Assert {
                kind: AssertKind::DivisionByZero,
                ..
            }
        )
    }));
}

#[test]
fn failed_runtime_normalization_leaves_program_unchanged() {
    let mut program = checked("fn f(i32 value, usize amount)->i32 { return value << amount; }");
    let before = mir_program_to_string(&program);
    let error = normalize_runtime_checks(&mut program, &TargetSpec::new(0), wrapping())
        .expect_err("zero-width target cannot lower usize shift counts");
    assert!(error.to_string().contains("pointer width"));
    assert_eq!(mir_program_to_string(&program), before);
    assert!(
        program
            .bodies()
            .all(|body| body.phase() == MirPhase::Checked)
    );
}

#[test]
fn analysis_reports_are_atomic_across_functions() {
    let mut program = built("fn good()->i32 { return 1; } fn bad()->i32 { }");
    assert!(analyze_program(&mut program, &TARGET).is_err());
    assert!(program.bodies().all(|body| body.phase() == MirPhase::Built));
}

#[test]
fn live_sets_are_deterministic() {
    let program = built("fn f(i32 a)->i32 { return a; }");
    let liveness = analyze_liveness(program.body(DefId::new(0)).expect("body"));
    let expected: BTreeSet<_> = [Local::new(1)].into_iter().collect();
    assert_eq!(liveness.live_in[BasicBlock::new(0)], expected);
}
