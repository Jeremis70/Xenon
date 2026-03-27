use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use xenonc::backend::codegen::CodeGen;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::constant_fold::fold_constants;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compile a Xenon source string to LLVM IR text.
fn compile_to_ir(src: &str) -> String {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program);

    Target::initialize_native(&InitializationConfig::default())
        .expect("native target init should succeed");
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).expect("target from triple should succeed");
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let tm = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::None,
            RelocMode::Default,
            CodeModel::Default,
        )
        .expect("target machine creation should succeed");

    let context = Context::create();
    let cg = CodeGen::new(&context, "test", tm.get_target_data());
    let module = cg
        .compile_program(&program)
        .expect("codegen should succeed");
    module.print_to_string().to_string()
}

// ── Named return variable ─────────────────────────────────────────────────────

/// The named return variable is recognised as a writable variable inside the body.
#[test]
fn named_return_var_compiles_without_error() {
    compile_to_ir("fn add(u32 x, u32 y)->u32 sum { sum = x + y; }");
}

/// Without an explicit `return`, the compiler emits an implicit `ret` that
/// loads and returns the named variable.
#[test]
fn named_return_var_implicit_return_emits_ret() {
    let ir = compile_to_ir("fn add(u32 x, u32 y)->u32 sum { sum = x + y; }");
    assert!(
        ir.contains("ret i32"),
        "expected ret instruction in IR:\n{ir}"
    );
}

/// The named return variable is zero-initialised on function entry, so a
/// function that never assigns it still returns a deterministic value.
#[test]
fn named_return_var_is_zero_initialized() {
    let ir = compile_to_ir("fn f()->u32 result { }");
    assert!(
        ir.contains("store i32 0"),
        "expected zero-init store in IR:\n{ir}"
    );
}

/// An explicit `return` still works correctly when a named return variable is
/// present — the two styles must coexist.
#[test]
fn named_return_var_explicit_return_still_works() {
    let ir = compile_to_ir("fn add(u32 x, u32 y)->u32 sum { sum = x + y; return sum; }");
    assert!(
        ir.contains("ret i32"),
        "expected ret instruction in IR:\n{ir}"
    );
}

// ── Regression: functions without a named return binding ─────────────────────

/// Plain `return <expr>` in a function without a named return binding continues
/// to work after the implicit-return feature was added.
#[test]
fn unnamed_return_explicit_return_unaffected() {
    let ir = compile_to_ir("fn double(u32 x)->u32 { return x; }");
    assert!(
        ir.contains("ret i32"),
        "expected ret instruction in IR:\n{ir}"
    );
}

// ── If statements ─────────────────────────────────────────────────────────────

/// A bare `if` (no else) must emit a `then` block, an `else` block, and a
/// `if_merge` block that both sides branch to.
#[test]
fn if_only_emits_then_else_and_merge_blocks() {
    let ir = compile_to_ir("fn f(u1 x)->u32 result { if x { result = 1; } }");
    assert!(ir.contains("then:"), "expected then block:\n{ir}");
    assert!(ir.contains("else:"), "expected else block:\n{ir}");
    assert!(ir.contains("if_merge:"), "expected if_merge block:\n{ir}");
}

/// When every branch terminates (returns), the merge block must be absent from
/// the IR — it would be unreachable and the codegen should delete it.
#[test]
fn if_all_branches_return_omits_merge_block() {
    let ir = compile_to_ir("fn f(u1 x)->u8 { if x { return 1; } else { return 0; } }");
    assert!(
        !ir.contains("if_merge"),
        "dead merge block should be removed:\n{ir}"
    );
}

/// The condition of an `if` whose expression is already an `i1` (e.g. any
/// comparison) must be used directly — no redundant `icmp ne ..., false` wrapper.
#[test]
fn if_comparison_condition_has_no_redundant_icmp() {
    let ir = compile_to_ir("fn f(u32 x)->u32 result { if x == 0 { result = 1; } }");
    // There must be exactly one `icmp` — the comparison itself.
    let icmp_count = ir.matches("icmp").count();
    assert_eq!(
        icmp_count, 1,
        "expected exactly one icmp instruction, found {icmp_count}:\n{ir}"
    );
}

/// An `else if` chain is compiled correctly: all reachable blocks have
/// predecessors and every branch is terminated.
#[test]
fn else_if_chain_all_blocks_have_predecessors() {
    let ir = compile_to_ir(
        "fn f(u32 x)->u8 { if x == 1 { return 1; } else if x == 2 { return 2; } else { return 3; } }",
    );
    assert!(
        !ir.contains("No predecessors"),
        "all blocks should have predecessors:\n{ir}"
    );
}

/// A chain of *two* `else if` clauses produces the correct number of returns
/// and no unreachable blocks.
#[test]
fn multiple_else_if_clauses_all_blocks_have_predecessors() {
    let ir = compile_to_ir(
        "fn f(u32 x)->u8 { if x == 1 { return 1; } else if x == 2 { return 2; } else if x == 3 { return 3; } else { return 4; } }",
    );
    assert!(
        !ir.contains("No predecessors"),
        "all blocks should have predecessors:\n{ir}"
    );
    // Four distinct return values must all appear.
    assert!(ir.contains("ret i8 1"), "missing ret i8 1:\n{ir}");
    assert!(ir.contains("ret i8 2"), "missing ret i8 2:\n{ir}");
    assert!(ir.contains("ret i8 3"), "missing ret i8 3:\n{ir}");
    assert!(ir.contains("ret i8 4"), "missing ret i8 4:\n{ir}");
}

/// The `if` codegen integrates with the named-return-variable feature:
/// assigning inside an `if` body and relying on the implicit return must work.
#[test]
fn if_body_can_assign_named_return_variable() {
    let ir = compile_to_ir("fn f(u1 x)->u32 result { if x { result = 42; } }");
    assert!(ir.contains("ret i32"), "expected ret instruction:\n{ir}");
}

/// Statements after an `if` that has an open merge path continue to be emitted
/// in the merge block — the IR must contain both the if blocks and a subsequent
/// store/ret past the merge.
#[test]
fn statements_after_if_are_emitted_in_merge_block() {
    let ir = compile_to_ir("fn f(u1 x)->u32 result { if x { result = 1; } result = 99; }");
    // The final assignment into 'result' and the implicit return must be present.
    assert!(ir.contains("store"), "expected store instructions:\n{ir}");
    assert!(ir.contains("ret i32"), "expected ret instruction:\n{ir}");
}

// ── Return statements ─────────────────────────────────────────────────────────

/// A function without a `return` and without a named return variable must
/// produce a `MissingReturn` error instead of segfaulting in LLVM.
#[test]
fn missing_return_yields_codegen_error() {
    use xenonc::error::CodegenError;

    let tokens = xenonc::frontend::lexer::lex("fn bad()->u32 { u32 x = 1; }")
        .expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program);

    Target::initialize_native(&InitializationConfig::default())
        .expect("native target init should succeed");
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).expect("target from triple should succeed");
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let tm = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::None,
            RelocMode::Default,
            CodeModel::Default,
        )
        .expect("target machine creation should succeed");

    let context = Context::create();
    let cg = CodeGen::new(&context, "test", tm.get_target_data());
    let err = cg
        .compile_program(&program)
        .expect_err("codegen should fail with MissingReturn");

    assert!(
        matches!(err, CodegenError::MissingReturn { ref name } if name == "bad"),
        "expected MissingReturn for function `bad`, got: {err}"
    );
}

/// An explicit `return` at the end of a function without a named return
/// variable compiles correctly and the IR terminates with `ret`.
#[test]
fn explicit_return_terminates_function() {
    let ir = compile_to_ir("fn f()->u32 { return 7; }");
    assert!(
        ir.contains("ret i32 7"),
        "expected `ret i32 7` in IR:\n{ir}"
    );
}

// ── Infinite `loop` ───────────────────────────────────────────────────────────

/// An infinite `loop` with no `break` must not produce a `loop_after` block
/// in the IR — the dead block should be removed entirely.
#[test]
fn infinite_loop_omits_loop_after_block() {
    let ir = compile_to_ir("fn f()->u32 result { loop { result = 1; } }");
    assert!(
        !ir.contains("loop_after"),
        "dead loop_after block should be removed:\n{ir}"
    );
}

/// An infinite `loop` produces a `loop_body` block with a back-edge to itself.
#[test]
fn infinite_loop_body_block_has_back_edge() {
    let ir = compile_to_ir("fn f()->u32 result { loop { result = 1; } }");
    assert!(ir.contains("loop_body"), "expected loop_body block:\n{ir}");
    // The back-edge unconditional branch back to loop_body must appear.
    assert!(
        ir.contains("br label %loop_body"),
        "expected back-edge branch to loop_body:\n{ir}"
    );
}

/// A `loop` with a `break` must emit `loop_after` as the exit target and load
/// the break value at that point.
#[test]
fn loop_with_break_emits_loop_after_block() {
    // `loop` is an expression; `return` consumes its value so the function terminates.
    let ir = compile_to_ir("fn f()->u32 { return loop { break 42; }; }");
    assert!(
        ir.contains("loop_after:"),
        "expected loop_after block:\n{ir}"
    );
    assert!(
        ir.contains("loop_val"),
        "expected loop result load in IR:\n{ir}"
    );
}

/// `continue` inside a `loop` must branch back to `loop_body` (the only
/// available re-entry point for an infinite loop).
#[test]
fn continue_in_infinite_loop_branches_to_loop_body() {
    // Uses a named return so the function is valid without an explicit return.
    let ir = compile_to_ir("fn f()->u32 result { loop { if result == 0 { continue; } break 1; } }");
    assert!(
        ir.contains("br label %loop_body"),
        "expected continue to branch back to loop_body:\n{ir}"
    );
}

// ── Pre-condition loops (`while` / `until`) ───────────────────────────────────

/// A `while` loop must generate a `loop_cond` block that is branched to
/// before the body and also as the back-edge target for `continue`.
#[test]
fn while_loop_emits_loop_cond_block() {
    let ir = compile_to_ir("fn f()->u32 result { while result == 0 { result = 1; } }");
    assert!(ir.contains("loop_cond:"), "expected loop_cond block:\n{ir}");
    assert!(ir.contains("loop_body:"), "expected loop_body block:\n{ir}");
    assert!(
        ir.contains("loop_after:"),
        "expected loop_after block:\n{ir}"
    );
}

/// The `while` entry edge must jump to `loop_cond`, not directly to `loop_body`.
#[test]
fn while_loop_entry_branches_to_cond_first() {
    let ir = compile_to_ir("fn f()->u32 result { while result == 0 { result = 1; } }");
    // The entry block unconditionally branches to loop_cond (pre-check).
    assert!(
        ir.contains("br label %loop_cond"),
        "expected entry to branch to loop_cond:\n{ir}"
    );
}

/// An `until` loop iterates while the condition is *false* and exits when it
/// becomes true —  the conditional branch must still exit to `loop_after`.
#[test]
fn until_loop_exits_when_condition_becomes_true() {
    let ir = compile_to_ir("fn f()->u32 result { until result == 5 { result = 5; } }");
    assert!(ir.contains("loop_cond:"), "expected loop_cond block:\n{ir}");
    assert!(
        ir.contains("loop_after:"),
        "expected loop_after block:\n{ir}"
    );
    // Both body_bb and after_bb must be referenced from the cond block.
    assert!(
        ir.contains("br i1"),
        "expected conditional branch in loop_cond:\n{ir}"
    );
}

// ── Post-condition loops (`do-while` / `do-until`) ────────────────────────────

/// A `do-while` loop must place `loop_cond` *after* `loop_body` in the IR so
/// that the body always executes at least once.
#[test]
fn do_while_loop_emits_cond_after_body() {
    let ir = compile_to_ir("fn f()->u32 result { do { result = result + 1; } while result == 0 }");
    // Entry must jump directly to loop_body, not loop_cond.
    assert!(ir.contains("loop_body:"), "expected loop_body block:\n{ir}");
    assert!(ir.contains("loop_cond:"), "expected loop_cond block:\n{ir}");
    // loop_cond must appear after loop_body in the textual IR.
    let body_pos = ir.find("loop_body:").expect("loop_body must exist");
    let cond_pos = ir.find("loop_cond:").expect("loop_cond must exist");
    assert!(
        cond_pos > body_pos,
        "loop_cond should appear after loop_body in the IR:\n{ir}"
    );
}

/// A `do-until` loop executes the body at least once and exits when the
/// condition first becomes true.
#[test]
fn do_until_loop_emits_cond_after_body() {
    let ir = compile_to_ir("fn f()->u32 result { do { result = result + 1; } until result == 5 }");
    assert!(ir.contains("loop_body:"), "expected loop_body block:\n{ir}");
    assert!(ir.contains("loop_cond:"), "expected loop_cond block:\n{ir}");
    let body_pos = ir.find("loop_body:").expect("loop_body must exist");
    let cond_pos = ir.find("loop_cond:").expect("loop_cond must exist");
    assert!(
        cond_pos > body_pos,
        "loop_cond should appear after loop_body in the IR:\n{ir}"
    );
}

// ── usize / isize (pointer-sized integers) ────────────────────────────────────

/// `usize` lowers to the pointer-sized integer type reported by the target's
/// data layout — the same type LLVM uses for pointer arithmetic on that target.
#[test]
fn usize_lowers_to_pointer_sized_int() {
    let ir = compile_to_ir("fn f(usize x)->usize { return x; }");
    // On a 64-bit host this is i64; on 32-bit it's i32. Either way the IR
    // must contain the right LLVM integer type for the target.
    let ptr_bits = std::mem::size_of::<*const ()>() * 8;
    assert!(
        ir.contains(&format!("i{ptr_bits}")),
        "expected i{ptr_bits} in IR for usize:\n{ir}"
    );
}

/// `isize` lowers to the same pointer-sized integer type as `usize`; there is
/// no sign distinction at the LLVM type level.
#[test]
fn isize_lowers_to_pointer_sized_int() {
    let ir = compile_to_ir("fn f(isize x)->isize { return x; }");
    let ptr_bits = std::mem::size_of::<*const ()>() * 8;
    assert!(
        ir.contains(&format!("i{ptr_bits}")),
        "expected i{ptr_bits} in IR for isize:\n{ir}"
    );
}

/// Arithmetic on `usize` values produces valid IR without errors.
#[test]
fn usize_arithmetic_compiles() {
    compile_to_ir("fn add(usize a, usize b)->usize { return a + b; }");
}
