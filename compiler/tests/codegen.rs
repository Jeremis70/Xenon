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
