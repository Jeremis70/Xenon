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
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), false, &program);
    let module = cg.compile_program().expect("codegen should succeed");
    module.print_to_string().to_string()
}

// ── Named return variable ─────────────────────────────────────────────────────

/// Named return variables require an explicit `return` — implicit return
/// from a named variable is no longer supported.
#[test]
fn named_return_var_compiles_without_error() {
    compile_to_ir("fn add(u32 x, u32 y)->u32 sum { sum = x + y; return sum; }");
}

/// An explicit `return` with a named return variable emits a `ret`.
#[test]
fn named_return_var_explicit_return_emits_ret() {
    let ir = compile_to_ir("fn add(u32 x, u32 y)->u32 sum { sum = x + y; return sum; }");
    assert!(
        ir.contains("ret i32"),
        "expected ret instruction in IR:\n{ir}"
    );
}

/// The named return variable is zero-initialised on function entry.
#[test]
fn named_return_var_is_zero_initialized() {
    let ir = compile_to_ir("fn f()->u32 result { return result; }");
    assert!(
        ir.contains("store i32 0"),
        "expected zero-init store in IR:\n{ir}"
    );
}

/// Returning an expression directly (without assigning to a named variable)
/// compiles and emits the expected `ret`.
#[test]
fn named_return_with_expression_return() {
    let ir = compile_to_ir("fn add(u32 x, u32 y)->u32 sum { return x + y; }");
    assert!(
        ir.contains("ret i32"),
        "expected ret instruction in IR:\n{ir}"
    );
}

/// A named return variable without an explicit `return` must produce a
/// `MissingReturn` error — implicit return is no longer supported.
#[test]
fn named_return_without_explicit_return_errors() {
    use xenonc::error::CodegenError;

    let tokens = xenonc::frontend::lexer::lex("fn add(u32 x, u32 y)->u32 sum { sum = x + y; }")
        .expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), false, &program);
    let err = cg
        .compile_program()
        .expect_err("codegen should fail with MissingReturn");

    assert!(
        matches!(err, CodegenError::MissingReturn { ref name, .. } if name == "add"),
        "expected MissingReturn for function `add`, got: {err}"
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
    let ir = compile_to_ir("fn f(u1 x)->u32 result { if x { result = 1; } return result; }");
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
    let ir = compile_to_ir("fn f(u32 x)->u32 result { if x == 0 { result = 1; } return result; }");
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
/// assigning inside an `if` body and using an explicit return must work.
#[test]
fn if_body_can_assign_named_return_variable() {
    let ir = compile_to_ir("fn f(u1 x)->u32 result { if x { result = 42; } return result; }");
    assert!(ir.contains("ret i32"), "expected ret instruction:\n{ir}");
}

/// Statements after an `if` that has an open merge path continue to be emitted
/// in the merge block — the IR must contain both the if blocks and a subsequent
/// store/ret past the merge.
#[test]
fn statements_after_if_are_emitted_in_merge_block() {
    let ir = compile_to_ir(
        "fn f(u1 x)->u32 result { if x { result = 1; } result = 99; return result; }",
    );
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

    let tokens = xenonc::frontend::lexer::lex("fn bad()->u32 { let u32 x = 1; }")
        .expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), false, &program);
    let err = cg
        .compile_program()
        .expect_err("codegen should fail with MissingReturn");

    assert!(
        matches!(err, CodegenError::MissingReturn { ref name, .. } if name == "bad"),
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
    let ir = compile_to_ir("fn f()->u32 result { loop { result = 1; } return result; }");
    assert!(
        !ir.contains("loop_after"),
        "dead loop_after block should be removed:\n{ir}"
    );
}

/// An infinite `loop` produces a `loop_body` block with a back-edge to itself.
#[test]
fn infinite_loop_body_block_has_back_edge() {
    let ir = compile_to_ir("fn f()->u32 result { loop { result = 1; } return result; }");
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
    let ir = compile_to_ir(
        "fn f()->u32 result { loop { if result == 0 { continue; } break 1; } return result; }",
    );
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
    let ir =
        compile_to_ir("fn f()->u32 result { while result == 0 { result = 1; } return result; }");
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
    let ir =
        compile_to_ir("fn f()->u32 result { while result == 0 { result = 1; } return result; }");
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
    let ir =
        compile_to_ir("fn f()->u32 result { until result == 5 { result = 5; } return result; }");
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
    let ir = compile_to_ir(
        "fn f()->u32 result { do { result = result + 1; } while result == 0 return result; }",
    );
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
    let ir = compile_to_ir(
        "fn f()->u32 result { do { result = result + 1; } until result == 5 return result; }",
    );
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

// ── Unsigned integer semantics ────────────────────────────────────────────────

/// Comparing two `u32` values must use the unsigned `ult` predicate.
#[test]
fn unsigned_lt_uses_ult() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->bool { return a < b; }");
    assert!(
        ir.contains("icmp ult"),
        "expected `icmp ult` for unsigned comparison:\n{ir}"
    );
}

/// Comparing two `i32` values must use the signed `slt` predicate.
#[test]
fn signed_lt_uses_slt() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->bool { return a < b; }");
    assert!(
        ir.contains("icmp slt"),
        "expected `icmp slt` for signed comparison:\n{ir}"
    );
}

/// Dividing two `u32` values must use the unsigned `udiv` instruction.
#[test]
fn unsigned_div_uses_udiv() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a / b; }");
    assert!(
        ir.contains("udiv"),
        "expected `udiv` for unsigned division:\n{ir}"
    );
}

/// Right-shifting a `u32` must use the logical shift right (`lshr`).
#[test]
fn unsigned_shr_uses_lshr() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a >> b; }");
    assert!(
        ir.contains("lshr"),
        "expected `lshr` for unsigned right shift:\n{ir}"
    );
}

/// A narrow unsigned variable compared against a wide literal must widen the
/// variable with `zext`, not truncate the literal.
#[test]
fn narrow_unsigned_vs_wide_literal() {
    let ir = compile_to_ir("fn f(u8 x)->bool { return x < 200; }");
    assert!(
        ir.contains("zext"),
        "expected `zext` to widen u8 before comparison:\n{ir}"
    );
    assert!(
        ir.contains("icmp ult"),
        "expected unsigned comparison (`icmp ult`):\n{ir}"
    );
}

// ── Helpers for debug-checks tests ───────────────────────────────────────────

/// Like `compile_to_ir` but passes `debug_checks = true` to `CodeGen::new`.
fn compile_to_ir_debug(src: &str) -> String {
    let tokens = lex(src).expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), true, &program);
    let module = cg.compile_program().expect("codegen should succeed");
    module.print_to_string().to_string()
}

// ── Block-scoped variables ────────────────────────────────────────────────────

/// A variable declared inside an `if` body must not be visible after the block.
/// Attempting to use `x` (declared inside the `if`) after it must fail with
/// `UndefinedVariable`.
#[test]
fn if_body_variable_is_not_visible_after_block() {
    use xenonc::error::CodegenError;

    // `x` is declared inside the if-branch but referenced after the if — this
    // must be a compile error, not silently succeed with a dangling pointer.
    let tokens = lex("fn f(u1 cond)->u32 { if cond { let u32 x = 1; } return x; }")
        .expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), false, &program);
    let err = cg
        .compile_program()
        .expect_err("should fail: `x` is out of scope");

    assert!(
        matches!(err, CodegenError::UndefinedVariable { ref name, .. } if name == "x"),
        "expected UndefinedVariable for `x`, got: {err}"
    );
}

/// A variable declared inside the `then` branch must not shadow identically
/// named variables in the outer scope — the outer variable must still hold its
/// original value after the `if` merges.
#[test]
fn outer_variable_is_unchanged_after_if_block() {
    // `result` is in outer scope.  Inside the if, a *new* `result` is declared
    // but it is scoped to the branch and must not overwrite the outer one.
    // After the if, the outer `result` (= 99) is what gets returned.
    let ir = compile_to_ir(
        "fn f(u1 cond)->u32 result { result = 99; if cond { let u32 result = 42; } return result; }",
    );
    // The function must still compile and contain a `ret i32`.
    assert!(
        ir.contains("ret i32"),
        "expected function to compile and emit ret:\n{ir}"
    );
}

/// A variable declared inside a loop body must not be visible after the loop.
#[test]
fn loop_body_variable_is_not_visible_after_loop() {
    use xenonc::error::CodegenError;

    let tokens = lex("fn f()->u32 { loop { let u32 x = 1; break x; } return x; }")
        .expect("lexing should succeed");
    let mut parser = Parser::new(&tokens);
    let program = parser.parse_program().expect("parsing should succeed");
    let program = fold_constants(program).expect("fold should succeed");

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
    let cg = CodeGen::new(&context, "test", tm.get_target_data(), false, &program);
    let err = cg
        .compile_program()
        .expect_err("should fail: `x` is out of scope after loop");

    assert!(
        matches!(err, CodegenError::UndefinedVariable { ref name, .. } if name == "x"),
        "expected UndefinedVariable for `x`, got: {err}"
    );
}

/// An inner scope variable may shadow an outer-scope variable; the inner
/// assignment must not affect the outer binding.
#[test]
fn inner_scope_does_not_pollute_outer() {
    // After the if, `result` (named return, outer scope) must still be 10.
    let ir = compile_to_ir(
        "fn f(u1 cond)->u32 result { result = 10; if cond { let u32 result = 99; } return result; }",
    );
    assert!(
        ir.contains("ret i32"),
        "expected function to compile:\n{ir}"
    );
    // The outer store of 10 must be present.
    assert!(
        ir.contains("store i32 10"),
        "outer store of 10 missing:\n{ir}"
    );
    // The inner store of 99 must also appear (in the then block).
    assert!(
        ir.contains("store i32 99"),
        "inner store of 99 missing:\n{ir}"
    );
}

// ── Division by zero protection ───────────────────────────────────────────────

/// Division of a signed integer must emit a zero-check before `sdiv`.
/// The IR must contain a `div_trap` block and an `llvm.trap` call.
#[test]
fn signed_div_emits_zero_check() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a / b; }");
    assert!(
        ir.contains("div_trap"),
        "expected div_trap block for signed division:\n{ir}"
    );
    assert!(
        ir.contains("llvm.trap"),
        "expected llvm.trap call in div_trap:\n{ir}"
    );
    assert!(ir.contains("sdiv"), "expected sdiv instruction:\n{ir}");
}

/// Division of an unsigned integer must emit a zero-check before `udiv`.
#[test]
fn unsigned_div_emits_zero_check() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a / b; }");
    assert!(
        ir.contains("div_trap"),
        "expected div_trap block for unsigned division:\n{ir}"
    );
    assert!(ir.contains("llvm.trap"), "expected llvm.trap call:\n{ir}");
    assert!(ir.contains("udiv"), "expected udiv instruction:\n{ir}");
}

/// Signed remainder must also emit a zero-check before `srem`.
#[test]
fn signed_rem_emits_zero_check() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a % b; }");
    assert!(ir.contains("div_trap"), "expected div_trap block:\n{ir}");
    assert!(ir.contains("srem"), "expected srem instruction:\n{ir}");
}

/// Unsigned remainder must also emit a zero-check before `urem`.
#[test]
fn unsigned_rem_emits_zero_check() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a % b; }");
    assert!(ir.contains("div_trap"), "expected div_trap block:\n{ir}");
    assert!(ir.contains("urem"), "expected urem instruction:\n{ir}");
}

/// The div_ok block must be present and the division instruction placed in it
/// (i.e. after the zero-check guard), not before.
#[test]
fn div_ok_block_contains_division() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a / b; }");
    assert!(ir.contains("div_ok"), "expected div_ok block:\n{ir}");
    // div_ok must appear before sdiv in the textual IR.
    let ok_pos = ir.find("div_ok").expect("div_ok must exist");
    let div_pos = ir.find("sdiv").expect("sdiv must exist");
    assert!(
        div_pos > ok_pos,
        "sdiv should appear after div_ok in the IR:\n{ir}"
    );
}

// ── Shift count validation ────────────────────────────────────────────────────

/// A left-shift must emit a shift-count validation before the `shl` instruction.
#[test]
fn left_shift_emits_shift_check() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a << b; }");
    assert!(
        ir.contains("shift_trap"),
        "expected shift_trap block for left shift:\n{ir}"
    );
    assert!(
        ir.contains("llvm.trap"),
        "expected llvm.trap in shift_trap:\n{ir}"
    );
    assert!(ir.contains("shl"), "expected shl instruction:\n{ir}");
}

/// A right-shift of a signed integer must emit a shift-count check before `ashr`.
#[test]
fn signed_right_shift_emits_shift_check() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a >> b; }");
    assert!(
        ir.contains("shift_trap"),
        "expected shift_trap block for right shift:\n{ir}"
    );
    assert!(
        ir.contains("ashr"),
        "expected ashr (arithmetic shift right):\n{ir}"
    );
}

/// A right-shift of an unsigned integer emits a shift-count check before `lshr`.
#[test]
fn unsigned_right_shift_emits_shift_check() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a >> b; }");
    assert!(
        ir.contains("shift_trap"),
        "expected shift_trap block for unsigned right shift:\n{ir}"
    );
    assert!(
        ir.contains("lshr"),
        "expected lshr (logical shift right):\n{ir}"
    );
}

/// The shift_ok block must appear and the shift instruction must follow it.
#[test]
fn shift_ok_block_contains_shift_instruction() {
    let ir = compile_to_ir("fn f(u32 a, u32 b)->u32 { return a << b; }");
    assert!(ir.contains("shift_ok"), "expected shift_ok block:\n{ir}");
    let ok_pos = ir.find("shift_ok").expect("shift_ok must exist");
    let shl_pos = ir.find("shl").expect("shl must exist");
    assert!(
        shl_pos > ok_pos,
        "shl should appear after shift_ok in the IR:\n{ir}"
    );
}

// ── Integer overflow checks (debug mode) ─────────────────────────────────────

/// In debug mode, signed addition must use the `llvm.sadd.with.overflow`
/// intrinsic and emit an overflow trap block.
#[test]
fn debug_signed_add_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(i32 a, i32 b)->i32 { return a + b; }");
    assert!(
        ir.contains("sadd.with.overflow"),
        "expected sadd.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
    assert!(
        ir.contains("llvm.trap"),
        "expected llvm.trap in overflow_trap:\n{ir}"
    );
}

/// In debug mode, signed subtraction must use `llvm.ssub.with.overflow`.
#[test]
fn debug_signed_sub_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(i32 a, i32 b)->i32 { return a - b; }");
    assert!(
        ir.contains("ssub.with.overflow"),
        "expected ssub.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
}

/// In debug mode, signed multiplication must use `llvm.smul.with.overflow`.
#[test]
fn debug_signed_mul_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(i32 a, i32 b)->i32 { return a * b; }");
    assert!(
        ir.contains("smul.with.overflow"),
        "expected smul.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
}

/// In debug mode, unsigned addition must use `llvm.uadd.with.overflow`.
#[test]
fn debug_unsigned_add_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(u32 a, u32 b)->u32 { return a + b; }");
    assert!(
        ir.contains("uadd.with.overflow"),
        "expected uadd.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
}

/// In debug mode, unsigned subtraction must use `llvm.usub.with.overflow`.
#[test]
fn debug_unsigned_sub_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(u32 a, u32 b)->u32 { return a - b; }");
    assert!(
        ir.contains("usub.with.overflow"),
        "expected usub.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
}

/// In debug mode, unsigned multiplication must use `llvm.umul.with.overflow`.
#[test]
fn debug_unsigned_mul_uses_overflow_intrinsic() {
    let ir = compile_to_ir_debug("fn f(u32 a, u32 b)->u32 { return a * b; }");
    assert!(
        ir.contains("umul.with.overflow"),
        "expected umul.with.overflow intrinsic in debug mode:\n{ir}"
    );
    assert!(
        ir.contains("overflow_trap"),
        "expected overflow_trap block:\n{ir}"
    );
}

/// In release mode (`debug_checks = false`), plain `add` must be emitted —
/// no overflow intrinsic, no trap block.
#[test]
fn release_add_is_plain_wrapping() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a + b; }");
    assert!(
        !ir.contains("with.overflow"),
        "release mode must not use overflow intrinsic:\n{ir}"
    );
    assert!(
        !ir.contains("overflow_trap"),
        "release mode must not have overflow_trap block:\n{ir}"
    );
    assert!(ir.contains("add"), "expected plain add instruction:\n{ir}");
}

/// In release mode, plain `sub` is emitted (no overflow intrinsic).
#[test]
fn release_sub_is_plain_wrapping() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a - b; }");
    assert!(
        !ir.contains("with.overflow"),
        "release mode must not use overflow intrinsic:\n{ir}"
    );
    assert!(ir.contains("sub"), "expected plain sub instruction:\n{ir}");
}

/// In release mode, plain `mul` is emitted (no overflow intrinsic).
#[test]
fn release_mul_is_plain_wrapping() {
    let ir = compile_to_ir("fn f(i32 a, i32 b)->i32 { return a * b; }");
    assert!(
        !ir.contains("with.overflow"),
        "release mode must not use overflow intrinsic:\n{ir}"
    );
    assert!(ir.contains("mul"), "expected plain mul instruction:\n{ir}");
}

/// The overflow_ok block must come after the overflow extraction, and the
/// result value produced by the intrinsic must be extracted before the trap
/// guard — so the `extractvalue` for the result precedes `overflow_ok`.
#[test]
fn debug_overflow_ok_block_is_present() {
    let ir = compile_to_ir_debug("fn f(i32 a, i32 b)->i32 { return a + b; }");
    assert!(
        ir.contains("overflow_ok"),
        "expected overflow_ok block:\n{ir}"
    );
    let trap_pos = ir.find("overflow_trap").expect("overflow_trap must exist");
    let ok_pos = ir.find("overflow_ok").expect("overflow_ok must exist");
    // trap block is defined before ok block in textual IR order.
    assert!(
        ok_pos > trap_pos,
        "overflow_ok should appear after overflow_trap in the IR:\n{ir}"
    );
}

// ── Entry point / #[entry] attribute ──────────────────────────────────────────

/// A function named "main" with `#[entry]` compiles as `@main` directly.
#[test]
fn entry_named_main_compiles_directly() {
    let ir = compile_to_ir("#[entry] fn main()->i32 { return 0; }");
    assert!(
        ir.contains("define i32 @main()"),
        "expected @main definition:\n{ir}"
    );
}

/// A function with a custom name and `#[entry]` gets a `@main` wrapper.
#[test]
fn entry_custom_name_emits_main_wrapper() {
    let ir = compile_to_ir("#[entry] fn start()->i32 { return 42; }");
    // The user function is defined under its real name.
    assert!(
        ir.contains("define i32 @start()"),
        "expected @start definition:\n{ir}"
    );
    // A thin @main wrapper is also generated.
    assert!(
        ir.contains("define i32 @main()"),
        "expected @main wrapper:\n{ir}"
    );
    // The wrapper calls @start.
    assert!(
        ir.contains("call i32 @start()"),
        "expected call to @start in @main wrapper:\n{ir}"
    );
}

/// Entry function can coexist with other helper functions.
#[test]
fn entry_with_helper_functions() {
    let src = "#[entry] fn run()->i32 { return helper(); } fn helper()->i32 { return 7; }";
    let ir = compile_to_ir(src);
    assert!(ir.contains("define i32 @run()"), "expected @run:\n{ir}");
    assert!(
        ir.contains("define i32 @helper()"),
        "expected @helper:\n{ir}"
    );
    assert!(ir.contains("define i32 @main()"), "expected @main:\n{ir}");
}

/// When entry is not named "main" and there IS another function named "main",
/// the non-entry "main" must be renamed to avoid collision with the @main wrapper,
/// and call sites to it must still resolve correctly.
#[test]
fn entry_plus_user_main_no_collision() {
    // `fn main` is a plain helper — NOT the entry point.
    // Codegen must rename it to `_xe.main` and emit a @main wrapper for `start`.
    let src = "#[entry] fn start()->i32 { return main(); } fn main()->i32 { return 99; }";
    let ir = compile_to_ir(src);

    // The entry function compiles under its real name.
    assert!(ir.contains("define i32 @start()"), "expected @start:\n{ir}");

    // The renamed helper must appear in the IR.
    assert!(
        ir.contains("@\"_xe.main\"") || ir.contains("@_xe.main"),
        "expected renamed symbol _xe.main in IR:\n{ir}"
    );

    // The @main wrapper must call @start, not the renamed helper.
    assert!(
        ir.contains("call i32 @start()"),
        "expected @main wrapper to call @start:\n{ir}"
    );
}
