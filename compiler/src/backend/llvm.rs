//! LLVM implementation of the backend contract. This module only consumes MIR.

use std::collections::HashMap;
use std::num::NonZero;

use inkwell::basic_block::BasicBlock as LlvmBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::module::{FlagBehavior, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FloatType, IntType};
use inkwell::values::{
    AggregateValueEnum, BasicMetadataValueEnum, BasicValueEnum, FloatValue, FunctionValue,
    IntValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};
use num_bigint::{BigInt, Sign};

use crate::backend::contract::{
    Artifact, ArtifactKind, Backend, BackendError, CodegenOptions, OutputRequest, RelocationModel,
};
use crate::backend::prepare::{Symbols, prepare_program};
use crate::index::Idx;
use crate::middle::ids::DefId;
use crate::middle::mir::typing;
use crate::middle::mir::verify::verify_program;
use crate::middle::mir::{
    BasicBlock, BinOp, Body, CastKind, ConstValue, Constant, Local, MirPhase, MirProgram, Operand,
    Place, ProjectionElem, RETURN_PLACE, Rvalue, StatementKind, SwitchTargets, TerminatorKind,
    UnOp,
};
use crate::middle::target::TargetSpec;
use crate::types::Type;

/// Native LLVM backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct LlvmBackend;

impl Backend for LlvmBackend {
    fn name(&self) -> &'static str {
        "llvm"
    }

    fn emit(
        &self,
        program: &MirProgram,
        target: &TargetSpec,
        options: CodegenOptions,
        outputs: &[OutputRequest],
    ) -> Result<Vec<Artifact>, BackendError> {
        if let Some(body) = program
            .bodies()
            .find(|body| body.phase() != MirPhase::Runtime)
        {
            return Err(BackendError::InvalidMir(format!(
                "`{}` is in phase `{}`; LLVM requires Runtime MIR",
                program.fn_name(body.def_id()),
                body.phase()
            )));
        }
        let prepared = prepare_program(program)?;
        verify_program(&prepared, target)
            .map_err(|errors| BackendError::InvalidMir(errors.to_string()))?;

        let initialization = |error: String| BackendError::Initialization(error);
        Target::initialize_native(&InitializationConfig::default()).map_err(initialization)?;
        let triple = TargetMachine::get_default_triple();
        let llvm_target =
            Target::from_triple(&triple).map_err(|error| initialization(error.to_string()))?;
        let machine = llvm_target
            .create_target_machine(
                &triple,
                &TargetMachine::get_host_cpu_name().to_string(),
                &TargetMachine::get_host_cpu_features().to_string(),
                optimization_level(options.optimization),
                reloc_mode(options.relocation_model),
                CodeModel::Default,
            )
            .ok_or_else(|| initialization("could not create host target machine".to_owned()))?;
        let pointer_width = machine.get_target_data().get_pointer_byte_size(None) * 8;
        if pointer_width != target.pointer_width() {
            return Err(BackendError::TargetMismatch(format!(
                "MIR target has {}-bit pointers, LLVM host target has {pointer_width}-bit pointers",
                target.pointer_width()
            )));
        }

        let context = Context::create();
        let module = context.create_module("xenon");
        module.set_triple(&triple);
        module.set_data_layout(&machine.get_target_data().get_data_layout());
        set_pic_flags(&context, &module, options.relocation_model);
        let mut generator = MirCodegen::new(&context, module, target, &prepared)?;
        for body in prepared.bodies() {
            generator.compile_body(body)?;
        }
        let module = generator.module;
        module
            .verify()
            .map_err(|error| BackendError::InvalidMir(error.to_string()))?;
        if options.optimization > 0 {
            let pipeline = format!("default<O{}>", options.optimization.min(3));
            module
                .run_passes(&pipeline, &machine, PassBuilderOptions::create())
                .map_err(|error| invalid_mir(format!("LLVM optimization failed: {error}")))?;
        }

        outputs
            .iter()
            .map(|output| {
                match output.kind {
                    ArtifactKind::LlvmIr => module.print_to_file(&output.path),
                    ArtifactKind::Object => {
                        machine.write_to_file(&module, FileType::Object, &output.path)
                    }
                }
                .map_err(|error| BackendError::Output(error.to_string()))?;
                Ok(Artifact {
                    kind: output.kind,
                    path: output.path.clone(),
                })
            })
            .collect()
    }
}

/// The LLVM relocation mode of `model`. PIE code is PIC code; the module
/// flags set by [`set_pic_flags`] tell LLVM it is for an executable.
fn reloc_mode(model: RelocationModel) -> RelocMode {
    match model {
        RelocationModel::Static => RelocMode::Static,
        RelocationModel::Pic | RelocationModel::Pie => RelocMode::PIC,
        RelocationModel::DynamicNoPic => RelocMode::DynamicNoPic,
    }
}

/// Records position independence in the module, as clang does: `PIC Level`
/// for PIC and PIE code, and `PIE Level` for PIE code, which lets LLVM
/// access the module's own symbols directly.
///
/// clang merges these flags with `Min`/`Max`, which inkwell does not expose.
/// Xenon emits a single module, so the merge behavior never applies; `Error`
/// keeps any future mismatch loud.
fn set_pic_flags<'ctx>(context: &'ctx Context, module: &Module<'ctx>, model: RelocationModel) {
    let level = context.i32_type().const_int(2, false);
    if matches!(model, RelocationModel::Pic | RelocationModel::Pie) {
        module.add_basic_value_flag("PIC Level", FlagBehavior::Error, level);
    }
    if model == RelocationModel::Pie {
        module.add_basic_value_flag("PIE Level", FlagBehavior::Error, level);
    }
}

fn optimization_level(level: u8) -> OptimizationLevel {
    match level {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        2 => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    }
}

/// Translates the MIR bodies of one program into an LLVM module.
///
/// Every local lives in a stack slot; LLVM's `mem2reg` promotes them.
struct MirCodegen<'ctx, 'a> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    target: &'a TargetSpec,
    functions: HashMap<DefId, FunctionValue<'ctx>>,
    /// The stack slots of the body being compiled.
    local_slots: HashMap<Local, PointerValue<'ctx>>,
}

impl<'ctx, 'a> MirCodegen<'ctx, 'a> {
    fn new(
        context: &'ctx Context,
        module: Module<'ctx>,
        target: &'a TargetSpec,
        program: &MirProgram,
    ) -> Result<Self, BackendError> {
        let mut this = Self {
            context,
            module,
            builder: context.create_builder(),
            target,
            functions: HashMap::new(),
            local_slots: HashMap::new(),
        };
        this.declare_functions(program)?;
        Ok(this)
    }

    fn declare_functions(&mut self, program: &MirProgram) -> Result<(), BackendError> {
        let symbols = Symbols::for_program(program);
        for (id, declaration) in program.decls().iter_enumerated() {
            let params = declaration
                .sig
                .inputs
                .iter()
                .map(|ty| self.llvm_type(ty).map(BasicMetadataTypeEnum::from))
                .collect::<Result<Vec<_>, _>>()?;
            let function_type = self
                .llvm_type(&declaration.sig.output)?
                .fn_type(&params, false);
            let name = symbols
                .name(id)
                .ok_or_else(|| invalid_mir(format!("missing symbol for function {id}")))?;
            let function = self.module.add_function(name, function_type, None);
            self.functions.insert(id, function);
        }
        Ok(())
    }

    fn compile_body(&mut self, body: &Body) -> Result<(), BackendError> {
        let function = self.function(body.def_id())?;
        let blocks: Vec<LlvmBlock<'ctx>> = body
            .basic_blocks()
            .iter()
            .map(|_| self.context.append_basic_block(function, "bb"))
            .collect();
        let entry = *blocks
            .first()
            .ok_or_else(|| invalid_mir("function has no entry block".to_owned()))?;

        self.builder.position_at_end(entry);
        self.local_slots.clear();
        for (local, decl) in body.local_decls().iter_enumerated() {
            let slot = self
                .builder
                .build_alloca(self.llvm_type(&decl.ty)?, &local.to_string())
                .map_err(llvm_error("alloca local"))?;
            self.local_slots.insert(local, slot);
        }
        for (parameter, local) in function.get_param_iter().zip(body.args_iter()) {
            self.builder
                .build_store(self.local_slots[&local], parameter)
                .map_err(llvm_error("store parameter"))?;
        }

        for (block, data) in body.basic_blocks().iter_enumerated() {
            self.builder.position_at_end(blocks[block.index()]);
            for statement in &data.statements {
                match &statement.kind {
                    StatementKind::Assign(assign) => {
                        let (place, rvalue) = &**assign;
                        let value = self.rvalue(rvalue, body)?;
                        let destination = self.place_ptr(place, body)?;
                        self.builder
                            .build_store(destination, value)
                            .map_err(llvm_error("store assignment"))?;
                    }
                    // Every local has a slot for the whole function.
                    StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::Nop => {}
                }
            }
            self.terminator(&data.terminator.kind, body, &blocks)?;
        }
        Ok(())
    }

    fn terminator(
        &mut self,
        terminator: &TerminatorKind,
        body: &Body,
        blocks: &[LlvmBlock<'ctx>],
    ) -> Result<(), BackendError> {
        let block = |target: &BasicBlock| blocks[target.index()];
        match terminator {
            TerminatorKind::Goto { target } => {
                self.builder
                    .build_unconditional_branch(block(target))
                    .map_err(llvm_error("branch"))?;
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                let value = self.operand(discr, body)?.into_int_value();
                self.switch(value, targets, blocks)?;
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
            } => {
                let callee = self.function(*func)?;
                let values = args
                    .iter()
                    .map(|arg| self.operand(arg, body).map(BasicMetadataValueEnum::from))
                    .collect::<Result<Vec<_>, _>>()?;
                let call = self
                    .builder
                    .build_call(callee, &values, "call")
                    .map_err(llvm_error("call"))?;
                if let Some(value) = call.try_as_basic_value().basic() {
                    let place = self.place_ptr(destination, body)?;
                    self.builder
                        .build_store(place, value)
                        .map_err(llvm_error("store call result"))?;
                }
                match target {
                    Some(target) => self.builder.build_unconditional_branch(block(target)),
                    None => self.builder.build_unreachable(),
                }
                .map_err(llvm_error("call continuation"))?;
            }
            TerminatorKind::Assert {
                cond,
                expected,
                kind: _,
                target,
            } => {
                let condition = self.operand(cond, body)?.into_int_value();
                let condition = if *expected {
                    condition
                } else {
                    self.builder
                        .build_not(condition, "assert.not")
                        .map_err(llvm_error("assert condition"))?
                };
                let function = self.function(body.def_id())?;
                let trap = self.context.append_basic_block(function, "assert.fail");
                self.builder
                    .build_conditional_branch(condition, block(target), trap)
                    .map_err(llvm_error("assert branch"))?;
                self.builder.position_at_end(trap);
                self.build_trap()?;
            }
            TerminatorKind::Return => {
                let return_type = self.llvm_type(body.return_ty())?;
                let value = self
                    .builder
                    .build_load(return_type, self.local_slots[&RETURN_PLACE], "return")
                    .map_err(llvm_error("load return value"))?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(llvm_error("return"))?;
            }
            TerminatorKind::Unreachable => {
                self.builder
                    .build_unreachable()
                    .map_err(llvm_error("unreachable"))?;
            }
            TerminatorKind::EndOfBody => {
                return Err(invalid_mir(
                    "Runtime MIR contains end-of-body terminator".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Aborts the program: calls `llvm.trap` and ends the block.
    fn build_trap(&self) -> Result<(), BackendError> {
        let trap = self.module.get_function("llvm.trap").unwrap_or_else(|| {
            let ty = self.context.void_type().fn_type(&[], false);
            self.module.add_function("llvm.trap", ty, None)
        });
        self.builder
            .build_call(trap, &[], "")
            .map_err(llvm_error("trap call"))?;
        self.builder
            .build_unreachable()
            .map_err(llvm_error("trap unreachable"))?;
        Ok(())
    }

    fn switch(
        &self,
        discr: IntValue<'ctx>,
        targets: &SwitchTargets,
        blocks: &[LlvmBlock<'ctx>],
    ) -> Result<(), BackendError> {
        let cases: Vec<_> = targets
            .iter()
            .map(|(value, target)| {
                (
                    bigint_constant(discr.get_type(), value),
                    blocks[target.index()],
                )
            })
            .collect();
        self.builder
            .build_switch(discr, blocks[targets.otherwise().index()], &cases)
            .map_err(llvm_error("switch"))?;
        Ok(())
    }

    fn rvalue(&self, rvalue: &Rvalue, body: &Body) -> Result<BasicValueEnum<'ctx>, BackendError> {
        match rvalue {
            Rvalue::Use(operand) => self.operand(operand, body),
            Rvalue::UnaryOp(op, operand) => {
                let value = self.operand(operand, body)?;
                let ty = operand_type(operand, body)?;
                self.unary(*op, value, ty)
            }
            Rvalue::BinaryOp(op, operands) => {
                let (lhs, rhs) = &**operands;
                let ty = operand_type(lhs, body)?;
                let rhs_ty = operand_type(rhs, body)?;
                self.binary(
                    *op,
                    self.operand(lhs, body)?,
                    self.operand(rhs, body)?,
                    ty,
                    rhs_ty,
                )
            }
            Rvalue::Overflows(op, operands) => {
                let (lhs, rhs) = &**operands;
                let ty = operand_type(lhs, body)?;
                let lhs = self.operand(lhs, body)?.into_int_value();
                let rhs = self.operand(rhs, body)?.into_int_value();
                self.overflows(*op, lhs, rhs, ty).map(Into::into)
            }
            Rvalue::Cast(kind, operand, target) => {
                let value = self.operand(operand, body)?;
                self.cast(*kind, value, operand_type(operand, body)?, target)
            }
            Rvalue::CastLoses(kind, operand, target) => {
                let value = self.operand(operand, body)?;
                self.cast_loses(*kind, value, operand_type(operand, body)?, target)
                    .map(Into::into)
            }
            Rvalue::AddressOf(_, place) => Ok(self.place_ptr(place, body)?.into()),
            Rvalue::Aggregate(_, operands) => {
                let ty = typing::rvalue_ty(rvalue, body)
                    .map_err(|error| invalid_mir(error.to_string()))?;
                let elems = operands
                    .iter()
                    .map(|operand| self.operand(operand, body))
                    .collect::<Result<Vec<_>, _>>()?;
                self.tuple(&ty, elems)
            }
        }
    }

    /// Builds a value of the tuple type `ty` from its elements.
    fn tuple(
        &self,
        ty: &Type,
        elems: Vec<BasicValueEnum<'ctx>>,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let struct_ty = self.llvm_type(ty)?.into_struct_type();
        let mut value: AggregateValueEnum<'ctx> = struct_ty.get_poison().into();
        for (index, elem) in elems.into_iter().enumerate() {
            value = self
                .builder
                .build_insert_value(value, elem, index as u32, "tuple")
                .map_err(llvm_error("insert tuple element"))?;
        }
        Ok(value.into_struct_value().into())
    }

    fn unary(
        &self,
        op: UnOp,
        value: BasicValueEnum<'ctx>,
        ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let value = match (op, value) {
            (UnOp::Neg, BasicValueEnum::IntValue(value)) => {
                self.builder.build_int_neg(value, "neg").map(Into::into)
            }
            (UnOp::Neg, BasicValueEnum::FloatValue(value)) => {
                self.builder.build_float_neg(value, "fneg").map(Into::into)
            }
            (UnOp::Not, BasicValueEnum::IntValue(value)) if ty.is_bool() => {
                let zero = value.get_type().const_zero();
                self.builder
                    .build_int_compare(IntPredicate::EQ, value, zero, "not")
                    .map(Into::into)
            }
            (UnOp::Not, BasicValueEnum::IntValue(value)) => {
                self.builder.build_not(value, "bitnot").map(Into::into)
            }
            _ => return Err(unsupported(format!("unary operation {op:?} on `{ty}`"))),
        };
        value.map_err(llvm_error("unary operation"))
    }

    /// Lowers a binary operation whose left operand has MIR type `ty` and
    /// right operand `rhs_ty`; they differ only for shifts.
    fn binary(
        &self,
        op: BinOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
        ty: &Type,
        rhs_ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let unsupported_op = || unsupported(format!("operator {op:?} on `{ty}`"));
        let builder = &self.builder;
        let value = match (lhs, rhs) {
            (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) => match op {
                BinOp::Add => builder.build_float_add(lhs, rhs, "fadd").map(Into::into),
                BinOp::Sub => builder.build_float_sub(lhs, rhs, "fsub").map(Into::into),
                BinOp::Mul => builder.build_float_mul(lhs, rhs, "fmul").map(Into::into),
                BinOp::Div => builder.build_float_div(lhs, rhs, "fdiv").map(Into::into),
                BinOp::Rem => builder.build_float_rem(lhs, rhs, "frem").map(Into::into),
                _ => {
                    let predicate = float_predicate(op).ok_or_else(unsupported_op)?;
                    builder
                        .build_float_compare(predicate, lhs, rhs, "fcmp")
                        .map(Into::into)
                }
            },
            // Pointers only compare for equality, as addresses.
            (BasicValueEnum::PointerValue(lhs), BasicValueEnum::PointerValue(rhs)) => {
                let predicate = match op {
                    BinOp::Eq => IntPredicate::EQ,
                    BinOp::Ne => IntPredicate::NE,
                    _ => return Err(unsupported_op()),
                };
                let int_type = self.int_type(self.target.pointer_width())?;
                let lhs = builder
                    .build_ptr_to_int(lhs, int_type, "ptr.lhs")
                    .map_err(llvm_error("pointer comparison cast"))?;
                let rhs = builder
                    .build_ptr_to_int(rhs, int_type, "ptr.rhs")
                    .map_err(llvm_error("pointer comparison cast"))?;
                builder
                    .build_int_compare(predicate, lhs, rhs, "ptr.cmp")
                    .map(Into::into)
            }
            (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => {
                let signed = ty.is_signed_integer();
                // MIR allows a shift amount of any integer type, extended by
                // its own signedness.
                let rhs = if op.is_shift() && lhs.get_type() != rhs.get_type() {
                    builder
                        .build_int_cast_sign_flag(
                            rhs,
                            lhs.get_type(),
                            rhs_ty.is_signed_integer(),
                            "shift.amount",
                        )
                        .map_err(llvm_error("shift amount conversion"))?
                } else {
                    rhs
                };
                match op {
                    BinOp::Add => builder.build_int_add(lhs, rhs, "add"),
                    BinOp::Sub => builder.build_int_sub(lhs, rhs, "sub"),
                    BinOp::Mul => builder.build_int_mul(lhs, rhs, "mul"),
                    BinOp::Div if signed => builder.build_int_signed_div(lhs, rhs, "sdiv"),
                    BinOp::Div => builder.build_int_unsigned_div(lhs, rhs, "udiv"),
                    BinOp::Rem if signed => builder.build_int_signed_rem(lhs, rhs, "srem"),
                    BinOp::Rem => builder.build_int_unsigned_rem(lhs, rhs, "urem"),
                    BinOp::BitAnd => builder.build_and(lhs, rhs, "and"),
                    BinOp::BitOr => builder.build_or(lhs, rhs, "or"),
                    BinOp::BitXor => builder.build_xor(lhs, rhs, "xor"),
                    BinOp::Shl => builder.build_left_shift(lhs, rhs, "shl"),
                    BinOp::Shr => builder.build_right_shift(lhs, rhs, signed, "shr"),
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        let predicate = int_predicate(op, signed).ok_or_else(unsupported_op)?;
                        builder.build_int_compare(predicate, lhs, rhs, "cmp")
                    }
                }
                .map(Into::into)
            }
            _ => return Err(unsupported_op()),
        };
        value.map_err(llvm_error("binary operation"))
    }

    /// Whether `op` overflows the integer type `ty`: the operation is
    /// redone at twice the width and compared with the narrowed result.
    fn overflows(
        &self,
        op: BinOp,
        lhs: IntValue<'ctx>,
        rhs: IntValue<'ctx>,
        ty: &Type,
    ) -> Result<IntValue<'ctx>, BackendError> {
        let width = lhs.get_type().get_bit_width();
        let wide_type = self.int_type(
            width
                .checked_mul(2)
                .ok_or_else(|| unsupported("integer type width is too large".to_owned()))?,
        )?;
        let extend = |value| {
            if ty.is_signed_integer() {
                self.builder
                    .build_int_s_extend(value, wide_type, "overflow.extend")
            } else {
                self.builder
                    .build_int_z_extend(value, wide_type, "overflow.extend")
            }
            .map_err(llvm_error("overflow operand extension"))
        };
        let (lhs_wide, rhs_wide) = (extend(lhs)?, extend(rhs)?);
        let wide_result = match op {
            BinOp::Add => self
                .builder
                .build_int_add(lhs_wide, rhs_wide, "overflow.add"),
            BinOp::Sub => self
                .builder
                .build_int_sub(lhs_wide, rhs_wide, "overflow.sub"),
            BinOp::Mul => self
                .builder
                .build_int_mul(lhs_wide, rhs_wide, "overflow.mul"),
            _ => return Err(invalid_mir(format!("overflow check for {op:?}"))),
        }
        .map_err(llvm_error("extended overflow operation"))?;
        let narrowed = self
            .builder
            .build_int_truncate(wide_result, lhs.get_type(), "overflow.narrow")
            .map_err(llvm_error("overflow truncation"))?;
        self.builder
            .build_int_compare(IntPredicate::NE, wide_result, extend(narrowed)?, "overflow")
            .map_err(llvm_error("overflow comparison"))
    }

    /// The raw conversion of `value` from `source` to `target`; the
    /// runtime guard has already ruled out values it would change.
    fn cast(
        &self,
        kind: CastKind,
        value: BasicValueEnum<'ctx>,
        source: &Type,
        target: &Type,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let target_type = self.llvm_type(target)?;
        let builder = &self.builder;
        let value = match kind {
            CastKind::IntToInt => {
                let value = value.into_int_value();
                let (from, to) = (
                    value.get_type().get_bit_width(),
                    target_type.into_int_type().get_bit_width(),
                );
                let target_type = target_type.into_int_type();
                if from == to {
                    return Ok(value.into());
                } else if from > to {
                    builder.build_int_truncate(value, target_type, "trunc")
                } else if source.is_signed_integer() {
                    builder.build_int_s_extend(value, target_type, "sext")
                } else {
                    builder.build_int_z_extend(value, target_type, "zext")
                }
                .map(Into::into)
            }
            CastKind::IntToFloat => {
                let value = value.into_int_value();
                return Ok(self
                    .int_to_float(value, source, target_type.into_float_type())?
                    .into());
            }
            CastKind::FloatToInt => {
                let value = value.into_float_value();
                return Ok(self
                    .float_to_int(value, target, target_type.into_int_type())?
                    .into());
            }
            CastKind::FloatToFloat => {
                let value = value.into_float_value();
                return Ok(self.convert_float(value, source, target)?.into());
            }
            CastKind::PtrToPtr => return Ok(value),
            CastKind::PtrToInt => builder
                .build_ptr_to_int(
                    value.into_pointer_value(),
                    target_type.into_int_type(),
                    "ptrtoint",
                )
                .map(Into::into),
            CastKind::IntToPtr => builder
                .build_int_to_ptr(
                    value.into_int_value(),
                    target_type.into_pointer_type(),
                    "inttoptr",
                )
                .map(Into::into),
        };
        value.map_err(llvm_error("cast"))
    }

    /// Whether the cast of `value` from `source` to `target` would not
    /// represent it exactly. Never poison, whatever `value` holds: raw
    /// conversions that may be poison for out-of-range values are only
    /// selected when the value is in range.
    fn cast_loses(
        &self,
        kind: CastKind,
        value: BasicValueEnum<'ctx>,
        source: &Type,
        target: &Type,
    ) -> Result<IntValue<'ctx>, BackendError> {
        let builder = &self.builder;
        let bool_type = self.context.bool_type();
        let error = || llvm_error("cast check");
        match kind {
            CastKind::PtrToPtr | CastKind::PtrToInt | CastKind::IntToPtr => {
                Ok(bool_type.const_zero())
            }
            // Compare in a signed type one bit wider than both, which holds
            // every value of either.
            CastKind::IntToInt => {
                let value = value.into_int_value();
                let (min, max) = self.int_bounds(target)?;
                let width = value
                    .get_type()
                    .get_bit_width()
                    .max(self.int_width(target)?)
                    + 1;
                let wide_type = self.int_type(width)?;
                let wide = if source.is_signed_integer() {
                    builder.build_int_s_extend(value, wide_type, "cast.wide")
                } else {
                    builder.build_int_z_extend(value, wide_type, "cast.wide")
                }
                .map_err(error())?;
                let below = builder
                    .build_int_compare(
                        IntPredicate::SLT,
                        wide,
                        bigint_constant(wide_type, &min),
                        "cast.below",
                    )
                    .map_err(error())?;
                let above = builder
                    .build_int_compare(
                        IntPredicate::SGT,
                        wide,
                        bigint_constant(wide_type, &max),
                        "cast.above",
                    )
                    .map_err(error())?;
                builder
                    .build_or(below, above, "cast.loses")
                    .map_err(error())
            }
            // Lossy unless `min <= x < max + 1` and `x` is integral; NaN
            // fails the ordered comparisons.
            CastKind::FloatToInt => {
                let value = value.into_float_value();
                let float_type = value.get_type();
                let width = self.int_width(target)?;
                if width == 0 {
                    return builder
                        .build_float_compare(
                            FloatPredicate::UNE,
                            value,
                            float_type.const_zero(),
                            "cast.loses",
                        )
                        .map_err(error());
                }
                let (min, max) = self.int_bounds(target)?;
                let low = self.float_constant(float_type, &min)?;
                let high = self.float_constant(float_type, &(max + 1))?;
                let above_low = builder
                    .build_float_compare(FloatPredicate::OGE, value, low, "cast.above_low")
                    .map_err(error())?;
                let below_high = builder
                    .build_float_compare(FloatPredicate::OLT, value, high, "cast.below_high")
                    .map_err(error())?;
                let in_range = builder
                    .build_and(above_low, below_high, "cast.in_range")
                    .map_err(error())?;
                let truncated = self.float_to_int(value, target, self.int_type(width)?)?;
                let back = self.int_to_float(truncated, target, float_type)?;
                let fractional = builder
                    .build_float_compare(FloatPredicate::UNE, back, value, "cast.fractional")
                    .map_err(error())?;
                self.select_loss(in_range, fractional)
            }
            // Lossy unless the rounded float is below `max + 1` (so it
            // converts back without poison) and converts back to `value`.
            CastKind::IntToFloat => {
                let value = value.into_int_value();
                let float_type = self.llvm_type(target)?.into_float_type();
                let converted = self.int_to_float(value, source, float_type)?;
                let (_, max) = self.int_bounds(source)?;
                let high = self.float_constant(float_type, &(max + 1))?;
                let in_range = builder
                    .build_float_compare(FloatPredicate::OLT, converted, high, "cast.in_range")
                    .map_err(error())?;
                let back = self.float_to_int(converted, source, value.get_type())?;
                let rounded = builder
                    .build_int_compare(IntPredicate::NE, back, value, "cast.rounded")
                    .map_err(error())?;
                self.select_loss(in_range, rounded)
            }
            // Lossy if converting back changes a number; NaN stays NaN.
            CastKind::FloatToFloat => {
                let value = value.into_float_value();
                let converted = self.convert_float(value, source, target)?;
                let back = self.convert_float(converted, target, source)?;
                let changed = builder
                    .build_float_compare(FloatPredicate::UNE, back, value, "cast.changed")
                    .map_err(error())?;
                let is_number = builder
                    .build_float_compare(FloatPredicate::ORD, value, value, "cast.is_number")
                    .map_err(error())?;
                builder
                    .build_and(changed, is_number, "cast.loses")
                    .map_err(error())
            }
        }
    }

    /// `in_range ? loses : true`. A `select` ignores the arm it does not
    /// pick, so `loses` may be poison when `in_range` is false.
    fn select_loss(
        &self,
        in_range: IntValue<'ctx>,
        loses: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, BackendError> {
        let always = self.context.bool_type().const_all_ones();
        self.builder
            .build_select(in_range, loses, always, "cast.loses")
            .map(BasicValueEnum::into_int_value)
            .map_err(llvm_error("cast check"))
    }

    /// Converts an integer of type `source` to a float, rounding to nearest.
    fn int_to_float(
        &self,
        value: IntValue<'ctx>,
        source: &Type,
        float_type: FloatType<'ctx>,
    ) -> Result<FloatValue<'ctx>, BackendError> {
        if source.is_signed_integer() {
            self.builder
                .build_signed_int_to_float(value, float_type, "sitofp")
        } else {
            self.builder
                .build_unsigned_int_to_float(value, float_type, "uitofp")
        }
        .map_err(llvm_error("int to float conversion"))
    }

    /// Converts a float to an integer of type `target`, rounding toward
    /// zero; poison if the result does not fit.
    fn float_to_int(
        &self,
        value: FloatValue<'ctx>,
        target: &Type,
        int_type: IntType<'ctx>,
    ) -> Result<IntValue<'ctx>, BackendError> {
        if target.is_signed_integer() {
            self.builder
                .build_float_to_signed_int(value, int_type, "fptosi")
        } else {
            self.builder
                .build_float_to_unsigned_int(value, int_type, "fptoui")
        }
        .map_err(llvm_error("float to int conversion"))
    }

    /// Converts a float between float types, rounding to nearest. LLVM only
    /// extends to wider and truncates to narrower types, so `f16` and
    /// `bf16` convert through `f32`.
    fn convert_float(
        &self,
        value: FloatValue<'ctx>,
        source: &Type,
        target: &Type,
    ) -> Result<FloatValue<'ctx>, BackendError> {
        if source == target {
            return Ok(value);
        }
        let target_type = self.llvm_type(target)?.into_float_type();
        let width = |ty: &Type| {
            ty.float_width()
                .ok_or_else(|| invalid_mir(format!("float conversion of `{ty}`")))
        };
        let builder = &self.builder;
        match width(source)?.cmp(&width(target)?) {
            std::cmp::Ordering::Less => builder.build_float_ext(value, target_type, "fpext"),
            std::cmp::Ordering::Greater => builder.build_float_trunc(value, target_type, "fptrunc"),
            std::cmp::Ordering::Equal => {
                let wide = builder
                    .build_float_ext(value, self.context.f32_type(), "fpext")
                    .map_err(llvm_error("float conversion"))?;
                builder.build_float_trunc(wide, target_type, "fptrunc")
            }
        }
        .map_err(llvm_error("float conversion"))
    }

    /// The float nearest to the integer `value`, infinite beyond the
    /// range of `float_type`. Exact for the powers of two and zero that
    /// bound cast checks.
    fn float_constant(
        &self,
        float_type: FloatType<'ctx>,
        value: &BigInt,
    ) -> Result<FloatValue<'ctx>, BackendError> {
        let width = u32::try_from(value.bits())
            .ok()
            .and_then(|bits| bits.checked_add(1))
            .ok_or_else(|| unsupported("integer bound is too wide".to_owned()))?;
        let int_type = self.int_type(width)?;
        self.builder
            .build_signed_int_to_float(bigint_constant(int_type, value), float_type, "bound")
            .map_err(llvm_error("float bound"))
    }

    /// The bit width of the integer type `ty` on the target.
    fn int_width(&self, ty: &Type) -> Result<u32, BackendError> {
        self.target
            .int_width(ty)
            .ok_or_else(|| invalid_mir(format!("`{ty}` is not an integer type")))
    }

    /// The inclusive range of the integer type `ty` on the target.
    fn int_bounds(&self, ty: &Type) -> Result<(BigInt, BigInt), BackendError> {
        self.target
            .int_bounds(ty)
            .ok_or_else(|| invalid_mir(format!("`{ty}` is not an integer type")))
    }

    fn operand(
        &self,
        operand: &Operand,
        body: &Body,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        match operand {
            Operand::Copy(place) => {
                let ty = typing::place_ty(place, body)
                    .map_err(|error| invalid_mir(error.to_string()))?;
                self.builder
                    .build_load(self.llvm_type(ty)?, self.place_ptr(place, body)?, "load")
                    .map_err(llvm_error("load operand"))
            }
            Operand::Constant(constant) => self.constant(constant),
        }
    }

    fn constant(&self, constant: &Constant) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let ty = self.llvm_type(&constant.ty)?;
        Ok(match &constant.value {
            ConstValue::Bool(value) => ty
                .into_int_type()
                .const_int(u64::from(*value), false)
                .into(),
            ConstValue::Int(_) if constant.ty.is_zero_sized() => ty.const_zero(),
            ConstValue::Int(value) => bigint_constant(ty.into_int_type(), value).into(),
            ConstValue::Float(value) => ty.into_float_type().const_float(*value).into(),
            ConstValue::Address(value) => {
                let address = bigint_constant(self.int_type(self.target.pointer_width())?, value);
                self.builder
                    .build_int_to_ptr(address, ty.into_pointer_type(), "address")
                    .map_err(llvm_error("address constant conversion"))?
                    .into()
            }
            ConstValue::Tuple(_) => {
                let elems = constant
                    .tuple_elems()
                    .ok_or_else(|| invalid_mir(format!("malformed tuple constant `{constant}`")))?
                    .iter()
                    .map(|elem| self.constant(elem))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.tuple(&constant.ty, elems);
            }
        })
    }

    /// The address of `place`, loading every pointer it dereferences.
    fn place_ptr(&self, place: &Place, body: &Body) -> Result<PointerValue<'ctx>, BackendError> {
        let mut pointer = *self
            .local_slots
            .get(&place.local)
            .ok_or_else(|| invalid_mir(format!("missing stack slot for {}", place.local)))?;
        let mut ty = &body.local_decls()[place.local].ty;
        for elem in &place.projection {
            match elem {
                ProjectionElem::Deref => {
                    pointer = self
                        .builder
                        .build_load(self.llvm_type(ty)?, pointer, "deref")
                        .map_err(llvm_error("load pointer"))?
                        .into_pointer_value();
                    ty = ty
                        .pointee()
                        .ok_or_else(|| invalid_mir("dereference of non-pointer".to_owned()))?;
                }
                ProjectionElem::Field(index) => {
                    pointer = self
                        .builder
                        .build_struct_gep(self.llvm_type(ty)?, pointer, *index, "field")
                        .map_err(llvm_error("tuple element address"))?;
                    ty = ty
                        .tuple_elems()
                        .and_then(|elems| elems.get(*index as usize))
                        .ok_or_else(|| invalid_mir(format!("no element {index} in `{ty}`")))?;
                }
            }
        }
        Ok(pointer)
    }

    fn function(&self, def_id: DefId) -> Result<FunctionValue<'ctx>, BackendError> {
        self.functions
            .get(&def_id)
            .copied()
            .ok_or_else(|| invalid_mir(format!("unknown function {def_id}")))
    }

    fn int_type(&self, width: u32) -> Result<IntType<'ctx>, BackendError> {
        let width =
            NonZero::new(width).ok_or_else(|| unsupported("zero-width integer".to_owned()))?;
        self.context
            .custom_width_int_type(width)
            .map_err(|error| unsupported(error.to_string()))
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, BackendError> {
        Ok(match ty {
            Type::Bool => self.context.bool_type().into(),
            // Runtime MIR never computes on a zero-sized integer, so it only
            // needs a size-zero value to store, pass, and return.
            Type::Int(0) | Type::UInt(0) => self.context.struct_type(&[], false).into(),
            Type::Int(width) | Type::UInt(width) => self.int_type(*width)?.into(),
            Type::USize | Type::ISize => self.int_type(self.target.pointer_width())?.into(),
            Type::Float16 => self.context.f16_type().into(),
            Type::BFloat16 => self.context.bf16_type().into(),
            Type::Float32 => self.context.f32_type().into(),
            Type::Float64 => self.context.f64_type().into(),
            Type::Float128 => self.context.f128_type().into(),
            Type::Pointer(_) | Type::Reference(_) => {
                self.context.ptr_type(AddressSpace::default()).into()
            }
            Type::Tuple(elems) => {
                let fields = elems
                    .iter()
                    .map(|elem| self.llvm_type(elem))
                    .collect::<Result<Vec<_>, _>>()?;
                self.context.struct_type(&fields, false).into()
            }
        })
    }
}

/// The integer comparison `op` performs, `None` if it is no comparison.
fn int_predicate(op: BinOp, signed: bool) -> Option<IntPredicate> {
    Some(match (op, signed) {
        (BinOp::Eq, _) => IntPredicate::EQ,
        (BinOp::Ne, _) => IntPredicate::NE,
        (BinOp::Lt, true) => IntPredicate::SLT,
        (BinOp::Lt, false) => IntPredicate::ULT,
        (BinOp::Le, true) => IntPredicate::SLE,
        (BinOp::Le, false) => IntPredicate::ULE,
        (BinOp::Gt, true) => IntPredicate::SGT,
        (BinOp::Gt, false) => IntPredicate::UGT,
        (BinOp::Ge, true) => IntPredicate::SGE,
        (BinOp::Ge, false) => IntPredicate::UGE,
        _ => return None,
    })
}

/// The float comparison `op` performs, `None` if it is no comparison.
/// All are ordered (false on NaN) except `!=`, which is true on NaN.
fn float_predicate(op: BinOp) -> Option<FloatPredicate> {
    Some(match op {
        BinOp::Eq => FloatPredicate::OEQ,
        BinOp::Ne => FloatPredicate::UNE,
        BinOp::Lt => FloatPredicate::OLT,
        BinOp::Le => FloatPredicate::OLE,
        BinOp::Gt => FloatPredicate::OGT,
        BinOp::Ge => FloatPredicate::OGE,
        _ => return None,
    })
}

/// The constant `value` of `int_type`, truncated to its width in two's
/// complement.
fn bigint_constant<'ctx>(int_type: IntType<'ctx>, value: &BigInt) -> IntValue<'ctx> {
    let width = int_type.get_bit_width() as usize;
    let fill = if value.sign() == Sign::Minus {
        u64::MAX
    } else {
        0
    };
    let mut words = vec![fill; width.div_ceil(64)];
    for (index, byte) in value.to_signed_bytes_le().into_iter().enumerate() {
        let shift = (index % 8) * 8;
        if let Some(word) = words.get_mut(index / 8) {
            *word = (*word & !(0xff_u64 << shift)) | (u64::from(byte) << shift);
        }
    }
    if let Some(last) = words.last_mut()
        && !width.is_multiple_of(64)
    {
        *last &= (1_u64 << (width % 64)) - 1;
    }
    int_type.const_int_arbitrary_precision(&words)
}

fn operand_type<'a>(operand: &'a Operand, body: &'a Body) -> Result<&'a Type, BackendError> {
    typing::operand_ty(operand, body).map_err(|error| invalid_mir(error.to_string()))
}

fn invalid_mir(message: String) -> BackendError {
    BackendError::InvalidMir(message)
}

fn unsupported(message: String) -> BackendError {
    BackendError::Unsupported(message)
}

fn llvm_error(operation: &'static str) -> impl FnOnce(BuilderError) -> BackendError {
    move |error| invalid_mir(format!("{operation}: {error:?}"))
}
