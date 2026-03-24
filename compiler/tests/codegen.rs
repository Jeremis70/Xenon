use inkwell::context::Context;
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
    let context = Context::create();
    let cg = CodeGen::new(&context, "test");
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
