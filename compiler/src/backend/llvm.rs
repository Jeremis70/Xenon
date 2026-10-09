//! LLVM implementation of the backend contract. This module only consumes MIR.

use std::collections::HashMap;
use std::num::NonZero;

use inkwell::basic_block::BasicBlock as LlvmBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, IntType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};
use num_bigint::{BigInt, Sign};

use crate::backend::contract::{
    Artifact, ArtifactKind, Backend, BackendError, CodegenOptions, OutputRequest,
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
                RelocMode::Default,
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
        let mut generator = MirCodegen::new(&context, module, target, &prepared)?;
        for body in prepared.bodies() {
            generator.compile_body(body)?;
        }
        let module = generator.module;
        module
            .verify()
            .map_err(|error| BackendError::InvalidMir(error.to_string()))?;

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
                self.binary(*op, self.operand(lhs, body)?, self.operand(rhs, body)?, ty)
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
            Rvalue::AddressOf(_, place) => Ok(self.place_ptr(place, body)?.into()),
        }
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

    /// Lowers a binary operation whose operands have MIR type `ty`.
    fn binary(
        &self,
        op: BinOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
        ty: &Type,
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
                // MIR allows a shift amount of any integer type.
                let rhs = if op.is_shift() && lhs.get_type() != rhs.get_type() {
                    builder
                        .build_int_cast(rhs, lhs.get_type(), "shift.amount")
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
                let (value, target_type) = (value.into_int_value(), target_type.into_float_type());
                if source.is_signed_integer() {
                    builder.build_signed_int_to_float(value, target_type, "sitofp")
                } else {
                    builder.build_unsigned_int_to_float(value, target_type, "uitofp")
                }
                .map(Into::into)
            }
            CastKind::FloatToInt => {
                let (value, target_type) = (value.into_float_value(), target_type.into_int_type());
                if target.is_signed_integer() {
                    builder.build_float_to_signed_int(value, target_type, "fptosi")
                } else {
                    builder.build_float_to_unsigned_int(value, target_type, "fptoui")
                }
                .map(Into::into)
            }
            CastKind::FloatToFloat => {
                let (value, target_type) =
                    (value.into_float_value(), target_type.into_float_type());
                if value.get_type() == target_type {
                    return Ok(value.into());
                }
                builder
                    .build_float_cast(value, target_type, "fpcast")
                    .map(Into::into)
            }
            CastKind::PtrToPtr => return Ok(value),
        };
        value.map_err(llvm_error("cast"))
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
            ConstValue::Int(value) => bigint_constant(ty.into_int_type(), value).into(),
            ConstValue::Float(value) => ty.into_float_type().const_float(*value).into(),
            ConstValue::Address(value) => {
                let address = bigint_constant(self.int_type(self.target.pointer_width())?, value);
                self.builder
                    .build_int_to_ptr(address, ty.into_pointer_type(), "address")
                    .map_err(llvm_error("address constant conversion"))?
                    .into()
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

/// The ordered float comparison `op` performs, `None` if it is no
/// comparison.
fn float_predicate(op: BinOp) -> Option<FloatPredicate> {
    Some(match op {
        BinOp::Eq => FloatPredicate::OEQ,
        BinOp::Ne => FloatPredicate::ONE,
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
