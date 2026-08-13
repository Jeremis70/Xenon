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
use inkwell::values::{BasicValue, BasicValueEnum, FunctionValue, PointerValue};

use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::error::{CodegenError, CodegenResult};
use crate::frontend::ast::{BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, UnaryOp};
use crate::middle::validate::infer_expr_type_after_validate;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

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
    /// AST type of values written to `result_slot` / loaded at loop exit.
    result_ast_ty: Type,
}

pub struct CodeGen<'ctx, 'a> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    /// Block-scoped variable map. Each entry is a scope level; variable
    /// lookup walks the stack top-down so inner scopes shadow outer ones.
    variables: Vec<HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>, Type)>>,
    /// Stack of frames pushed when entering a loop, popped on exit.
    loop_stack: Vec<LoopFrame<'ctx>>,
    /// The function currently being compiled; set at the start of
    /// `compile_function` so that `Expr::Loop` in expression position can
    /// access the function value and return type without threading extra
    /// parameters through `codegen_expr`.
    current_fn: Option<(FunctionValue<'ctx>, BasicTypeEnum<'ctx>)>,
    /// Whole program (for type inference in codegen).
    ast_program: &'a Program,
    /// Function whose body is being codegen'd (for type inference).
    ast_fn: Option<&'a Function>,
    /// Data layout of the compilation target. Used to resolve `usize`/`isize`
    /// to their pointer-sized LLVM integer type via `ptr_sized_int_type`.
    target_data: TargetData,
    /// When true, emit runtime checks for overflow, division by zero, and
    /// invalid shift amounts. Typically enabled at `-O0` (debug builds).
    debug_checks: bool,
    /// Maps source function names to LLVM IR names when they differ
    /// (e.g. a non-entry function named "main" becomes "_xe.main").
    /// The `_xe.` prefix contains a dot, which is illegal in Xenon
    /// identifiers but valid in LLVM IR, so collisions are impossible.
    name_map: HashMap<String, String>,
}

impl<'ctx, 'a> CodeGen<'ctx, 'a> {
    pub fn new(
        context: &'ctx Context,
        module_name: &str,
        target_data: TargetData,
        debug_checks: bool,
        ast_program: &'a Program,
    ) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            variables: vec![HashMap::new()],
            loop_stack: Vec::new(),
            current_fn: None,
            ast_program,
            ast_fn: None,
            target_data,
            debug_checks,
            name_map: HashMap::new(),
        }
    }

    fn llvm_type(&self, ty: &crate::frontend::ast::Type) -> CodegenResult<BasicTypeEnum<'ctx>> {
        Ok(match ty {
            Type::Bool => self.context.bool_type().into(),
            Type::Int(w) | Type::UInt(w) => {
                let nz = std::num::NonZero::new(*w).ok_or(CodegenError::InvalidIrState(
                    "integer type with zero bit width",
                ))?;
                self.context
                    .custom_width_int_type(nz)
                    .map_err(|_| {
                        CodegenError::InvalidIrState("LLVM rejected custom_width_int_type")
                    })?
                    .into()
            }
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

    // ── Scope management ─────────────────────────────────────────────────

    fn push_scope(&mut self) {
        self.variables.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.variables.pop();
    }

    fn lookup_variable(
        &self,
        name: &str,
    ) -> Option<&(PointerValue<'ctx>, BasicTypeEnum<'ctx>, Type)> {
        for scope in self.variables.iter().rev() {
            if let Some(entry) = scope.get(name) {
                return Some(entry);
            }
        }
        None
    }

    fn insert_variable(
        &mut self,
        name: String,
        val: (PointerValue<'ctx>, BasicTypeEnum<'ctx>, Type),
    ) {
        if let Some(scope) = self.variables.last_mut() {
            scope.insert(name, val);
        }
    }

    // ── Runtime-check helpers ────────────────────────────────────────────

    fn parent_function(&self) -> CodegenResult<FunctionValue<'ctx>> {
        self.current_fn
            .map(|(f, _)| f)
            .ok_or(CodegenError::InvalidIrState("no current function"))
    }

    fn get_trap_function(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("llvm.trap") {
            return f;
        }
        let void_ty = self.context.void_type();
        let fn_ty = void_ty.fn_type(&[], false);
        self.module.add_function("llvm.trap", fn_ty, None)
    }

    fn emit_trap(&self) -> CodegenResult<()> {
        let trap_fn = self.get_trap_function();
        self.builder
            .build_call(trap_fn, &[], "")
            .map_err(llvm_err!("build_call (trap)"))?;
        self.builder
            .build_unreachable()
            .map_err(llvm_err!("build_unreachable"))?;
        Ok(())
    }

    fn emit_div_zero_check(&self, divisor: inkwell::values::IntValue<'ctx>) -> CodegenResult<()> {
        let fn_val = self.parent_function()?;
        let zero = divisor.get_type().const_zero();
        let is_zero = self
            .builder
            .build_int_compare(IntPredicate::EQ, divisor, zero, "div_zero_check")
            .map_err(llvm_err!("build_int_compare (div zero check)"))?;
        let trap_bb = self.context.append_basic_block(fn_val, "div_trap");
        let ok_bb = self.context.append_basic_block(fn_val, "div_ok");
        self.builder
            .build_conditional_branch(is_zero, trap_bb, ok_bb)
            .map_err(llvm_err!("build_conditional_branch (div zero)"))?;
        self.builder.position_at_end(trap_bb);
        self.emit_trap()?;
        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    fn emit_shift_check(
        &self,
        shift_amt: inkwell::values::IntValue<'ctx>,
        bit_width: u32,
    ) -> CodegenResult<()> {
        let fn_val = self.parent_function()?;
        let limit = shift_amt.get_type().const_int(bit_width as u64, false);
        let too_large = self
            .builder
            .build_int_compare(IntPredicate::UGE, shift_amt, limit, "shift_check")
            .map_err(llvm_err!("build_int_compare (shift check)"))?;
        let trap_bb = self.context.append_basic_block(fn_val, "shift_trap");
        let ok_bb = self.context.append_basic_block(fn_val, "shift_ok");
        self.builder
            .build_conditional_branch(too_large, trap_bb, ok_bb)
            .map_err(llvm_err!("build_conditional_branch (shift)"))?;
        self.builder.position_at_end(trap_bb);
        self.emit_trap()?;
        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    fn get_overflow_intrinsic(
        &self,
        name: &str,
        int_ty: inkwell::types::IntType<'ctx>,
    ) -> FunctionValue<'ctx> {
        let fn_name = format!("{name}.i{}", int_ty.get_bit_width());
        if let Some(f) = self.module.get_function(&fn_name) {
            return f;
        }
        let bool_ty = self.context.bool_type();
        let struct_ty = self
            .context
            .struct_type(&[int_ty.into(), bool_ty.into()], false);
        let fn_ty = struct_ty.fn_type(&[int_ty.into(), int_ty.into()], false);
        self.module.add_function(&fn_name, fn_ty, None)
    }

    fn emit_checked_arithmetic(
        &self,
        intrinsic_name: &str,
        lhs: inkwell::values::IntValue<'ctx>,
        rhs: inkwell::values::IntValue<'ctx>,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        let int_ty = lhs.get_type();
        let intrinsic = self.get_overflow_intrinsic(intrinsic_name, int_ty);
        let call = self
            .builder
            .build_call(intrinsic, &[lhs.into(), rhs.into()], "checked")
            .map_err(llvm_err!("build_call (overflow intrinsic)"))?;
        let result_struct = call
            .try_as_basic_value()
            .basic()
            .ok_or(CodegenError::InvalidIrState(
                "overflow intrinsic returned void",
            ))?
            .into_struct_value();
        let result = self
            .builder
            .build_extract_value(result_struct, 0, "result")
            .map_err(llvm_err!("build_extract_value (result)"))?
            .into_int_value();
        let overflow = self
            .builder
            .build_extract_value(result_struct, 1, "overflow")
            .map_err(llvm_err!("build_extract_value (overflow)"))?
            .into_int_value();

        let fn_val = self.parent_function()?;
        let trap_bb = self.context.append_basic_block(fn_val, "overflow_trap");
        let ok_bb = self.context.append_basic_block(fn_val, "overflow_ok");
        self.builder
            .build_conditional_branch(overflow, trap_bb, ok_bb)
            .map_err(llvm_err!("build_conditional_branch (overflow)"))?;
        self.builder.position_at_end(trap_bb);
        self.emit_trap()?;
        self.builder.position_at_end(ok_bb);
        Ok(result)
    }

    fn collect_param_types(&self, f: &Function) -> CodegenResult<Vec<BasicTypeEnum<'ctx>>> {
        f.params
            .iter()
            .map(|p| self.llvm_type(&p.ty))
            .collect::<CodegenResult<_>>()
    }

    pub fn compile_program(mut self) -> CodegenResult<Module<'ctx>> {
        // Find the entry function (if any) and determine LLVM names.
        let entry_fn_name = self
            .ast_program
            .functions
            .iter()
            .find(|f| f.attributes.iter().any(|a| a.name == "entry"))
            .map(|f| f.name.clone());

        // If there's an entry function not named "main" and another function IS
        // named "main", the latter must be renamed to avoid collision with the
        // generated @main wrapper.
        let needs_main_rename = entry_fn_name.as_ref().is_some_and(|name| name != "main")
            && self
                .ast_program
                .functions
                .iter()
                .any(|f| f.name == "main" && !f.attributes.iter().any(|a| a.name == "entry"));

        if needs_main_rename {
            self.name_map
                .insert("main".to_string(), "_xe.main".to_string());
        }

        for f in &self.ast_program.functions {
            self.declare_function_with_name(
                f,
                self.llvm_name_for(f, &entry_fn_name, needs_main_rename),
            )?;
        }
        for f in &self.ast_program.functions {
            self.ast_fn = Some(f);
            self.compile_function_with_name(
                f,
                self.llvm_name_for(f, &entry_fn_name, needs_main_rename),
            )?;
            self.ast_fn = None;
        }

        // Emit @main wrapper if the entry function is not already named "main".
        if let Some(ref entry_name) = entry_fn_name
            && entry_name != "main"
        {
            self.emit_main_wrapper(entry_name)?;
        }

        Ok(self.module)
    }

    /// Returns the LLVM function name for a given AST function.
    fn llvm_name_for(
        &self,
        f: &Function,
        _entry_fn_name: &Option<String>,
        needs_main_rename: bool,
    ) -> String {
        if needs_main_rename && f.name == "main" && !f.attributes.iter().any(|a| a.name == "entry")
        {
            "_xe.main".to_string()
        } else {
            f.name.clone()
        }
    }

    /// Registers a function signature under the given LLVM name.
    fn declare_function_with_name(
        &self,
        f: &Function,
        llvm_name: String,
    ) -> CodegenResult<FunctionValue<'ctx>> {
        if let Some(existing) = self.module.get_function(&llvm_name) {
            return Ok(existing);
        }
        let ret_ty = self.llvm_type(&f.return_type.ty)?;
        let param_types: Vec<BasicTypeEnum> = self.collect_param_types(f)?;
        let param_metadata: Vec<inkwell::types::BasicMetadataTypeEnum> =
            param_types.iter().map(|&t| t.into()).collect();
        let fn_ty = ret_ty.fn_type(&param_metadata, false);
        Ok(self.module.add_function(&llvm_name, fn_ty, None))
    }

    /// Compiles a function body under the given LLVM name.
    fn compile_function_with_name(
        &mut self,
        f: &Function,
        llvm_name: String,
    ) -> CodegenResult<FunctionValue<'ctx>> {
        let fn_val = self
            .module
            .get_function(&llvm_name)
            .ok_or(CodegenError::InvalidIrState(
                "declare_function must be called before compile_function",
            ))?;

        let param_types: Vec<BasicTypeEnum> = self.collect_param_types(f)?;
        let ret_ty = self.llvm_type(&f.return_type.ty)?;
        self.current_fn = Some((fn_val, ret_ty));
        let entry = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry);

        self.variables = vec![HashMap::new()];

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
                .ok_or(CodegenError::InvalidIrState("missing param value"))?;
            self.builder
                .build_store(alloca, incoming)
                .map_err(llvm_err!("build_store (param)"))?;
            self.insert_variable(
                param.name.clone().unwrap_or_default(),
                (alloca, llvm_ty, param.ty.clone()),
            );
        }

        // If the return type has a named binding (e.g. `-> u32 sum`), allocate a
        // stack slot for it and zero-initialise it so that it can be used as a
        // regular variable inside the body and returned implicitly.
        if let Some(ret_name) = &f.return_type.name {
            let alloca = self
                .builder
                .build_alloca(ret_ty, ret_name)
                .map_err(llvm_err!("build_alloca (named return)"))?;
            let zero: BasicValueEnum = match ret_ty {
                BasicTypeEnum::IntType(it) => it.const_zero().into(),
                BasicTypeEnum::FloatType(ft) => ft.const_float(0.0).into(),
                _ => {
                    return Err(CodegenError::InvalidIrState(
                        "named return: unsupported LLVM type for zero init",
                    ));
                }
            };
            self.builder
                .build_store(alloca, zero)
                .map_err(llvm_err!("build_store (named return init)"))?;
            self.insert_variable(ret_name.clone(), (alloca, ret_ty, f.return_type.ty.clone()));
        }

        for stmt in &f.body {
            // compile_stmt returns true when the current block has been terminated
            // (e.g. a return was emitted); stop processing the remaining statements.
            if self.compile_stmt(stmt, fn_val, ret_ty, &f.return_type.ty)? {
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
                    span: f.span,
                });
            };
            let (ptr, ty, _) =
                self.lookup_variable(ret_name.as_str())
                    .ok_or(CodegenError::InvalidIrState(
                        "named return variable missing from variable map",
                    ))?;
            let val = self
                .builder
                .build_load(*ty, *ptr, ret_name)
                .map_err(llvm_err!("build_load (implicit return)"))?
                .as_basic_value_enum();
            let val = self.coerce_basic_to_target(val, ret_ty, &f.return_type.ty)?;
            self.builder
                .build_return(Some(&val))
                .map_err(llvm_err!("build_return (implicit)"))?;
        }

        Ok(fn_val)
    }

    /// Emits a thin C-ABI `@main` wrapper that calls the user's entry function.
    fn emit_main_wrapper(&self, entry_name: &str) -> CodegenResult<()> {
        let i32_ty = self.context.i32_type();
        let fn_ty = i32_ty.fn_type(&[], false);
        let main_fn = self.module.add_function("main", fn_ty, None);
        let bb = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(bb);

        let callee = self
            .module
            .get_function(entry_name)
            .ok_or(CodegenError::InvalidIrState(
                "entry function not found in module",
            ))?;
        let call = self
            .builder
            .build_call(callee, &[], "ret")
            .map_err(llvm_err!("build_call (main wrapper)"))?;
        let ret_val = call
            .try_as_basic_value()
            .basic()
            .ok_or(CodegenError::InvalidIrState("entry function returned void"))?;
        self.builder
            .build_return(Some(&ret_val))
            .map_err(llvm_err!("build_return (main wrapper)"))?;
        Ok(())
    }

    /// Compiles a single statement. Returns `true` when the current basic block
    /// has been terminated (a `return` was emitted), signalling the caller to
    /// stop processing further statements in the same scope.
    fn compile_stmt(
        &mut self,
        stmt: &Stmt,
        fn_val: FunctionValue<'ctx>,
        ret_ty: BasicTypeEnum<'ctx>,
        ret_ast_ty: &Type,
    ) -> CodegenResult<bool> {
        match &stmt.kind {
            StmtKind::Return(inner) => {
                let value = self.codegen_expr(inner)?;
                let value = self.coerce_basic_to_target(value, ret_ty, ret_ast_ty)?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(llvm_err!("build_return"))?;
                Ok(true)
            }
            StmtKind::VarDecl(binding) => {
                let llvm_ty = self.llvm_type(&binding.ty)?;
                let var_name = binding.name.as_deref().unwrap_or("_");
                let alloca = self
                    .builder
                    .build_alloca(llvm_ty, var_name)
                    .map_err(llvm_err!("build_alloca"))?;
                self.insert_variable(
                    binding.name.clone().unwrap_or_default(),
                    (alloca, llvm_ty, binding.ty.clone()),
                );
                let init_val = self.codegen_expr(binding.default.as_ref().ok_or(
                    CodegenError::InvalidIrState("VarDecl binding must have a default value"),
                )?)?;
                let init_val = self.coerce_basic_to_target(init_val, llvm_ty, &binding.ty)?;
                self.builder
                    .build_store(alloca, init_val)
                    .map_err(llvm_err!("build_store"))?;
                Ok(false)
            }
            StmtKind::Assign { name, value } => {
                let (var_ptr, var_ty, ast_ty) = {
                    let (ptr, ty, ast_ty) = self.lookup_variable(name).ok_or_else(|| {
                        CodegenError::UndefinedVariable {
                            name: name.clone(),
                            span: stmt.span,
                        }
                    })?;
                    (*ptr, *ty, ast_ty.clone())
                };
                let val = self.codegen_expr(value)?;
                let val = self.coerce_basic_to_target(val, var_ty, &ast_ty)?;
                self.builder
                    .build_store(var_ptr, val)
                    .map_err(llvm_err!("build_store"))?;
                Ok(false)
            }
            StmtKind::Expr(expr) => {
                self.codegen_expr(expr)?;
                // An expression like an infinite loop may have terminated the
                // current block; stop emitting further statements if so.
                let terminated = self
                    .builder
                    .get_insert_block()
                    .is_some_and(|b| b.get_terminator().is_some());
                Ok(terminated)
            }
            StmtKind::Break(opt_expr) => {
                let (break_bb, result_slot, slot_ast_ty) = {
                    let frame = self.loop_stack.last().ok_or(CodegenError::InvalidIrState(
                        "`break` used outside of a loop",
                    ))?;
                    (
                        frame.break_bb,
                        frame.result_slot,
                        frame.result_ast_ty.clone(),
                    )
                };
                if let Some(expr) = opt_expr {
                    let val = self.codegen_expr(expr)?;
                    let slot_ty = self.llvm_type(&slot_ast_ty)?;
                    let val = self.coerce_basic_to_target(val, slot_ty, &slot_ast_ty)?;
                    self.builder
                        .build_store(result_slot, val)
                        .map_err(llvm_err!("build_store (break value)"))?;
                }
                self.builder
                    .build_unconditional_branch(break_bb)
                    .map_err(llvm_err!("build_unconditional_branch (break)"))?;
                Ok(true)
            }
            StmtKind::Continue => {
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
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.codegen_expr(condition)?;
                let is_true = self.expr_to_bool_condition(cond_val)?;

                let then_block = self.context.append_basic_block(fn_val, "then");
                let else_block = self.context.append_basic_block(fn_val, "else");

                self.builder
                    .build_conditional_branch(is_true, then_block, else_block)
                    .map_err(llvm_err!("build_conditional_branch"))?;

                // Then branch
                self.builder.position_at_end(then_block);
                self.push_scope();
                for s in then_branch {
                    if self.compile_stmt(s, fn_val, ret_ty, ret_ast_ty)? {
                        break;
                    }
                }
                self.pop_scope();
                let then_end =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no insert block after then branch",
                        ))?;
                let then_falls_through = then_end.get_terminator().is_none();

                // Else branch
                self.builder.position_at_end(else_block);
                self.push_scope();
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        if self.compile_stmt(s, fn_val, ret_ty, ret_ast_ty)? {
                            break;
                        }
                    }
                }
                self.pop_scope();
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

    fn coerce_basic_to_target(
        &self,
        val: BasicValueEnum<'ctx>,
        target: BasicTypeEnum<'ctx>,
        ast_ty: &Type,
    ) -> CodegenResult<BasicValueEnum<'ctx>> {
        if val.get_type() == target {
            return Ok(val);
        }
        match (val, target) {
            (BasicValueEnum::IntValue(v), BasicTypeEnum::IntType(_)) => {
                let is_unsigned = matches!(ast_ty, Type::UInt(_) | Type::USize);
                Ok(self.cast_int_to_type(v, target, is_unsigned)?.into())
            }
            (BasicValueEnum::FloatValue(v), BasicTypeEnum::FloatType(ft)) => {
                if v.get_type() == ft {
                    Ok(v.into())
                } else {
                    Ok(self
                        .builder
                        .build_float_cast(v, ft, "float_cast")
                        .map_err(llvm_err!("build_float_cast"))?
                        .into())
                }
            }
            (BasicValueEnum::IntValue(v), BasicTypeEnum::FloatType(ft)) => {
                let is_unsigned = matches!(ast_ty, Type::UInt(_) | Type::USize);
                if is_unsigned {
                    Ok(self
                        .builder
                        .build_unsigned_int_to_float(v, ft, "uitofp")
                        .map_err(llvm_err!("build_unsigned_int_to_float"))?
                        .into())
                } else {
                    Ok(self
                        .builder
                        .build_signed_int_to_float(v, ft, "sitofp")
                        .map_err(llvm_err!("build_signed_int_to_float"))?
                        .into())
                }
            }
            (BasicValueEnum::FloatValue(v), BasicTypeEnum::IntType(it)) => {
                let is_unsigned = matches!(ast_ty, Type::UInt(_) | Type::USize);
                if is_unsigned {
                    Ok(self
                        .builder
                        .build_float_to_unsigned_int(v, it, "fptoui")
                        .map_err(llvm_err!("build_float_to_unsigned_int"))?
                        .into())
                } else {
                    Ok(self
                        .builder
                        .build_float_to_signed_int(v, it, "fptosi")
                        .map_err(llvm_err!("build_float_to_signed_int"))?
                        .into())
                }
            }
            _ => Err(CodegenError::InvalidIrState(
                "coerce_basic_to_target: incompatible value and target",
            )),
        }
    }

    /// AST types for every stack slot visible from LLVM scopes (inner scopes shadow outer).
    fn visible_ast_var_types(&self) -> HashMap<String, Type> {
        let mut m = HashMap::new();
        for scope in self.variables.iter().rev() {
            for (name, (_, _, ty)) in scope {
                m.entry(name.clone()).or_insert_with(|| ty.clone());
            }
        }
        m
    }

    fn expr_to_bool_condition(
        &self,
        val: BasicValueEnum<'ctx>,
    ) -> CodegenResult<inkwell::values::IntValue<'ctx>> {
        match val {
            BasicValueEnum::IntValue(v) => {
                if v.get_type().get_bit_width() == 1 {
                    Ok(v)
                } else {
                    let zero = v.get_type().const_zero();
                    self.builder
                        .build_int_compare(IntPredicate::NE, v, zero, "cond_i")
                        .map_err(llvm_err!("build_int_compare (cond)"))
                }
            }
            BasicValueEnum::FloatValue(v) => {
                let zero = v.get_type().const_float(0.0);
                self.builder
                    .build_float_compare(FloatPredicate::ONE, v, zero, "cond_f")
                    .map_err(llvm_err!("build_float_compare (cond)"))
            }
            _ => Err(CodegenError::InvalidIrState(
                "condition must be bool or numeric",
            )),
        }
    }

    fn codegen_expr(&mut self, e: &Expr) -> CodegenResult<BasicValueEnum<'ctx>> {
        match &e.kind {
            ExprKind::Int(v) => Ok(bigint_to_llvm_const(self.context, v).into()),
            ExprKind::Bool(b) => Ok(self
                .context
                .bool_type()
                .const_int(u64::from(*b), false)
                .into()),
            ExprKind::Float(fv) => {
                let locals = self.visible_ast_var_types();
                let ast_ty = infer_expr_type_after_validate(
                    e,
                    self.ast_program,
                    self.ast_fn.ok_or(CodegenError::InvalidIrState(
                        "codegen: missing current function for float literal",
                    ))?,
                    &locals,
                )
                .map_err(|e| CodegenError::Other(e.to_string()))?;
                let ll = self.llvm_type(&ast_ty)?;
                let BasicTypeEnum::FloatType(ft) = ll else {
                    return Err(CodegenError::UnsupportedType {
                        ty: ast_ty.to_string(),
                        span: e.span,
                    });
                };
                Ok(ft.const_float(*fv).into())
            }
            ExprKind::Ident(name) => {
                let &(ref ptr, ty, ref _ast_ty) =
                    self.lookup_variable(name)
                        .ok_or_else(|| CodegenError::UndefinedVariable {
                            name: name.clone(),
                            span: e.span,
                        })?;
                self.builder
                    .build_load(ty, *ptr, name.as_str())
                    .map_err(llvm_err!("build_load"))
                    .map(|v| v.as_basic_value_enum())
            }
            ExprKind::BinOp { lhs, op, rhs } => {
                let lhs_unsigned = self.infer_expr_unsigned(lhs);
                let rhs_unsigned = self.infer_expr_unsigned(rhs);
                let lhs_val = self.codegen_expr(lhs)?;
                let rhs_val = self.codegen_expr(rhs)?;
                match (lhs_val, rhs_val) {
                    (BasicValueEnum::IntValue(l), BasicValueEnum::IntValue(r)) => self
                        .codegen_binop(op, l, r, lhs_unsigned, rhs_unsigned)
                        .map(Into::into),
                    (BasicValueEnum::FloatValue(l), BasicValueEnum::FloatValue(r)) => {
                        self.codegen_binop_float(op, l, r)
                    }
                    _ => Err(CodegenError::InvalidIrState(
                        "mixed operand categories in binop (should be caught by validate)",
                    )),
                }
            }
            ExprKind::UnaryOp { op, operand } => {
                let val = self.codegen_expr(operand)?;
                match val {
                    BasicValueEnum::IntValue(v) => self.codegen_unaryop(op, v).map(Into::into),
                    BasicValueEnum::FloatValue(v) => {
                        self.codegen_unaryop_float(op, v).map(Into::into)
                    }
                    _ => Err(CodegenError::InvalidIrState(
                        "unary op on unsupported value",
                    )),
                }
            }
            ExprKind::Call { name, args } => {
                let llvm_name = self
                    .name_map
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                let callee = self.module.get_function(&llvm_name).ok_or_else(|| {
                    CodegenError::UndefinedFunction {
                        name: name.clone(),
                        span: e.span,
                    }
                })?;
                let expected = callee.count_params() as usize;
                if args.len() != expected {
                    return Err(CodegenError::ArgumentCountMismatch {
                        name: name.clone(),
                        expected,
                        got: args.len(),
                        span: e.span,
                    });
                }
                let callee_fn = self
                    .ast_program
                    .functions
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or(CodegenError::InvalidIrState("call: missing AST for callee"))?;
                let mut compiled_args: Vec<inkwell::values::BasicMetadataValueEnum> =
                    Vec::with_capacity(args.len());
                for ((arg_expr, param), param_binding) in args
                    .iter()
                    .zip(callee.get_param_iter())
                    .zip(callee_fn.params.iter())
                {
                    let val = self.codegen_expr(arg_expr)?;
                    let target = param.get_type();
                    compiled_args.push(
                        self.coerce_basic_to_target(val, target, &param_binding.ty)?
                            .into(),
                    );
                }
                let call = self
                    .builder
                    .build_call(callee, &compiled_args, "call")
                    .map_err(llvm_err!("build_call"))?;
                let ret = call
                    .try_as_basic_value()
                    .basic()
                    .ok_or(CodegenError::InvalidIrState("call returned void"))?;
                Ok(ret.as_basic_value_enum())
            }
            ExprKind::IfElse {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_v = self.codegen_expr(condition)?;
                let cond_i1 = self.expr_to_bool_condition(cond_v)?;
                let locals = self.visible_ast_var_types();
                let merged_ast = infer_expr_type_after_validate(
                    e,
                    self.ast_program,
                    self.ast_fn.ok_or(CodegenError::InvalidIrState(
                        "if-expression outside of function",
                    ))?,
                    &locals,
                )
                .map_err(|e| CodegenError::Other(e.to_string()))?;
                let merged_ll = self.llvm_type(&merged_ast)?;

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

                self.builder.position_at_end(then_bb);
                let then_val_raw = self.codegen_expr(then_branch)?;
                let then_end_bb =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no block after then expression",
                        ))?;

                self.builder.position_at_end(else_bb);
                let else_val_raw = self.codegen_expr(else_branch)?;
                let else_end_bb =
                    self.builder
                        .get_insert_block()
                        .ok_or(CodegenError::InvalidIrState(
                            "no block after else expression",
                        ))?;

                self.builder.position_at_end(then_end_bb);
                let then_val = self.coerce_basic_to_target(then_val_raw, merged_ll, &merged_ast)?;
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(llvm_err!("build_unconditional_branch (ife then->merge)"))?;

                self.builder.position_at_end(else_end_bb);
                let else_val = self.coerce_basic_to_target(else_val_raw, merged_ll, &merged_ast)?;
                self.builder
                    .build_unconditional_branch(merge_bb)
                    .map_err(llvm_err!("build_unconditional_branch (ife else->merge)"))?;

                self.builder.position_at_end(merge_bb);
                let phi = self
                    .builder
                    .build_phi(merged_ll, "ife_result")
                    .map_err(llvm_err!("build_phi"))?;
                phi.add_incoming(&[(&then_val, then_end_bb), (&else_val, else_end_bb)]);
                Ok(phi.as_basic_value().as_basic_value_enum())
            }
            ExprKind::Loop { body } => self.compile_loop(body, None, false, false, e),
            ExprKind::CondLoop {
                post,
                inverted,
                condition,
                body,
            } => self.compile_loop(body, Some(condition), *post, *inverted, e),
        }
    }

    fn compile_loop(
        &mut self,
        body: &[Stmt],
        condition: Option<&Expr>,
        post: bool,
        inverted: bool,
        loop_expr: &Expr,
    ) -> CodegenResult<BasicValueEnum<'ctx>> {
        let (fn_val, ret_llvm) = self.current_fn.ok_or(CodegenError::InvalidIrState(
            "loop expression compiled outside of a function",
        ))?;
        let fn_ret_ast = &self
            .ast_fn
            .ok_or(CodegenError::InvalidIrState("loop: missing ast_fn"))?
            .return_type
            .ty;

        let locals = self.visible_ast_var_types();
        let result_ast = infer_expr_type_after_validate(
            loop_expr,
            self.ast_program,
            self.ast_fn.unwrap(),
            &locals,
        )
        .map_err(|e| CodegenError::Other(e.to_string()))?;
        let slot_ll = self.llvm_type(&result_ast)?;
        let result_slot = self
            .builder
            .build_alloca(slot_ll, "loop_result")
            .map_err(llvm_err!("build_alloca (loop result)"))?;

        let loop_result_default: BasicValueEnum = match slot_ll {
            BasicTypeEnum::IntType(it) => it.const_zero().into(),
            BasicTypeEnum::FloatType(ft) => ft.const_float(0.0).into(),
            _ => {
                return Err(CodegenError::InvalidIrState(
                    "loop result: unsupported LLVM slot type",
                ));
            }
        };
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
            result_ast_ty: result_ast.clone(),
        });
        self.push_scope();
        for s in body {
            if self.compile_stmt(s, fn_val, ret_llvm, fn_ret_ast)? {
                break;
            }
        }
        self.pop_scope();
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
            return Ok(self.context.i64_type().const_zero().into());
        }

        // Load the result from the result slot and return it.
        self.builder.position_at_end(loop_after);
        self.builder
            .build_load(slot_ll, result_slot, "loop_val")
            .map_err(llvm_err!("build_load (loop result)"))
            .map(|v| v.as_basic_value_enum())
    }

    fn emit_loop_condition(
        &mut self,
        condition: &Expr,
        inverted: bool,
        body_bb: BasicBlock<'ctx>,
        after_bb: BasicBlock<'ctx>,
    ) -> CodegenResult<()> {
        let cond_val = self.codegen_expr(condition)?;
        let cond_i1 = self.expr_to_bool_condition(cond_val)?;
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
        match &e.kind {
            ExprKind::Ident(name) => self
                .lookup_variable(name.as_str())
                .is_some_and(|(_, _, ty)| matches!(ty, Type::UInt(_) | Type::USize)),
            ExprKind::Int(_) => false,
            ExprKind::BinOp { lhs, rhs, .. } => {
                // Propagate unsignedness from either operand.
                self.infer_expr_unsigned(lhs) || self.infer_expr_unsigned(rhs)
            }
            ExprKind::UnaryOp { operand, .. } => self.infer_expr_unsigned(operand),
            ExprKind::IfElse {
                then_branch,
                else_branch,
                ..
            } => {
                // Propagate unsignedness from either branch.
                self.infer_expr_unsigned(then_branch) || self.infer_expr_unsigned(else_branch)
            }
            ExprKind::Call { .. } | ExprKind::Loop { .. } | ExprKind::CondLoop { .. } => false,
            ExprKind::Bool(_) | ExprKind::Float(_) => false,
        }
    }

    fn codegen_binop_float(
        &self,
        op: &BinOp,
        mut lhs: inkwell::values::FloatValue<'ctx>,
        mut rhs: inkwell::values::FloatValue<'ctx>,
    ) -> CodegenResult<BasicValueEnum<'ctx>> {
        let lw = lhs.get_type().get_bit_width();
        let rw = rhs.get_type().get_bit_width();
        if lw < rw {
            lhs = self
                .builder
                .build_float_cast(lhs, rhs.get_type(), "widen_l")
                .map_err(llvm_err!("build_float_cast"))?;
        } else if rw < lw {
            rhs = self
                .builder
                .build_float_cast(rhs, lhs.get_type(), "widen_r")
                .map_err(llvm_err!("build_float_cast"))?;
        }
        match op {
            BinOp::Add => Ok(self
                .builder
                .build_float_add(lhs, rhs, "fadd")
                .map_err(llvm_err!("build_float_add"))?
                .into()),
            BinOp::Sub => Ok(self
                .builder
                .build_float_sub(lhs, rhs, "fsub")
                .map_err(llvm_err!("build_float_sub"))?
                .into()),
            BinOp::Mul => Ok(self
                .builder
                .build_float_mul(lhs, rhs, "fmul")
                .map_err(llvm_err!("build_float_mul"))?
                .into()),
            BinOp::Div => Ok(self
                .builder
                .build_float_div(lhs, rhs, "fdiv")
                .map_err(llvm_err!("build_float_div"))?
                .into()),
            BinOp::Mod => Ok(self
                .builder
                .build_float_rem(lhs, rhs, "frem")
                .map_err(llvm_err!("build_float_rem"))?
                .into()),
            BinOp::Eq => Ok(self
                .builder
                .build_float_compare(FloatPredicate::OEQ, lhs, rhs, "feq")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            BinOp::NotEq => Ok(self
                .builder
                .build_float_compare(FloatPredicate::ONE, lhs, rhs, "fne")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            BinOp::Lt => Ok(self
                .builder
                .build_float_compare(FloatPredicate::OLT, lhs, rhs, "flt")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            BinOp::Gt => Ok(self
                .builder
                .build_float_compare(FloatPredicate::OGT, lhs, rhs, "fgt")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            BinOp::LtEq => Ok(self
                .builder
                .build_float_compare(FloatPredicate::OLE, lhs, rhs, "fle")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            BinOp::GtEq => Ok(self
                .builder
                .build_float_compare(FloatPredicate::OGE, lhs, rhs, "fge")
                .map_err(llvm_err!("build_float_compare"))?
                .into()),
            _ => Err(CodegenError::InvalidIrState(
                "unsupported operator for float operands",
            )),
        }
    }

    fn codegen_unaryop_float(
        &self,
        op: &UnaryOp,
        val: inkwell::values::FloatValue<'ctx>,
    ) -> CodegenResult<inkwell::values::FloatValue<'ctx>> {
        match op {
            UnaryOp::Neg => self
                .builder
                .build_float_neg(val, "fneg")
                .map_err(llvm_err!("build_float_neg")),
            _ => Err(CodegenError::InvalidIrState(
                "unsupported unary operator for float",
            )),
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

        // Combined unsigned flag: if either operand is unsigned, the operation
        // is treated as unsigned. This handles cases like `200 < x` where the
        // literal on the LHS is sign-neutral but `x` is `u32`.
        let is_unsigned = lhs_unsigned || rhs_unsigned;

        match op {
            BinOp::Add if self.debug_checks && is_unsigned => {
                self.emit_checked_arithmetic("llvm.uadd.with.overflow", lhs, rhs)
            }
            BinOp::Add if self.debug_checks => {
                self.emit_checked_arithmetic("llvm.sadd.with.overflow", lhs, rhs)
            }
            BinOp::Add => self
                .builder
                .build_int_add(lhs, rhs, "add")
                .map_err(llvm_err!("build_int_add")),
            BinOp::Sub if self.debug_checks && is_unsigned => {
                self.emit_checked_arithmetic("llvm.usub.with.overflow", lhs, rhs)
            }
            BinOp::Sub if self.debug_checks => {
                self.emit_checked_arithmetic("llvm.ssub.with.overflow", lhs, rhs)
            }
            BinOp::Sub => self
                .builder
                .build_int_sub(lhs, rhs, "sub")
                .map_err(llvm_err!("build_int_sub")),
            BinOp::Mul if self.debug_checks && is_unsigned => {
                self.emit_checked_arithmetic("llvm.umul.with.overflow", lhs, rhs)
            }
            BinOp::Mul if self.debug_checks => {
                self.emit_checked_arithmetic("llvm.smul.with.overflow", lhs, rhs)
            }
            BinOp::Mul => self
                .builder
                .build_int_mul(lhs, rhs, "mul")
                .map_err(llvm_err!("build_int_mul")),
            BinOp::Div if is_unsigned => {
                self.emit_div_zero_check(rhs)?;
                self.builder
                    .build_int_unsigned_div(lhs, rhs, "udiv")
                    .map_err(llvm_err!("build_int_unsigned_div"))
            }
            BinOp::Div => {
                self.emit_div_zero_check(rhs)?;
                self.builder
                    .build_int_signed_div(lhs, rhs, "sdiv")
                    .map_err(llvm_err!("build_int_signed_div"))
            }
            BinOp::Mod if is_unsigned => {
                self.emit_div_zero_check(rhs)?;
                self.builder
                    .build_int_unsigned_rem(lhs, rhs, "urem")
                    .map_err(llvm_err!("build_int_unsigned_rem"))
            }
            BinOp::Mod => {
                self.emit_div_zero_check(rhs)?;
                self.builder
                    .build_int_signed_rem(lhs, rhs, "srem")
                    .map_err(llvm_err!("build_int_signed_rem"))
            }
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
                let exp_pred = if rhs_unsigned {
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
                let pred = if is_unsigned {
                    IntPredicate::ULT
                } else {
                    IntPredicate::SLT
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "lt")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::Gt => {
                let pred = if is_unsigned {
                    IntPredicate::UGT
                } else {
                    IntPredicate::SGT
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "gt")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::LtEq => {
                let pred = if is_unsigned {
                    IntPredicate::ULE
                } else {
                    IntPredicate::SLE
                };
                self.builder
                    .build_int_compare(pred, lhs, rhs, "le")
                    .map_err(llvm_err!("build_int_compare"))
            }
            BinOp::GtEq => {
                let pred = if is_unsigned {
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
            BinOp::LShift => {
                self.emit_shift_check(rhs, wt.get_bit_width())?;
                self.builder
                    .build_left_shift(lhs, rhs, "lshift")
                    .map_err(llvm_err!("build_left_shift"))
            }
            BinOp::RShift => {
                self.emit_shift_check(rhs, wt.get_bit_width())?;
                self.builder
                    .build_right_shift(lhs, rhs, !is_unsigned, "rshift")
                    .map_err(llvm_err!("build_right_shift"))
            }
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

/// Converts a [`BigInt`] to an LLVM integer constant of appropriate width.
///
/// For values that fit in an `i64`, uses the fast [`const_int`] path and
/// returns an `i64` constant. For larger values, computes the minimum bit
/// width required to represent the value in two's complement, builds a
/// `u64` word array, and returns a constant of that custom `iN` type via
/// [`const_int_arbitrary_precision`].
fn bigint_to_llvm_const<'ctx>(
    context: &'ctx Context,
    value: &BigInt,
) -> inkwell::values::IntValue<'ctx> {
    if let Some(v) = value.to_i64() {
        context.i64_type().const_int(v as u64, true)
    } else {
        // Convert to sign-magnitude u64 words for const_int_arbitrary_precision.
        // LLVM expects two's complement in a u64 word array.
        let (sign, bytes) = value.to_bytes_le();
        // Compute the minimum number of bits needed (extra sign bit for negative).
        let bit_len = bytes.len() * 8
            + if sign == num_bigint::Sign::Minus {
                1
            } else {
                0
            };
        let width = std::cmp::max(bit_len as u32, 64);
        let int_ty = context
            .custom_width_int_type(std::num::NonZero::new(width).expect("width is non-zero"))
            .expect("custom_width_int_type failed");
        // Pack bytes into u64 words (little-endian).
        let num_words = (width as usize).div_ceil(64);
        let mut words = vec![0u64; num_words];
        for (i, &byte) in bytes.iter().enumerate() {
            let word_idx = i / 8;
            let bit_offset = (i % 8) * 8;
            words[word_idx] |= (byte as u64) << bit_offset;
        }
        // If negative, convert from magnitude to two's complement.
        if sign == num_bigint::Sign::Minus {
            // Invert all bits and add 1.
            for w in &mut words {
                *w = !*w;
            }
            let mut carry = 1u64;
            for w in &mut words {
                let (sum, overflow) = w.overflowing_add(carry);
                *w = sum;
                carry = if overflow { 1 } else { 0 };
            }
        }
        int_ty.const_int_arbitrary_precision(&words)
    }
}

pub fn emit_object_and_ir(
    program: &Program,
    out_obj: &Path,
    out_ll: Option<&Path>,
    debug_checks: bool,
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
    let cg = CodeGen::new(
        &context,
        "xenon_mvp",
        tm.get_target_data(),
        debug_checks,
        program,
    );
    let module = cg.compile_program()?;
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
