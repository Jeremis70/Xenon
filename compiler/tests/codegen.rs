//! LLVM backend integration tests. Backend entry points are supplied with MIR,
//! not source programs or AST nodes.

use std::path::Path;

use xenonc::backend::contract::{
    ArtifactKind, Backend, CodegenOptions, OutputRequest, RelocationModel,
};
use xenonc::backend::llvm::LlvmBackend;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
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
            CodegenOptions {
                optimization: 0,
                relocation_model: RelocationModel::Pic,
            },
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
        "#[entry]\nfn main() -> u8 { let u8 value = 20; value = value + 22; return value; }\n",
    );

    let llvm_ir = emit_ir(&mir, &path);

    assert!(llvm_ir.contains("define i32 @main"), "{llvm_ir}");
    assert!(llvm_ir.contains("llvm.trap"), "{llvm_ir}");
}

#[test]
fn llvm_arrays_use_array_types_and_checked_geps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mir = build_mir_for_source(
        "fn get([3]u8 a, i32 i)->u8 { return a[i]; }
         #[entry] fn main()->u8 { let [3]u8 a = [1, 2, 3]; return get(a, 1); }",
    );
    let ir = emit_ir(&mir, &dir.path().join("arrays.ll"));
    assert!(ir.contains("[3 x i8]"), "{ir}");
    assert!(ir.contains("getelementptr inbounds [3 x i8]"), "{ir}");
    assert!(ir.contains("icmp slt") && ir.contains("icmp sge"), "{ir}");
    assert!(ir.contains("llvm.trap"), "{ir}");
}

#[test]
fn llvm_optimization_promotes_stack_slots_and_folds_constants() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("optimized.ll");
    let mir = build_mir_for_source(
        "#[entry] fn main()->u8 { let u8 value = 40; value = value + 2; return value; }",
    );
    let unoptimized = emit_ir(&mir, &path);
    assert!(unoptimized.contains("alloca"), "{unoptimized}");

    for optimization in [1, 2, 3, u8::MAX] {
        LlvmBackend
            .emit(
                &mir,
                &TargetSpec::host(),
                CodegenOptions {
                    optimization,
                    relocation_model: RelocationModel::Pic,
                },
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
    let source = "#[entry]\nfn main() {}\n";
    let tokens = lex(source).expect("source should lex");
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_program().expect("source should parse");
    let thir = check_program(&ast, &TargetSpec::host()).expect("source should type-check");
    let mir = build_mir(&thir).expect("THIR should lower");

    let result = LlvmBackend.emit(
        &mir,
        &TargetSpec::host(),
        CodegenOptions {
            optimization: 0,
            relocation_model: RelocationModel::Pic,
        },
        &[],
    );

    assert!(result.is_err(), "backend must require Runtime MIR");
}

#[test]
fn custom_entry_and_user_main_get_a_verified_mir_wrapper() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("entry.ll");
    let mir = build_mir_for_source(
        "#[entry]\nfn start() -> u8 { return 9; }\nfn main() -> i32 { return 0; }\n",
    );

    let llvm_ir = emit_ir(&mir, &path);

    assert!(llvm_ir.contains("define i32 @main()"), "{llvm_ir}");
    assert!(llvm_ir.contains("define i8 @start()"), "{llvm_ir}");
    assert!(llvm_ir.contains("define i32 @_xe.main()"), "{llvm_ir}");
    assert!(llvm_ir.contains("call i8 @start()"), "{llvm_ir}");
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

#[test]
fn tuples_lower_to_llvm_structs() {
    let ir = ir_for(
        "fn divmod(i32 n, i32 d)->i32 q, i32 r { q = n / d; r = n % d; return; }\n\
         fn swap(i32 a, i32 b)->i32, i32 { a, b = b, a; return a, b; }\n\
         fn unit() {}\n",
    );
    assert!(
        ir.contains("define { i32, i32 } @divmod(i32 %0, i32 %1)"),
        "{ir}"
    );
    assert!(
        ir.contains("define { i32, i32 } @swap(i32 %0, i32 %1)"),
        "{ir}"
    );
    assert!(ir.contains("define {} @unit()"), "{ir}");
    assert!(ir.contains("insertvalue { i32, i32 }"), "{ir}");
}

#[test]
fn tuple_elements_are_addressed_with_struct_geps() {
    let ir = ir_for("fn f((i32, bool) t)->bool { t.0 = 3; return t.1; }\n");
    assert!(ir.contains("getelementptr inbounds"), "{ir}");
    assert!(ir.contains("{ i32, i1 }"), "{ir}");
}

#[test]
fn zero_sized_tuples_keep_their_elements() {
    // `(i0, i0)` has no bits, but its elements must stay addressable.
    let ir = ir_for("fn f((i0, i0) t)->i0 { return t.1; }\n");
    assert!(ir.contains("{ {}, {} }"), "{ir}");
}

#[test]
fn pointer_casts_copy_the_bits() {
    let ir = ir_for(
        "fn to_int(*u8 p)->usize { return p as usize; } \
         fn to_ptr(usize n)->*u8 { return n as *u8; }",
    );
    assert!(ir.contains("ptrtoint ptr"), "{ir}");
    assert!(ir.contains("inttoptr i"), "{ir}");
    assert!(
        !ir.contains("assert.fail"),
        "pointer casts need no check:\n{ir}"
    );
}

#[test]
fn f16_and_bf16_convert_through_f32() {
    let ir = ir_for("fn f(f16 x)->bf16 { return x as bf16; }");
    assert!(ir.contains("fpext half"), "{ir}");
    assert!(ir.contains("fptrunc float"), "{ir}");
    assert!(ir.contains("to bfloat"), "{ir}");
}

/// Integer bounds beyond a float's range round to infinities, so the range
/// check must exclude `-inf` itself before the round trip, whose `fptosi`
/// would otherwise be poison.
#[test]
fn cast_checks_exclude_negative_infinity() {
    for source in [
        "fn f(f32 x)->i256 { return x as i256; }",
        "fn f(f16 x)->i32 { return x as i32; }",
        "fn f(i32 n)->f16 { return n as f16; }",
    ] {
        let ir = ir_for(source);
        assert!(
            ir.contains("%cast.above_neg_inf = fcmp ogt"),
            "{source}:\n{ir}"
        );
        assert!(ir.contains("%cast.in_range = and i1"), "{source}:\n{ir}");
    }
}

/// The relocation model reaches LLVM: PIC and PIE code record their level
/// in the module, as clang does.
#[test]
fn relocation_model_sets_module_pic_flags() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mir = build_mir_for_source("#[entry]\nfn main() {}\n");
    let ir_with = |relocation_model| {
        let path = dir.path().join("flags.ll");
        LlvmBackend
            .emit(
                &mir,
                &TargetSpec::host(),
                CodegenOptions {
                    optimization: 0,
                    relocation_model,
                },
                &[OutputRequest {
                    kind: ArtifactKind::LlvmIr,
                    path: path.clone(),
                }],
            )
            .expect("MIR should emit successfully");
        std::fs::read_to_string(&path).expect("LLVM IR should be written")
    };
    let pic = ir_with(RelocationModel::Pic);
    assert!(pic.contains(r#"!"PIC Level", i32 2"#), "{pic}");
    assert!(!pic.contains("PIE Level"), "{pic}");
    let pie = ir_with(RelocationModel::Pie);
    assert!(pie.contains(r#"!"PIC Level", i32 2"#), "{pie}");
    assert!(pie.contains(r#"!"PIE Level", i32 2"#), "{pie}");
    for model in [RelocationModel::Static, RelocationModel::DynamicNoPic] {
        let ir = ir_with(model);
        assert!(!ir.contains("PIC Level"), "{model:?}: {ir}");
    }
}

/// The compiler links the LLVM major version CI installs. `llvm-sys` does not
/// enforce it when LLVM is linked dynamically, and LLVM 22 aborts on
/// float/integer casts wider than 128 bits on non-x86 targets, so a
/// mismatched toolchain must fail here rather than in user programs.
#[test]
fn linked_llvm_is_version_23() {
    let (major, minor, patch) = inkwell::support::get_llvm_version();
    assert_eq!(
        major, 23,
        "xenonc is built for LLVM 23 but linked LLVM {major}.{minor}.{patch}"
    );
}

/// Hand-built MIR: `fn width(Rect r) -> f64 { return r.width; }`, with
/// `struct Rect { f64 width; f64 height; }` and an unused empty struct.
#[test]
fn structs_lower_to_named_llvm_types() {
    use std::sync::Arc;
    use xenonc::middle::mir::{
        BodyBuilder, FnDecl, FnSig, LocalDecl, Operand, Place, RETURN_PLACE, Rvalue, START_BLOCK,
        SourceInfo, TerminatorKind,
    };
    use xenonc::source::Span;
    use xenonc::types::{FieldDef, StructDef, StructTable, Type};

    let mut structs = StructTable::default();
    let field = |name: &str| FieldDef {
        name: name.to_owned(),
        ty: Type::Float64,
        span: Span::ZERO,
    };
    let rect = structs.push(StructDef {
        name: "Rect".into(),
        fields: vec![field("width"), field("height")],
        span: Span::ZERO,
    });
    structs.push(StructDef {
        name: "Empty".into(),
        fields: vec![],
        span: Span::ZERO,
    });
    let rect = structs.ty(rect);

    let mut program = MirProgram::new();
    program.set_structs(Arc::new(structs));
    let def_id = program.declare(FnDecl {
        name: "width".to_owned(),
        sig: FnSig {
            inputs: vec![rect.clone()],
            output: Type::Float64,
        },
        span: Span::ZERO,
    });
    let info = SourceInfo::outermost(Span::ZERO);
    let args = [LocalDecl::temp(rect, Span::ZERO)];
    let mut builder = BodyBuilder::new(def_id, Span::ZERO, Type::Float64, args);
    let param = xenonc::middle::mir::Local::from_u32(1);
    builder.push_assign(
        START_BLOCK,
        info,
        RETURN_PLACE.into(),
        Rvalue::Use(Operand::Copy(Place::from(param).field(0))),
    );
    builder.terminate(START_BLOCK, info, TerminatorKind::Return);
    program
        .set_body(builder.finish().expect("body should build"))
        .expect("body should attach");
    let target = TargetSpec::host();
    analyze_program(&mut program, &target).expect("MIR flow analysis should pass");
    let policy = RuntimeCheckPolicy {
        overflow: OverflowMode::Checked,
    };
    normalize_runtime_checks(&mut program, &target, policy).expect("should normalize");

    let dir = tempfile::tempdir().expect("tempdir");
    let ir = emit_ir(&program, &dir.path().join("out.ll"));
    assert!(ir.contains("%Rect = type { double, double }"), "{ir}");
    assert!(ir.contains("define double @width(%Rect %0)"), "{ir}");
    assert!(ir.contains("getelementptr inbounds"), "{ir}");
}

#[test]
fn struct_field_access_uses_struct_gep() {
    let ir = ir_for(
        "struct Rect { f64 width; f64 height; }\n\
         struct Node { i32 value; *Node next; }\n\
         fn area(Rect r) -> f64 { return r.width * r.height; }\n\
         fn make(f64 w) -> Rect { return Rect(height = 1.0, width = w); }\n\
         fn next(Node n) -> *Node { return n.next; }\n",
    );
    assert!(ir.contains("%Rect = type { double, double }"), "{ir}");
    assert!(ir.contains("%Node = type { i32, ptr }"), "{ir}");
    assert!(ir.contains("define double @area(%Rect %0)"), "{ir}");
    assert!(ir.contains("define %Rect @make(double %0)"), "{ir}");
    assert!(
        ir.contains("getelementptr inbounds nuw %Rect")
            || ir.contains("getelementptr inbounds %Rect"),
        "{ir}"
    );
    assert!(ir.contains("insertvalue %Rect"), "{ir}");
}

#[test]
fn methods_have_qualified_symbols() {
    let ir = ir_for(
        "struct Rect { f64 width; }\n\
         impl Rect {\n\
             fn scale(&self, f64 k) { self.width *= k; }\n\
             fn width_of(self) -> f64 { return self.width; }\n\
             fn unit() -> Rect { return Rect(width = 1.0); }\n\
         }\n\
         fn scale(f64 k) -> f64 { return k; }\n\
         fn f(Rect r) -> f64 { r.scale(scale(2.0)); return Rect.unit().width_of(); }\n",
    );
    assert!(
        ir.contains("define {} @Rect.scale(ptr %0, double %1)"),
        "{ir}"
    );
    assert!(
        ir.contains("define double @Rect.width_of(%Rect %0)"),
        "{ir}"
    );
    assert!(ir.contains("define %Rect @Rect.unit()"), "{ir}");
    assert!(ir.contains("define double @scale(double %0)"), "{ir}");
}

#[test]
fn getters_have_get_suffix_symbol() {
    let ir = ir_for(
        "struct Rect { f64 width; f64 height; }\n\
         impl Rect { f64 area { return self.width * self.height; } }\n\
         fn f(Rect r) -> f64 { return r.area; }\n",
    );
    assert!(ir.contains("define double @Rect.area.get(ptr %0)"), "{ir}");
    assert!(ir.contains("call double @Rect.area.get(ptr"), "{ir}");
    // A property takes no room in the struct.
    assert!(ir.contains("%Rect = type { double, double }"), "{ir}");
}

/// `struct S0 {}` and `struct Sn { S(n-1) a; S(n-1) b; }` up to `depth`.
fn doubling_empty_structs(depth: usize) -> String {
    let mut source = String::from("struct S0 {}\n");
    for level in 1..=depth {
        let inner = level - 1;
        source.push_str(&format!("struct S{level} {{ S{inner} a; S{inner} b; }}\n"));
    }
    source
}

#[test]
fn zero_sized_struct_trees_compile_without_being_expanded() {
    // `S60` has no data, but its fields written out are a tree of 2^60
    // leaves: nothing may walk it leaf by leaf.
    let source = format!(
        "{}\
         fn same(S60 a, S60 b) -> bool {{ return a == b; }}\n\
         fn different(S60 a, S60 b) -> bool {{ return a != b; }}\n\
         fn zero(S60 value, bool c) {{ while c {{ break value; }} }}\n\
         fn copy(S60 value) -> S60 {{ let S60 other = value; return other; }}\n",
        doubling_empty_structs(60)
    );
    let ir = ir_for(&source);
    // A struct without data is the empty LLVM struct, whatever its fields:
    // LLVM would otherwise walk them one by one.
    assert!(ir.contains("define i1 @same({} %0, {} %1)"), "{ir}");
    assert!(!ir.contains("%S60"), "{ir}");
    // Two values without data are equal, whatever their fields.
    assert!(ir.contains("store i1 true, ptr %_0"), "{ir}");
    // The zero value is one constant, not one per field.
    assert!(ir.contains("store {} zeroinitializer"), "{ir}");
}
