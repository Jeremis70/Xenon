//! LLVM backend integration tests. Backend entry points are supplied with MIR,
//! not source programs or AST nodes.

use std::path::Path;

use xenonc::backend::contract::{ArtifactKind, Backend, CodegenOptions, OutputRequest};
use xenonc::backend::llvm::LlvmBackend;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::middle::constant_fold::fold_constants;
use xenonc::middle::mir::MirProgram;
use xenonc::middle::mir::analysis::{
    OverflowMode, RuntimeCheckPolicy, analyze_program, normalize_runtime_checks,
};
use xenonc::middle::mir::build::build_mir;
use xenonc::middle::target::TargetSpec;
use xenonc::middle::typecheck::check_program;

fn build_mir_for_source(source: &str) -> MirProgram {
    let tokens = lex(source).expect("source should lex");
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_program().expect("source should parse");
    let ast = fold_constants(ast).expect("constants should fold");
    let target = TargetSpec::host();
    let thir = check_program(&ast, &target).expect("source should type-check");
    let mut mir = build_mir(&thir).expect("THIR should lower");
    analyze_program(&mut mir, &target).expect("MIR flow analysis should pass");
    normalize_runtime_checks(
        &mut mir,
        &target,
        RuntimeCheckPolicy {
            overflow: OverflowMode::Checked,
        },
    )
    .expect("runtime checks should normalize");
    mir
}

fn emit_ir(program: &MirProgram, path: &Path) -> String {
    LlvmBackend
        .emit(
            program,
            &TargetSpec::host(),
            CodegenOptions { optimization: 0 },
            &[OutputRequest {
                kind: ArtifactKind::LlvmIr,
                path: path.to_path_buf(),
            }],
        )
        .expect("MIR should emit successfully");
    std::fs::read_to_string(path).expect("LLVM IR should be written")
}

#[test]
fn llvm_backend_emits_runtime_mir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("backend.ll");
    let mir = build_mir_for_source(
        "#[entry]\nfn main() -> i32 { let i32 value = 20; value = value + 22; return value; }\n",
    );

    let llvm_ir = emit_ir(&mir, &path);

    assert!(llvm_ir.contains("define i32 @main"), "{llvm_ir}");
    assert!(llvm_ir.contains("llvm.trap"), "{llvm_ir}");
}

#[test]
fn llvm_optimization_promotes_stack_slots_and_folds_constants() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("optimized.ll");
    let mir = build_mir_for_source(
        "#[entry] fn main()->i32 { let i32 value = 40; value = value + 2; return value; }",
    );
    let unoptimized = emit_ir(&mir, &path);
    assert!(unoptimized.contains("alloca"), "{unoptimized}");

    for optimization in [1, 2, 3, u8::MAX] {
        LlvmBackend
            .emit(
                &mir,
                &TargetSpec::host(),
                CodegenOptions { optimization },
                &[OutputRequest {
                    kind: ArtifactKind::LlvmIr,
                    path: path.clone(),
                }],
            )
            .expect("optimization should succeed, including levels capped at O3");
        let optimized = std::fs::read_to_string(&path).expect("read optimized IR");
        assert!(
            optimized.contains("ret i32 42"),
            "O{optimization}: {optimized}"
        );
        assert!(
            !optimized.contains("alloca"),
            "O{optimization}: {optimized}"
        );
    }
}

#[test]
fn llvm_backend_rejects_built_mir() {
    let source = "#[entry]\nfn main() -> i32 { return 0; }\n";
    let tokens = lex(source).expect("source should lex");
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_program().expect("source should parse");
    let thir = check_program(&ast, &TargetSpec::host()).expect("source should type-check");
    let mir = build_mir(&thir).expect("THIR should lower");

    let result = LlvmBackend.emit(
        &mir,
        &TargetSpec::host(),
        CodegenOptions { optimization: 0 },
        &[],
    );

    assert!(result.is_err(), "backend must require Runtime MIR");
}

#[test]
fn custom_entry_and_user_main_get_a_verified_mir_wrapper() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("entry.ll");
    let mir = build_mir_for_source(
        "#[entry]\nfn start() -> i32 { return 9; }\nfn main() -> i32 { return 0; }\n",
    );

    let llvm_ir = emit_ir(&mir, &path);

    assert!(llvm_ir.contains("define i32 @main()"), "{llvm_ir}");
    assert!(llvm_ir.contains("define i32 @start()"), "{llvm_ir}");
    assert!(llvm_ir.contains("define i32 @_xe.main()"), "{llvm_ir}");
    assert!(llvm_ir.contains("call i32 @start()"), "{llvm_ir}");
}

/// Compiles `source` (without an entry point) to LLVM IR text.
fn ir_for(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    emit_ir(&build_mir_for_source(source), &dir.path().join("out.ll"))
}

#[test]
fn integer_operations_follow_signedness() {
    let unsigned = ir_for(
        "fn f(u32 a, u32 b)->bool { return a < b; }\n\
         fn g(u32 a, u32 b)->u32 { return a / b; }\n\
         fn h(u32 a, u32 b)->u32 { return a >> b; }\n",
    );
    assert!(unsigned.contains("icmp ult"), "{unsigned}");
    assert!(unsigned.contains("udiv"), "{unsigned}");
    assert!(unsigned.contains("lshr"), "{unsigned}");

    let signed = ir_for(
        "fn f(i32 a, i32 b)->bool { return a < b; }\n\
         fn g(i32 a, i32 b)->i32 { return a / b; }\n\
         fn h(i32 a, i32 b)->i32 { return a >> b; }\n",
    );
    assert!(signed.contains("icmp slt"), "{signed}");
    assert!(signed.contains("sdiv"), "{signed}");
    assert!(signed.contains("ashr"), "{signed}");
}

#[test]
fn float_and_pointer_comparisons_lower_to_compares() {
    let ir = ir_for(
        "fn f(f64 a, f64 b)->bool { return a <= b; }\n\
         fn g(*i32 a, *i32 b)->bool { return a == b; }\n",
    );
    assert!(ir.contains("fcmp ole double"), "{ir}");
    assert!(ir.contains("ptrtoint"), "{ir}");
    assert!(ir.contains("icmp eq"), "{ir}");
}

#[test]
fn float_not_equal_is_true_for_nan() {
    let ir = ir_for("fn f(f64 a, f64 b)->bool { return a != b; }");
    assert!(ir.contains("fcmp une double"), "{ir}");
}

#[test]
fn shift_amount_extends_by_its_own_signedness() {
    let ir = ir_for(
        "fn f(u16 x, u4 n)->u16 { return x << n; }\n\
         fn g(u16 x, i4 n)->u16 { return x << n; }\n",
    );
    assert!(ir.contains("zext i4 %"), "{ir}");
    assert!(ir.contains("to i16"), "{ir}");
    assert!(ir.contains("sext i4 %"), "{ir}");
}

#[test]
fn pointer_sized_integers_use_the_target_width() {
    let ir = ir_for("fn f(usize a, isize b)->usize { return a; }");
    let width = TargetSpec::host().pointer_width();
    assert!(
        ir.contains(&format!("define i{width} @f(i{width} %0, i{width} %1)")),
        "{ir}"
    );
}

#[test]
fn zero_sized_integers_are_empty_structs() {
    let ir = ir_for(
        "fn id(u0 a)->u0 { return a; }\n\
         fn add(i0 a, i0 b)->i0 { return a + b; }\n",
    );
    assert!(ir.contains("define {} @id({} %0)"), "{ir}");
    assert!(ir.contains("define {} @add({} %0, {} %1)"), "{ir}");
    assert!(!ir.contains("i0"), "{ir}");
}
