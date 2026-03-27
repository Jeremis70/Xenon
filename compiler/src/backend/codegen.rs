use crate::frontend::ast::Type;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetData, TargetMachine,
};
use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::values::{FunctionValue, PointerValue};

use inkwell::IntPredicate;

use crate::error::{CodegenError, CodegenResult};
use crate::frontend::ast::{BinOp, Expr, Function, Program, Stmt, UnaryOp};

/// Maps an inkwell builder error to [`CodegenError::LlvmBuilder`].
macro_rules! llvm_err {
    ($op:literal) => {
        |e| CodegenError::LlvmBuilder {
            operation: $op,
            message: format!("{e:?}"),
        }
    };
}

/// Tracks the LLVM blocks and result slot for a single loop nesting level.
struct LoopFrame<'ctx> {
    continue_bb: BasicBlock<'ctx>,
    break_bb: BasicBlock<'ctx>,
    /// Alloca slot that `break <expr>;` stores into; loaded at `loop_after`.
    result_slot: PointerValue<'ctx>,
}

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    variables: HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>, Type)>,
    /// Stack of frames pushed when entering a loop, popped on exit.
    loop_stack: Vec<LoopFrame<'ctx>>,
    /// The function currently being compiled; set at the start of
    /// `compile_function` so that `Expr::Loop` in expression position can
    /// access the function value and return type without threading extra
    /// parameters through `codegen_expr`.
    current_fn: Option<(FunctionValue<'ctx>, BasicTypeEnum<'ctx>)>,
    /// Data layout of the compilation target. Used to resolve `usize`/`isize`
    /// to their pointer-sized LLVM integer type via `ptr_sized_int_type`.
    target_data: TargetData,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str, target_data: TargetData) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            variables: HashMap::new(),
            loop_stack: Vec::new(),
            current_fn: None,
            target_data,
        }
    }

    fn llvm_type(&self, ty: &crate::frontend::ast::Type) -> CodegenResult<BasicTypeEnum<'ctx>> {
        Ok(match ty {
            Type::Bool => self.context.bool_type().into(),
            Type::Int(w) | Type::UInt(w) => self
                .context
                .custom_width_int_type(std::num::NonZero::new(*w).expect("bit width is non-zero"))
                .expect("custom_width_int_type failed")
                .into(),
            // usize/isize lower to the pointer-sized integer type for this target.
            // ptr_sized_int_type queries the data layout directly, so it's always
            // correct for any target triple the user passes at compile time.
            Type::USize | Type::ISize => self
                .context
                .ptr_sized_int_type(&self.target_data, None)
                .into(),
            Type::Float16 => self.context.f16_type().into(),
            Type::BFloat16 => self.context.bf16_type().into(),
            Type::Float32 => self.context.f32_type().into(),
            Type::Float64 => self.context.f64_type().into(),
            Type::Float128 => self.context.f128_type().into(),
        })
    }

    fn collect_param_types(&self, f: &Function) -> CodegenResult<Vec<BasicTypeEnum<'ctx>>> {
        f.params
            .iter()
            .map(|p| self.llvm_type(&p.ty))
            .collect::<CodegenResult<_>>()
    }

    pub fn compile_program(mut self, program: &Program) -> CodegenResult<Module<'ctx>> {
        // Declare all functions
        for f in &program.functions {
            self.declare_function(f)?;
        }
        // Then compile function bodies
        for f in &program.functions {
            self.compile_function(f)?;
        }
        Ok(self.module)
    }

    /// Registers a function signature without compiling yet
    fn declare_function(&self, f: &Function) -> CodegenResult<FunctionValue<'ctx>> {
        if let Some(existing) = self.module.get_function(&f.name) {
            return Ok(existing);
        }
        let ret_ty = self.llvm_type(&f.return_type.ty)?;
        let param_types: Vec<BasicTypeEnum> = self.collect_param_types(f)?;
        let param_metadata: Vec<inkwell::types::BasicMetadataTypeEnum> =
            param_types.iter().map(|&t| t.into()).collect();
        let fn_ty = ret_ty.fn_type(&param_metadata, false);
        Ok(self.module.add_function(&f.name, fn_ty, None))
    }

    fn compile_function(&mut self, f: &Function) -> CodegenResult<FunctionValue<'ctx>> {
        // Reuse the forward declaration emitted by declare_function.
        let fn_val = self
            .module
            .get_function(&f.name)
            .expect("declare_function must be called before compile_function");

        let param_types: Vec<BasicTypeEnum> = self.collect_param_types(f)?;
        let ret_ty = self.llvm_type(&f.return_type.ty)?;
        self.current_fn = Some((fn_val, ret_ty));
        let entry = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry);

        self.variables.clear();

        // Allocate a stack slot for each parameter and store the incoming value.
        for (i, param) in f.params.iter().enumerate() {
            let llvm_ty = param_types[i];
            let param_name = param.name.as_deref().unwrap_or("");
            let alloca = self
                .builder
                .build_alloca(llvm_ty, param_name)
                .map_err(llvm_err!("build_alloca (param)"))?;
            let incoming = fn_val
                .get_nth_param(i as u32)
                .ok_or(CodegenError::InvalidIrState("missing param value"))?
                .into_int_value();
            self.builder
                .build_store(alloca, incoming)
                .map_err(llvm_err!("build_store (param)"))?;
            self.variables.insert(
                param.name.clone().unwrap_or_default(),
                (alloca, llvm_ty, param.ty.clone()),
            );
        }

        // If the return type has a named binding (e.g. `-> u32 sum`), allocate a
        // stack slot for it and zero-initialise it so that it can be used as a
        // regular variable inside the body and returned implicitly.
        if let Some(ret_name) = &f.return_type.name {
            let BasicTypeEnum::IntType(int_ret_ty) = ret_ty else {
                return Err(CodegenError::InvalidIrState(
                    "named return variable is only supported for integer types",
                ));
            };
            let alloca = self
                .builder
                .build_alloca(ret_ty, ret_name)
                .map_err(llvm_err!("build_alloca (named return)"))?;
            let zero = int_ret_ty.const_zero();
            self.builder
                .build_store(alloca, zero)
                .map_err(llvm_err!("build_store (named return init)"))?;
            self.variables
                .insert(ret_name.clone(), (alloca, ret_ty, f.return_type.ty.clone()));
        }

        for stmt in &f.body {
            // compile_stmt returns true when the current block has been terminated
            // (e.g. a return was emitted); stop processing the remaining statements.
            if self.compile_stmt(stmt, fn_val, ret_ty)? {
                break;
            }
        }

        // If the body didn't end with an explicit `return`, either emit an
        // implicit return from the named return variable, or report an error.
        let current_block = self
            .builder
            .get_insert_block()
            .ok_or(CodegenError::InvalidIrState("no insert block after body"))?;
        if current_block.get_terminator().is_none() {
            let Some(ret_name) = &f.return_type.name else {
                return Err(CodegenError::MissingReturn {
                    name: f.name.clone(),
                });
            };
            let (ptr, ty, _) =
                self.variables
                    .get(ret_name.as_str())
                    .ok_or(CodegenError::InvalidIrState(
                        "named return variable missing from variable map",
                    ))?;
            let val = self
                .builder
                .build_load(*ty, *ptr, ret_name)
                .map_err(llvm_err!("build_load (implicit return)"))?
                .into_int_value();
            let val = self.cast_int_to_type(val, ret_ty, false)?;
            self.builder
                .build_return(Some(&val))
                .map_err(llvm_err!("build_return (implicit)"))?;
        }

        Ok(fn_val)
    }

    /// Compiles a single statement. Returns `true` when the current basic block
    /// has been terminated (a `return` was emitted), signalling the caller to
    /// stop processing further statements in the same scope.
    fn compile_stmt(
        &mut self,
        stmt: &Stmt,
        fn_val: FunctionValue<'ctx>,
        ret_ty: BasicTypeEnum<'ctx>,
    ) -> CodegenResult<bool> {
        match stmt {
            Stmt::Return(inner) => {
                let value = self.codegen_expr(inner)?;
                let value = self.cast_int_to_type(value, ret_ty, false)?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(llvm_err!("build_return"))?;
                Ok(true)
            }
            Stmt::VarDecl(binding) => {
                let llvm_ty = self.llvm_type(&binding.ty)?;
                let var_name = binding.name.as_deref().unwrap_or("_");
                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, var_name)
                    .map_err(llvm_err!("build_alloca"))?;
                self.variables.insert(
                    binding.name.clone().unwrap_or_default(),
                    (alloca, llvm_ty, binding.ty.clone()),
                );
                let init_val = self.codegen_expr(
                    binding
                        .default
                        .as_ref()
                        .expect("VarDecl binding must have a default"),
                )?;
                let is_unsigned = matches!(binding.ty, Type::UInt(_) | Type::USize);
                let init_val = self.cast_int_to_type(init_val, llvm_ty, is_unsigned)?;
                self.builder
                    .build_store(alloca, init_val)
                    .map_err(llvm_err!("build_store"))?;
                Ok(false)
            }
            Stmt::Assign { name, value } => {
                let (var_ptr, var_ty, is_unsigned) = {
                    let (ptr, ty, ast_ty) = self
                        .variables
                        .get(name)
                        .ok_or_else(|| CodegenError::UndefinedVariable { name: name.clone() })?;
                    (*ptr, *ty, matches!(ast_ty, Type::UInt(_) | Type::USize))
                };
                let val = self.codegen_expr(value)?;
                let val = self.cast_int_to_type(val, var_ty, is_unsigned)?;
                self.builder
                    .build_store(var_ptr, val)
                    .map_err(llvm_err!("build_store"))?;
                Ok(false)
            }
            Stmt::Expr(expr) => {
                self.codegen_expr(expr)?;
                // An expression like an infinite loop may have terminated the
                // current block; stop emitting further statements if so.
                let terminated = self
                    .builder
                    .get_insert_block()
                    .is_some_and(|b| b.get_terminator().is_some());
                Ok(terminated)
            }
            Stmt::Break(opt_expr) => {
                let (break_bb, result_slot) = {
                    let frame = self.loop_stack.last().ok_or(CodegenError::InvalidIrState(
                        "`break` used outside of a loop",
                    ))?;
                    (frame.break_bb, frame.result_slot)
                };
                if let Some(expr) = opt_expr {
                    let val = self.codegen_expr(expr)?;
                    let val = self.cast_int_to_type(val, self.context.i64_type().into(), false)?;
                    self.builder
                        .build_store(result_slot, val)
                        .map_err(llvm_err!("build_store (break value)"))?;
                }
                self.builder
                    .build_unconditional_branch(break_bb)
                    .map_err(llvm_err!("build_unconditional_branch (break)"))?;
                Ok(true)
            }
            Stmt::Continue => {
                let continue_bb = self
                    .loop_stack
                    .last()
                    .ok_or(CodegenError::InvalidIrState(
                        "`continue` used outside of a loop",
                    ))?
                    .continue_bb;
                self.builder
                    .build_unconditional_branch(continue_bb)
                    .map_err(llvm_err!("build_unconditional_branch (continue)"))?;
                Ok(true)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.codegen_expr(condition)?;
                // If the condition is already a 1-bit integer (e.g. the result of a
                // comparison), use it directly; otherwise treat non-zero as true.
                let is_true = if cond_val.get_type().get_bit_width() == 1 {
                    cond_val
                } else {
                    let zero = cond_val.get_type().const_zero();
                    self.builder
                        .build_int_compare(IntPredicate::NE, cond_val, zero, "if_cond")
                        .map_err(llvm_err!("build_int_compare"))?
                };

                let then_block = self.context.append_basic_block(fn_val, "then");
                let else_block = self.context.append_basic_block(fn_val, "else");

                self.builder
                    .build_conditional_branch(is_true, then_block, else_block)
                    .map_err(llvm_err!("build_conditional_branch"))?;

                // Then branch
                self.builder.position_at_end(then_block);
                for s in then_branch {
                    if self.compile_stmt(s, fn_val, ret_ty)? {
                        break;
                    }
                }
                let then_end =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no insert block after then branch",
                        ))?;
                let then_falls_through = then_end.get_terminator().is_none();

                // Else branch
                self.builder.position_at_end(else_block);
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        if self.compile_stmt(s, fn_val, ret_ty)? {
                            break;
                        }
                    }
                }
                let else_end =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no insert block after else branch",
                        ))?;
                let else_falls_through = else_end.get_terminator().is_none();

                // If no branch falls through there is no merge point: all paths
                // are terminated. Don't create a merge block — the function is
                // already correct and this avoids dead blocks in the IR.
                if !then_falls_through && !else_falls_through {
                    return Ok(true);
                }

                // At least one branch falls through: create the merge block and
                // wire the open branches to it.
                let merge_block = self.context.append_basic_block(fn_val, "if_merge");
                if then_falls_through {
                    self.builder.position_at_end(then_end);
                    self.builder
                        .build_unconditional_branch(merge_block)
                        .map_err(llvm_err!("build_unconditional_branch (then->merge)"))?;
                }
                if else_falls_through {
                    self.builder.position_at_end(else_end);
                    self.builder
                        .build_unconditional_branch(merge_block)
                        .map_err(llvm_err!("build_unconditional_branch (else->merge)"))?;
                }

                // Continue from the merge block
                self.builder.position_at_end(merge_block);
                Ok(false)
            }
        }
    }

    /// Casts `val` to `target` via truncation or sign/zero-extension as needed.
    /// When `is_unsigned` is true, widening uses zero-extension; otherwise
    /// sign-extension. If the bit widths are already equal no instruction is
    /// emitted.
    fn cast_int_to_type(
        &self,
        val: inkwell::values::IntValue<'ctx>,
        target: BasicTypeEnum<'ctx>,
        is_unsigned: bool,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        let BasicTypeEnum::IntType(target_ty) = target else {
            return Ok(val);
        };
        match val
            .get_type()
            .get_bit_width()
            .cmp(&target_ty.get_bit_width())
        {
            std::cmp::Ordering::Equal => Ok(val),
            std::cmp::Ordering::Greater => self
                .builder
                .build_int_truncate(val, target_ty, "trunc")
                .map_err(llvm_err!("build_int_truncate")),
            std::cmp::Ordering::Less if is_unsigned => self
                .builder
                .build_int_z_extend(val, target_ty, "zext")
                .map_err(llvm_err!("build_int_z_extend")),
            std::cmp::Ordering::Less => self
                .builder
                .build_int_s_extend(val, target_ty, "sext")
                .map_err(llvm_err!("build_int_s_extend")),
        }
    }

    fn codegen_expr(&mut self, e: &Expr) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        match e {
            Expr::Int(v) => Ok(self.context.i64_type().const_int(*v as u64, true)),
            Expr::Ident(name) => {
                let &(ref ptr, ty, ref _ast_ty) = self
                    .variables
                    .get(name)
                    .ok_or_else(|| CodegenError::UndefinedVariable { name: name.clone() })?;
                self.builder
                    .build_load(ty, *ptr, name.as_str())
                    .map_err(llvm_err!("build_load"))
                    .map(|v| v.into_int_value())
            }
            Expr::BinOp { lhs, op, rhs } => {
                let lhs_unsigned = self.infer_expr_unsigned(lhs);
                let rhs_unsigned = self.infer_expr_unsigned(rhs);
                let lhs_val = self.codegen_expr(lhs)?;
                let rhs_val = self.codegen_expr(rhs)?;
                self.codegen_binop(op, lhs_val, rhs_val, lhs_unsigned, rhs_unsigned)
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.codegen_expr(operand)?;
                self.codegen_unaryop(op, val)
            }
            Expr::Call { name, args } => {
                let callee = self
                    .module
                    .get_function(name)
                    .ok_or_else(|| CodegenError::UndefinedFunction { name: name.clone() })?;
                let expected = callee.count_params() as usize;
                if args.len() != expected {
                    return Err(CodegenError::ArgumentCountMismatch {
                        name: name.clone(),
                        expected,
                        got: args.len(),
                    });
                }
                let mut compiled_args: Vec<inkwell::values::BasicMetadataValueEnum> =
                    Vec::with_capacity(args.len());
                for (arg_expr, param) in args.iter().zip(callee.get_param_iter()) {
                    let val = self.codegen_expr(arg_expr)?;
                    let target = param.get_type();
                    compiled_args.push(self.cast_int_to_type(val, target, false)?.into());
                }
                let call = self
                    .builder
                    .build_call(callee, &compiled_args, "call")
                    .map_err(llvm_err!("build_call"))?;
                let ret = call
                    .try_as_basic_value()
                    .basic()
                    .ok_or(CodegenError::InvalidIrState("call returned void"))?;
                Ok(ret.into_int_value())
            }
            Expr::IfElse {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.codegen_expr(condition)?;
                // Use the condition directly when it is already i1, otherwise
                // emit icmp ne ..., zero to convert to a branch predicate.
                let cond_i1 = if cond_val.get_type().get_bit_width() == 1 {
                    cond_val
                } else {
                    let zero = cond_val.get_type().const_zero();
                    self.builder
                        .build_int_compare(IntPredicate::NE, cond_val, zero, "if_cond")
                        .map_err(llvm_err!("build_int_compare"))?
                };

                // Lower to control flow so only the taken branch executes.
                // This preserves side-effect semantics and avoids type mismatches
                // in build_select when the two branches have different bit-widths.
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no current basic block for if-expression",
                        ))?;
                let parent_fn = current_block
                    .get_parent()
                    .ok_or(CodegenError::InvalidIrState(
                        "if-expression outside of function",
                    ))?;

                let then_bb = self.context.append_basic_block(parent_fn, "ife_then");
                let else_bb = self.context.append_basic_block(parent_fn, "ife_else");
                let merge_bb = self.context.append_basic_block(parent_fn, "ife_merge");

                self.builder
                    .build_conditional_branch(cond_i1, then_bb, else_bb)
                    .map_err(llvm_err!("build_conditional_branch"))?;

                // Then branch
                self.builder.position_at_end(then_bb);
                let then_val_raw = self.codegen_expr(then_branch)?;
                // Re-read the insert block: codegen_expr may have appended blocks.
                let then_end_bb =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no block after then expression",
                        ))?;

                // Else branch — evaluate before emitting any branches so we
                // know both widths and can pick the common type.
                self.builder.position_at_end(else_bb);
                let else_val_raw = self.codegen_expr(else_branch)?;
                let else_end_bb =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no block after else expression",
                        ))?;

                // Cast both branches to the wider of the two types so the phi
                // node is always well-typed, even when branches yield i64 literals
                // and narrower variables.
                let common_ty = if then_val_raw.get_type().get_bit_width()
                    >= else_val_raw.get_type().get_bit_width()
                {
                    then_val_raw.get_type()
                } else {
                    else_val_raw.get_type()
                };
                // Emit the widening cast for `then` at the end of then_end_bb
                // (before its branch terminator).
                self.builder.position_at_end(then_end_bb);
                let then_val = self.cast_int_to_type(then_val_raw, common_ty.into(), false)?;
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(llvm_err!("build_unconditional_branch (ife then->merge)"))?;

                // Emit the widening cast for `else` at the end of else_end_bb.
                self.builder.position_at_end(else_end_bb);
                let else_val = self.cast_int_to_type(else_val_raw, common_ty.into(), false)?;
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(llvm_err!("build_unconditional_branch (ife else->merge)"))?;

                // Merge block: phi picks the value from whichever branch ran.
                self.builder.position_at_end(merge_bb);
                let phi = self
                    .builder
                    .build_phi(common_ty.as_basic_type_enum(), "ife_result")
                    .map_err(llvm_err!("build_phi"))?;
                phi.add_incoming(&[(&then_val, then_end_bb), (&else_val, else_end_bb)]);
                Ok(phi.as_basic_value().into_int_value())
            }
            Expr::Loop { body } => self.compile_loop(body, None, false, false),
            Expr::CondLoop {
                post,
                inverted,
                condition,
                body,
            } => self.compile_loop(body, Some(condition), *post, *inverted),
        }
    }

    fn compile_loop(
        &mut self,
        body: &[Stmt],
        condition: Option<&Expr>,
        post: bool,
        inverted: bool,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        let (fn_val, ret_ty) = self.current_fn.ok_or(CodegenError::InvalidIrState(
            "loop expression compiled outside of a function",
        ))?;

        // Allocate a result slot upfront so break can store into it
        let result_slot = self
            .builder
            .build_alloca(self.context.i64_type(), "loop_result")
            .map_err(llvm_err!("build_alloca (loop result)"))?;

        // Initialize the result slot to a well-defined default value.
        // This ensures that if the loop exits without writing a value
        // (e.g. via `break;` with no value or a false condition), the
        // subsequent load in `loop_after` reads a defined value rather
        // than uninitialized memory.
        let loop_result_default = self.context.i64_type().const_zero();
        self.builder
            .build_store(result_slot, loop_result_default)
            .map_err(llvm_err!("build_store (loop result default)"))?;
        // Create blocks in execution order for readable IR output.
        // Pre condition if there is one
        let loop_cond_pre = if condition.is_some() && !post {
            Some(self.context.append_basic_block(fn_val, "loop_cond"))
        } else {
            None
        };
        // Body block
        let loop_body = self.context.append_basic_block(fn_val, "loop_body");

        // Post condition if there is one
        let loop_cond_post = if condition.is_some() && post {
            Some(self.context.append_basic_block(fn_val, "loop_cond"))
        } else {
            None
        };

        // What to do after the loop exits
        let loop_after = self.context.append_basic_block(fn_val, "loop_after");

        // Unify loop conditions into a single block
        let loop_cond = loop_cond_pre.or(loop_cond_post);
        // What to do when the loop body finishes or a `continue` is hit
        let continue_bb = loop_cond.unwrap_or(loop_body);

        // Entry branch: either the pre-condition or the body.
        let entry_bb = if condition.is_some() && !post {
            continue_bb
        } else {
            loop_body
        };
        self.builder
            .build_unconditional_branch(entry_bb)
            .map_err(llvm_err!("build_unconditional_branch (loop entry)"))?;

        // Emit the pre-condition (while / until).
        if let (Some(cond_bb), Some(expr)) = (loop_cond_pre, condition) {
            self.builder.position_at_end(cond_bb);
            self.emit_loop_condition(expr, inverted, loop_body, loop_after)?;
        }

        // Compile the body.
        self.builder.position_at_end(loop_body);
        self.loop_stack.push(LoopFrame {
            continue_bb,
            break_bb: loop_after,
            result_slot,
        });
        for s in body {
            if self.compile_stmt(s, fn_val, ret_ty)? {
                break;
            }
        }
        self.loop_stack.pop();

        // If the body falls through (no terminator), branch to the back-edge
        // target: condition re-evaluation when one exists, or body top.
        let body_end = self
            .builder
            .get_insert_block()
            .ok_or(CodegenError::InvalidIrState("no block after loop body"))?;
        if body_end.get_terminator().is_none() {
            self.builder
                .build_unconditional_branch(continue_bb)
                .map_err(llvm_err!("build_unconditional_branch (loop back)"))?;
        }

        // Emit the post-condition (do-while / do-until).
        if let (Some(cond_bb), Some(expr)) = (loop_cond_post, condition) {
            self.builder.position_at_end(cond_bb);
            self.emit_loop_condition(expr, inverted, loop_body, loop_after)?;
        }

        // If nothing ever branched to loop_after the loop is infinite.
        // Remove the orphaned block entirely so it never appears in the IR.
        if loop_after.get_first_use().is_none() {
            loop_after
                .remove_from_function()
                .map_err(|()| CodegenError::InvalidIrState("failed to remove loop_after block"))?;
            // Return a dummy value; this path is never reachable.
            return Ok(self.context.i64_type().const_zero());
        }

        // Load the result from the result slot and return it.
        self.builder.position_at_end(loop_after);
        self.builder
            .build_load(self.context.i64_type(), result_slot, "loop_val")
            .map_err(llvm_err!("build_load (loop result)"))
            .map(|v| v.into_int_value())
    }

    fn emit_loop_condition(
        &mut self,
        condition: &Expr,
        inverted: bool,
        body_bb: BasicBlock<'ctx>,
        after_bb: BasicBlock<'ctx>,
    ) -> CodegenResult<()> {
        let cond_val = self.codegen_expr(condition)?;

        let cond_i1 = if cond_val.get_type().get_bit_width() == 1 {
            cond_val
        } else {
            self.builder
                .build_int_compare(
                    IntPredicate::NE,
                    cond_val,
                    cond_val.get_type().const_zero(),
                    "loop_cond_ne",
                )
                .map_err(llvm_err!("build_int_compare (loop condition)"))?
        };
        // `inverted`: condition is true → exit (until), false → continue.
        // `normal`:   condition is true → continue (while), false → exit.
        let (true_bb, false_bb) = if inverted {
            (after_bb, body_bb)
        } else {
            (body_bb, after_bb)
        };
        self.builder
            .build_conditional_branch(cond_i1, true_bb, false_bb)
            .map_err(llvm_err!("build_conditional_branch (loop condition)"))?;
        Ok(())
    }

    /// Returns `true` when the expression is known to produce an unsigned value.
    /// Integer literals are considered sign-neutral (`false`), so the other
    /// operand in a binary operation decides the signedness.
    fn infer_expr_unsigned(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(name) => self
                .variables
                .get(name.as_str())
                .is_some_and(|(_, _, ty)| matches!(ty, Type::UInt(_) | Type::USize)),
            Expr::Int(_) => false,
            Expr::BinOp { lhs, .. } => self.infer_expr_unsigned(lhs),
            Expr::UnaryOp { operand, .. } => self.infer_expr_unsigned(operand),
            Expr::IfElse { then_branch, .. } => self.infer_expr_unsigned(then_branch),
            Expr::Call { .. } | Expr::Loop { .. } | Expr::CondLoop { .. } => false,
        }
    }

    fn codegen_binop(
        &self,
        op: &BinOp,
        lhs: inkwell::values::IntValue<'ctx>,
        rhs: inkwell::values::IntValue<'ctx>,
        lhs_unsigned: bool,
        rhs_unsigned: bool,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        // Normalize operand widths. When one side is i1 (the result of a
        // comparison used in arithmetic like `(x == 0) + 2`), promote it to
        // the other side's width so we never accidentally truncate an integer
        // down to 1 bit. For other mismatches the narrower operand is extended
        // to the wider type (typically a literal stays at i64 and the variable
        // is widened) so no information is lost.
        let (lhs, rhs) = {
            let lw = lhs.get_type().get_bit_width();
            let rw = rhs.get_type().get_bit_width();
            if lw == rw {
                (lhs, rhs)
            } else if lw == 1 {
                // Boolean result on the left: promote it to the integer's width.
                let lhs = self.cast_int_to_type(lhs, rhs.get_type().into(), true)?;
                (lhs, rhs)
            } else if rw == 1 {
                // Boolean result on the right: promote it to the integer's width.
                let rhs = self.cast_int_to_type(rhs, lhs.get_type().into(), true)?;
                (lhs, rhs)
            } else if lw < rw {
                // Extend the narrower (lhs) to the wider type (rhs).
                let lhs = self.cast_int_to_type(lhs, rhs.get_type().into(), lhs_unsigned)?;
                (lhs, rhs)
            } else {
                // lw > rw: extend the narrower (rhs) to the wider type (lhs).
                let rhs = self.cast_int_to_type(rhs, lhs.get_type().into(), rhs_unsigned)?;
                (lhs, rhs)
            }
        };
        let wt = lhs.get_type();

        match op {
            BinOp::Add => self
                .builder
                .build_int_add(lhs, rhs, "add")
                .map_err(llvm_err!("build_int_add")),
            BinOp::Sub => self
                .builder
                .build_int_sub(lhs, rhs, "sub")
                .map_err(llvm_err!("build_int_sub")),
            BinOp::Mul => self
                .builder
                .build_int_mul(lhs, rhs, "mul")
                .map_err(llvm_err!("build_int_mul")),
            BinOp::Div if lhs_unsigned => self
                .builder
                .build_int_unsigned_div(lhs, rhs, "udiv")
                .map_err(llvm_err!("build_int_unsigned_div")),
            BinOp::Div => self
                .builder
                .build_int_signed_div(lhs, rhs, "sdiv")
                .map_err(llvm_err!("build_int_signed_div")),
            BinOp::Mod if lhs_unsigned => self
                .builder
                .build_int_unsigned_rem(lhs, rhs, "urem")
                .map_err(llvm_err!("build_int_unsigned_rem")),
            BinOp::Mod => self
                .builder
                .build_int_signed_rem(lhs, rhs, "srem")
                .map_err(llvm_err!("build_int_signed_rem")),
            BinOp::Pow => {
                // Integer exponentiation via a countdown loop:
                //   result = 1; while exp > 0 { result *= base; exp -= 1; }
                // Negative exponents on integers always yield 1 (integer truncation).
                let one = wt.const_int(1, false);
                let zero = wt.const_int(0, false);

                let pre_block = self
                    .builder
                    .get_insert_block()
                    .ok_or(CodegenError::InvalidIrState("pow: no current insert block"))?;
                let current_fn = pre_block.get_parent().ok_or(CodegenError::InvalidIrState(
                    "pow: insert block has no parent function",
                ))?;

                let loop_header = self.context.append_basic_block(current_fn, "pow_loop");
                let loop_body = self.context.append_basic_block(current_fn, "pow_body");
                let loop_exit = self.context.append_basic_block(current_fn, "pow_exit");

                // Fall into the loop header.
                self.builder
                    .build_unconditional_branch(loop_header)
                    .map_err(llvm_err!("build_unconditional_branch"))?;

                // ── Loop header: phi nodes + loop-exit condition ──────────
                self.builder.position_at_end(loop_header);

                let result_phi = self
                    .builder
                    .build_phi(wt, "pow_result")
                    .map_err(llvm_err!("build_phi"))?;
                let exp_phi = self
                    .builder
                    .build_phi(wt, "pow_exp")
                    .map_err(llvm_err!("build_phi"))?;

                let exp_val = exp_phi.as_basic_value().into_int_value();
                let exp_pred = if lhs_unsigned {
                    IntPredicate::UGT
                } else {
                    IntPredicate::SGT
                };
                let cond = self
                    .builder
                    .build_int_compare(exp_pred, exp_val, zero, "exp_gt_zero")
                    .map_err(llvm_err!("build_int_compare"))?;
                self.builder
                    .build_conditional_branch(cond, loop_body, loop_exit)
                    .map_err(llvm_err!("build_conditional_branch"))?;

                // ── Loop body: multiply and decrement ────────────────────
                self.builder.position_at_end(loop_body);

                let result_val = result_phi.as_basic_value().into_int_value();
                let new_result = self
                    .builder
                    .build_int_mul(result_val, lhs, "pow_mul")
                    .map_err(llvm_err!("build_int_mul"))?;
                let new_exp = self
                    .builder
                    .build_int_sub(exp_val, one, "pow_dec")
                    .map_err(llvm_err!("build_int_sub"))?;
                self.builder
                    .build_unconditional_branch(loop_header)
                    .map_err(llvm_err!("build_unconditional_branch"))?;

                // Wire up phi incoming values.
                result_phi.add_incoming(&[(&one, pre_block), (&new_result, loop_body)]);
                exp_phi.add_incoming(&[(&rhs, pre_block), (&new_exp, loop_body)]);

                // ── Exit block ───────────────────────────────────────────
                self.builder.position_at_end(loop_exit);

                Ok(result_phi.as_basic_value().into_int_value())
            }

            BinOp::Eq => self
                .builder
                .build_int_compare(IntPredicate::EQ, lhs, rhs, "eq")
                .map_err(llvm_err!("build_int_compare")),
            BinOp::NotEq => self
                .builder
                .build_int_compare(IntPredicate::NE, lhs, rhs, "ne")
                .map_err(llvm_err!("build_int_compare")),
            BinOp::Lt => {
                let pred = if lhs_unsigned {
                    IntPredicate::ULT
                } else {
                    IntPredicate::SLT
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "lt")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::Gt => {
                let pred = if lhs_unsigned {
                    IntPredicate::UGT
                } else {
                    IntPredicate::SGT
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "gt")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::LtEq => {
                let pred = if lhs_unsigned {
                    IntPredicate::ULE
                } else {
                    IntPredicate::SLE
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "le")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::GtEq => {
                let pred = if lhs_unsigned {
                    IntPredicate::UGE
                } else {
                    IntPredicate::SGE
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "ge")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::BitwiseAnd => self
                .builder
                .build_and(lhs, rhs, "and")
                .map_err(llvm_err!("build_and")),
            BinOp::BitwiseOr => self
                .builder
                .build_or(lhs, rhs, "or")
                .map_err(llvm_err!("build_or")),
            BinOp::BitwiseXor => self
                .builder
                .build_xor(lhs, rhs, "xor")
                .map_err(llvm_err!("build_xor")),
            BinOp::LogicalAnd => {
                // Logical AND: result is 1 if both lhs and rhs are non-zero
                let zero = wt.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                self.builder
                    .build_and(lhs_nonzero, rhs_nonzero, "logical_and")
                    .map_err(llvm_err!("build_and"))
            }
            BinOp::LogicalOr => {
                // Logical OR: result is 1 if either lhs or rhs is non-zero
                let zero = wt.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                self.builder
                    .build_or(lhs_nonzero, rhs_nonzero, "logical_or")
                    .map_err(llvm_err!("build_or"))
            }
            BinOp::LogicalXor => {
                // Logical XOR: result is 1 if exactly one of lhs or rhs is non-zero
                let zero = wt.const_int(0, false);
                let lhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, zero, "lhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                let rhs_nonzero = self
                    .builder
                    .build_int_compare(IntPredicate::NE, rhs, zero, "rhs_nz")
                    .map_err(llvm_err!("build_int_compare"))?;
                self.builder
                    .build_xor(lhs_nonzero, rhs_nonzero, "logical_xor")
                    .map_err(llvm_err!("build_xor"))
            }
            BinOp::LShift => self
                .builder
                .build_left_shift(lhs, rhs, "lshift")
                .map_err(llvm_err!("build_left_shift")),
            BinOp::RShift => self
                .builder
                .build_right_shift(lhs, rhs, !lhs_unsigned, "rshift")
                .map_err(llvm_err!("build_right_shift")),
        }
    }

    fn codegen_unaryop(
        &self,
        op: &UnaryOp,
        val: inkwell::values::IntValue<'ctx>,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        match op {
            // Two's-complement negation: 0 - val
            UnaryOp::Neg => self
                .builder
                .build_int_neg(val, "neg")
                .map_err(llvm_err!("build_int_neg")),
            // Logical NOT: result is 1 if val is 0, else 0
            UnaryOp::Not => {
                let ty = val.get_type();
                let zero = ty.const_zero();
                let is_zero = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, val, zero, "is_zero")
                    .map_err(llvm_err!("build_int_compare"))?;
                self.builder
                    .build_int_z_extend(is_zero, ty, "not_ext")
                    .map_err(llvm_err!("build_int_z_extend"))
            }
            // Bitwise complement: ~val
            UnaryOp::BitwiseNot => self
                .builder
                .build_not(val, "bitwise_not")
                .map_err(llvm_err!("build_not")),
        }
    }
}

pub fn emit_object_and_ir(
    program: &Program,
    out_obj: &Path,
    out_ll: Option<&Path>,
) -> CodegenResult<()> {
    // 1) Init target and build the TargetMachine upfront so the data layout
    //    is available before IR generation (required for usize/isize types).
    Target::initialize_native(&InitializationConfig::default())
        .map_err(CodegenError::TargetInit)?;

    let triple = TargetMachine::get_default_triple();
    let target =
        Target::from_triple(&triple).map_err(|e| CodegenError::TargetError(e.to_string()))?;

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
        .ok_or(CodegenError::TargetMachineCreation)?;

    // 2) Build IR module, passing the data layout so that usize/isize resolve
    //    to the correct pointer-sized integer type for this target.
    let context = Context::create();
    let cg = CodeGen::new(&context, "xenon_mvp", tm.get_target_data());
    let module = cg.compile_program(program)?;
    module.set_triple(&triple);

    // Optional: write LLVM IR text for debugging
    if let Some(ll_path) = out_ll {
        module
            .print_to_file(ll_path)
            .map_err(|e| CodegenError::OutputFile(format!("print_to_file(.ll) failed: {e}")))?;
    }

    // 3) Emit object file
    tm.write_to_file(&module, FileType::Object, out_obj)
        .map_err(|e| CodegenError::OutputFile(format!("write_to_file(.o) failed: {e}")))?;

    Ok(())
}

pub fn default_output_paths(out_dir: &Path) -> (PathBuf, PathBuf) {
    let obj = out_dir.join("out.o");
    let ll = out_dir.join("out.ll");
    (obj, ll)
}
