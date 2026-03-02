use std::path::{Path, PathBuf};

use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::values::FunctionValue;

use crate::ast::{Expr, Function, Program};

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
        }
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        for f in &program.functions {
            self.compile_function(f)?;
        }
        Ok(self.module)
    }

    fn compile_function(&mut self, f: &Function) -> Result<FunctionValue<'ctx>, String> {
        // MVP type mapping:
        // "u32" -> i32 (close enough for now; refine later)
        let ret_ty = match f.return_type.as_str() {
            "u32" | "i32" => self.context.i32_type(),
            other => return Err(format!("Unsupported return type for MVP: {other}")),
        };

        // MVP: no params
        let fn_ty = ret_ty.fn_type(&[], false);
        let fn_val = self.module.add_function(&f.name, fn_ty, None);

        let entry = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry);

        // MVP body: must contain exactly one return
        for expr in &f.body {
            match expr {
                Expr::Return(inner) => {
                    let value = self.codegen_expr(inner)?;
                    self.builder
                        .build_return(Some(&value))
                        .map_err(|e| format!("build_return failed: {e:?}"))?;
                }
                other => return Err(format!("Unsupported statement in MVP: {other:?}")),
            }
        }

        Ok(fn_val)
    }

    fn codegen_expr(&self, e: &Expr) -> Result<inkwell::values::IntValue<'ctx>, String> {
        let i32t = self.context.i32_type();

        match e {
            Expr::Int(v) => Ok(i32t.const_int(*v as u64, true)),
            Expr::Ident(name) => Err(format!("MVP: unknown identifier '{name}' (no vars yet)")),
            Expr::Return(_) => Err("MVP: nested return not allowed".into()),
        }
    }
}

pub fn emit_object_and_ir(
    program: &Program,
    out_obj: &Path,
    out_ll: Option<&Path>,
) -> Result<(), String> {
    // 1) Init target (native backend)
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|msg| format!("initialize_native failed: {msg}"))?;

    // 2) Build IR module
    let context = Context::create();
    let cg = CodeGen::new(&context, "xenon_mvp");
    let module = cg.compile_program(program)?;

    // Optional: write LLVM IR text for debugging
    if let Some(ll_path) = out_ll {
        module
            .print_to_file(ll_path)
            .map_err(|e| format!("print_to_file(.ll) failed: {e}"))?;
    }

    // 3) Configure triple + target machine
    let triple = TargetMachine::get_default_triple();
    module.set_triple(&triple);

    let target =
        Target::from_triple(&triple).map_err(|e| format!("Target::from_triple failed: {e}"))?;

    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();

    let tm = target
        .create_target_machine(
            &triple,
            cpu.as_str(),
            features.as_str(),
            OptimizationLevel::None,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or("create_target_machine returned None")?;

    // 4) Emit object file using write_to_file (TargetMachine API) :contentReference[oaicite:3]{index=3}
    tm.write_to_file(&module, FileType::Object, out_obj)
        .map_err(|e| format!("write_to_file(.o) failed: {e}"))?;

    Ok(())
}

pub fn default_output_paths(out_dir: &Path) -> (PathBuf, PathBuf) {
    let obj = out_dir.join("out.o");
    let ll = out_dir.join("out.ll");
    (obj, ll)
}
