use std::path::{Path, PathBuf};

use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::values::FunctionValue;

use inkwell::IntPredicate;

use crate::ast::{BinOp, Expr, Function, Program, UnaryOp};

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
            Expr::BinOp { lhs, op, rhs } => {
                let lhs_val = self.codegen_expr(lhs)?;
                let rhs_val = self.codegen_expr(rhs)?;
                self.codegen_binop(op, lhs_val, rhs_val)
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.codegen_expr(operand)?;
                self.codegen_unaryop(op, val)
            }
        }
    }

    fn codegen_binop(
        &self,
        op: &BinOp,
        lhs: inkwell::values::IntValue<'ctx>,
        rhs: inkwell::values::IntValue<'ctx>,
    ) -> Result<inkwell::values::IntValue<'ctx>, String> {
        let i32t = self.context.i32_type();

        // Helper to zero-extend an i1 comparison result to i32.
        let cmp_to_i32 = |cmp: inkwell::values::IntValue<'ctx>, name: &str| {
            self.builder
                .build_int_z_extend(cmp, i32t, name)
                .map_err(|e| format!("build_int_z_extend({name}) failed: {e:?}"))
        };

        match op {
            BinOp::Add => self
                .builder
                .build_int_add(lhs, rhs, "add")
                .map_err(|e| format!("build_int_add failed: {e:?}")),
            BinOp::Sub => self
                .builder
                .build_int_sub(lhs, rhs, "sub")
                .map_err(|e| format!("build_int_sub failed: {e:?}")),
            BinOp::Mul => self
                .builder
                .build_int_mul(lhs, rhs, "mul")
                .map_err(|e| format!("build_int_mul failed: {e:?}")),
            BinOp::Div => self
                .builder
                .build_int_signed_div(lhs, rhs, "div")
                .map_err(|e| format!("build_int_signed_div failed: {e:?}")),
            BinOp::Mod => self
                .builder
                .build_int_signed_rem(lhs, rhs, "mod")
                .map_err(|e| format!("build_int_signed_rem failed: {e:?}")),
            BinOp::Pow => {
                // Integer exponentiation via a countdown loop:
                //   result = 1; while exp > 0 { result *= base; exp -= 1; }
                // Negative exponents on integers always yield 1 (integer truncation).
                let one = i32t.const_int(1, false);
                let zero = i32t.const_int(0, false);

                let pre_block = self
                    .builder
                    .get_insert_block()
                    .ok_or("pow: no current insert block")?;
                let current_fn = pre_block
                    .get_parent()
                    .ok_or("pow: insert block has no parent function")?;

                let loop_header = self.context.append_basic_block(current_fn, "pow_loop");
                let loop_body = self.context.append_basic_block(current_fn, "pow_body");
                let loop_exit = self.context.append_basic_block(current_fn, "pow_exit");

                // Fall into the loop header.
                self.builder
                    .build_unconditional_branch(loop_header)
                    .map_err(|e| format!("pow: branch to loop header failed: {e:?}"))?;

                // ── Loop header: phi nodes + loop-exit condition ──────────
                self.builder.position_at_end(loop_header);

                let result_phi = self
                    .builder
                    .build_phi(i32t, "pow_result")
                    .map_err(|e| format!("pow: build_phi(result) failed: {e:?}"))?;
                let exp_phi = self
                    .builder
                    .build_phi(i32t, "pow_exp")
                    .map_err(|e| format!("pow: build_phi(exp) failed: {e:?}"))?;

                let exp_val = exp_phi.as_basic_value().into_int_value();
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, exp_val, zero, "exp_gt_zero")
                    .map_err(|e| format!("pow: build_int_compare failed: {e:?}"))?;
                self.builder
                    .build_conditional_branch(cond, loop_body, loop_exit)
                    .map_err(|e| format!("pow: build_conditional_branch failed: {e:?}"))?;

                // ── Loop body: multiply and decrement ────────────────────
                self.builder.position_at_end(loop_body);

                let result_val = result_phi.as_basic_value().into_int_value();
                let new_result = self
                    .builder
                    .build_int_mul(result_val, lhs, "pow_mul")
                    .map_err(|e| format!("pow: build_int_mul failed: {e:?}"))?;
                let new_exp = self
                    .builder
                    .build_int_sub(exp_val, one, "pow_dec")
                    .map_err(|e| format!("pow: build_int_sub failed: {e:?}"))?;
                self.builder
                    .build_unconditional_branch(loop_header)
                    .map_err(|e| format!("pow: back-edge branch failed: {e:?}"))?;

                // Wire up phi incoming values.
                result_phi.add_incoming(&[(&one, pre_block), (&new_result, loop_body)]);
                exp_phi.add_incoming(&[(&rhs, pre_block), (&new_exp, loop_body)]);

                // ── Exit block ───────────────────────────────────────────
                self.builder.position_at_end(loop_exit);

                Ok(result_phi.as_basic_value().into_int_value())
            }
            BinOp::Eq => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, lhs, rhs, "eq")
                    .map_err(|e| format!("build_int_compare(EQ) failed: {e:?}"))?;
                cmp_to_i32(cmp, "eq_ext")
            }
            BinOp::NotEq => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, rhs, "ne")
                    .map_err(|e| format!("build_int_compare(NE) failed: {e:?}"))?;
                cmp_to_i32(cmp, "ne_ext")
            }
            BinOp::Lt => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, lhs, rhs, "lt")
                    .map_err(|e| format!("build_int_compare(SLT) failed: {e:?}"))?;
                cmp_to_i32(cmp, "lt_ext")
            }
            BinOp::Gt => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, lhs, rhs, "gt")
                    .map_err(|e| format!("build_int_compare(SGT) failed: {e:?}"))?;
                cmp_to_i32(cmp, "gt_ext")
            }
            BinOp::LtEq => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, lhs, rhs, "le")
                    .map_err(|e| format!("build_int_compare(SLE) failed: {e:?}"))?;
                cmp_to_i32(cmp, "le_ext")
            }
            BinOp::GtEq => {
                let cmp = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, lhs, rhs, "ge")
                    .map_err(|e| format!("build_int_compare(SGE) failed: {e:?}"))?;
                cmp_to_i32(cmp, "ge_ext")
            }
            BinOp::BitwiseAnd => self
                .builder
                .build_and(lhs, rhs, "and")
                .map_err(|e| format!("build_and failed: {e:?}")),
            BinOp::BitwiseOr => self
                .builder
                .build_or(lhs, rhs, "or")
                .map_err(|e| format!("build_or failed: {e:?}")),
            BinOp::BitwiseXor => self
                .builder
                .build_xor(lhs, rhs, "xor")
                .map_err(|e| format!("build_xor failed: {e:?}")),
            BinOp::LogicalAnd => {
                // Logical AND: result is 1 if both lhs and rhs are non-zero
                let zero = i32t.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(|e| format!("build_int_compare(lhs_nz) failed: {e:?}"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(|e| format!("build_int_compare(rhs_nz) failed: {e:?}"))?;
                let result = self
                    .builder
                    .build_and(lhs_nonzero, rhs_nonzero, "logical_and")
                    .map_err(|e| format!("build_and failed: {e:?}"))?;
                cmp_to_i32(result, "logical_and_ext")
            }
            BinOp::LogicalOr => {
                // Logical OR: result is 1 if either lhs or rhs is non-zero
                let zero = i32t.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(|e| format!("build_int_compare(lhs_nz) failed: {e:?}"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(|e| format!("build_int_compare(rhs_nz) failed: {e:?}"))?;
                let result = self
                    .builder
                    .build_or(lhs_nonzero, rhs_nonzero, "logical_or")
                    .map_err(|e| format!("build_or failed: {e:?}"))?;
                cmp_to_i32(result, "logical_or_ext")
            }
            BinOp::LogicalXor => {
                // Logical XOR: result is 1 if exactly one of lhs or rhs is non-zero
                let zero = i32t.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(|e| format!("build_int_compare(lhs_nz) failed: {e:?}"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(|e| format!("build_int_compare(rhs_nz) failed: {e:?}"))?;
                let result = self
                    .builder
                    .build_xor(lhs_nonzero, rhs_nonzero, "logical_xor")
                    .map_err(|e| format!("build_xor failed: {e:?}"))?;
                cmp_to_i32(result, "logical_xor_ext")
            }
            BinOp::LShift => self
                .builder
                .build_left_shift(lhs, rhs, "lshift")
                .map_err(|e| format!("build_left_shift failed: {e:?}")),
            BinOp::RShift => self
                .builder
                .build_right_shift(lhs, rhs, true, "rshift")
                .map_err(|e| format!("build_right_shift failed: {e:?}")),
        }
    }

    fn codegen_unaryop(
        &self,
        op: &UnaryOp,
        val: inkwell::values::IntValue<'ctx>,
    ) -> Result<inkwell::values::IntValue<'ctx>, String> {
        match op {
            // Two's-complement negation: 0 - val
            UnaryOp::Neg => self
                .builder
                .build_int_neg(val, "neg")
                .map_err(|e| format!("build_int_neg failed: {e:?}")),
            // Logical NOT: result is 1 if val is 0, else 0
            UnaryOp::Not => {
                let i32t = self.context.i32_type();
                let zero = i32t.const_int(0, false);
                let is_zero = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, val, zero, "is_zero")
                    .map_err(|e| format!("build_int_compare failed: {e:?}"))?;
                self.builder
                    .build_int_z_extend(is_zero, i32t, "not_ext")
                    .map_err(|e| format!("build_int_z_extend failed: {e:?}"))
            }
            // Bitwise complement: ~val
            UnaryOp::BitwiseNot => self
                .builder
                .build_not(val, "bitwise_not")
                .map_err(|e| format!("build_not failed: {e:?}")),
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

    // 4) Emit object file using write_to_file (TargetMachine API)
    tm.write_to_file(&module, FileType::Object, out_obj)
        .map_err(|e| format!("write_to_file(.o) failed: {e}"))?;

    Ok(())
}

pub fn default_output_paths(out_dir: &Path) -> (PathBuf, PathBuf) {
    let obj = out_dir.join("out.o");
    let ll = out_dir.join("out.ll");
    (obj, ll)
}
