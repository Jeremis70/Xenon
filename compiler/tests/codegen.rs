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
