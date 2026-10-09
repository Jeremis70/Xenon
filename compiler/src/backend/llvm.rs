//! LLVM implementation of the backend contract. This module only consumes MIR.

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock as LlvmBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::{FloatPredicate, IntPredicate};
use std::collections::HashMap;

use crate::backend::contract::{
    Artifact, ArtifactKind, Backend, BackendError, CodegenOptions, OutputRequest,
};
use crate::backend::prepare::{Symbols, prepare_program};
use crate::index::Idx;
use crate::middle::ids::DefId;
use crate::middle::mir::verify::verify_program;
use crate::middle::mir::{
    BasicBlock, BinOp, Body, CastKind, ConstValue, Constant, Local, MirPhase, MirProgram, Operand,
    Place, ProjectionElem, Rvalue, StatementKind, SwitchTargets, TerminatorKind, UnOp,
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
        for body in program.bodies() {
            if body.phase() != MirPhase::Runtime {
                return Err(BackendError::InvalidMir(format!(
                    "function {:?} is in phase `{}`; LLVM requires Runtime MIR",
                    body.def_id(),
                    body.phase()
                )));
            }
        }
        let prepared = prepare_program(program, target).map_err(|error| {
            BackendError::InvalidMir(format!("backend preparation failed: {error}"))
        })?;
        verify_program(&prepared, target)
            .map_err(|errors| BackendError::InvalidMir(errors.to_string()))?;

        Target::initialize_native(&InitializationConfig::default())
            .map_err(|error| BackendError::Initialization(error.to_string()))?;
        let triple = TargetMachine::get_default_triple();
        let llvm_target = Target::from_triple(&triple)
            .map_err(|error| BackendError::Initialization(error.to_string()))?;
        let machine = llvm_target
            .create_target_machine(
                &triple,
                &TargetMachine::get_host_cpu_name().to_string(),
                &TargetMachine::get_host_cpu_features().to_string(),
                optimization_level(options.optimization),
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                BackendError::Initialization("could not create host target machine".to_owned())
            })?;
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
        let data_layout = machine.get_target_data().get_data_layout();
        module.set_data_layout(&data_layout);
        let mut generator = MirCodegen::new(&context, module, target, &prepared)?;
        generator.compile()?;
        generator
            .module
            .verify()
            .map_err(|error| BackendError::InvalidMir(error.to_string()))?;

        let mut artifacts = Vec::with_capacity(outputs.len());
        for output in outputs {
            match output.kind {
                ArtifactKind::LlvmIr => generator
                    .module
                    .print_to_file(&output.path)
                    .map_err(|error| BackendError::Output(error.to_string()))?,
                ArtifactKind::Object => machine
                    .write_to_file(&generator.module, FileType::Object, &output.path)
                    .map_err(|error| BackendError::Output(error.to_string()))?,
            }
            artifacts.push(Artifact {
                kind: output.kind,
                path: output.path.clone(),
            });
        }
        Ok(artifacts)
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

struct MirCodegen<'ctx, 'program, 'target> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    target: &'target TargetSpec,
    program: &'program MirProgram,
    symbols: Symbols,
    functions: HashMap<DefId, FunctionValue<'ctx>>,
    local_slots: HashMap<Local, PointerValue<'ctx>>,
}

impl<'ctx, 'program, 'target> MirCodegen<'ctx, 'program, 'target> {
    fn new(
        context: &'ctx Context,
        module: Module<'ctx>,
        target: &'target TargetSpec,
        program: &'program MirProgram,
    ) -> Result<Self, BackendError> {
        let mut this = Self {
            context,
            module,
            builder: context.create_builder(),
            target,
            program,
            symbols: Symbols::for_program(program),
            functions: HashMap::new(),
            local_slots: HashMap::new(),
        };
        this.declare_functions()?;
        Ok(this)
    }

    fn declare_functions(&mut self) -> Result<(), BackendError> {
        for (id, declaration) in self.program.decls().iter_enumerated() {
            let params = declaration
                .sig
                .inputs
                .iter()
                .map(|ty| self.llvm_type(ty))
                .collect::<Result<Vec<_>, _>>()?;
            let return_type = self.llvm_type(&declaration.sig.output)?;
            let params = params
                .iter()
                .map(|ty| ty.as_basic_type_enum().into())
                .collect::<Vec<BasicMetadataTypeEnum>>();
            let function_type = return_type.fn_type(&params, false);
            let name = self.symbols.name(id).ok_or_else(|| {
                BackendError::InvalidMir(format!("missing symbol for function {id:?}"))
            })?;
            let function = self.module.add_function(name, function_type, None);
            self.functions.insert(id, function);
        }
        Ok(())
    }

    fn compile(&mut self) -> Result<(), BackendError> {
        for body in self.program.bodies() {
            self.compile_body(body)?;
        }
        Ok(())
    }

    fn compile_body(&mut self, body: &Body) -> Result<(), BackendError> {
        let function = *self.functions.get(&body.def_id()).ok_or_else(|| {
            BackendError::InvalidMir(format!("missing LLVM function for {:?}", body.def_id()))
        })?;
        let blocks = body
            .basic_blocks()
            .iter()
            .map(|_| self.context.append_basic_block(function, "bb"))
            .collect::<Vec<_>>();
        let entry = *blocks
            .first()
            .ok_or_else(|| BackendError::InvalidMir("function has no entry block".to_owned()))?;
        self.local_slots.clear();
        self.builder.position_at_end(entry);
        for (local, decl) in body.local_decls().iter_enumerated() {
            let ty = self.llvm_type(&decl.ty)?;
            let slot = self
                .builder
                .build_alloca(ty, &local.to_string())
                .map_err(llvm_error("alloca local"))?;
            self.local_slots.insert(local, slot);
        }
        for (index, local) in body.args_iter().enumerate() {
            let parameter = function
                .get_nth_param(index as u32)
                .ok_or_else(|| BackendError::InvalidMir("missing function parameter".to_owned()))?;
            self.builder
                .build_store(self.local_slots[&local], parameter)
                .map_err(llvm_error("store parameter"))?;
        }

        for (block_id, block) in body.basic_blocks().iter_enumerated() {
            self.builder.position_at_end(blocks[block_id.index()]);
            for statement in &block.statements {
                if let StatementKind::Assign(assign) = &statement.kind {
                    let value = self.rvalue(&assign.1, body)?;
                    let destination = self.place_ptr(&assign.0, body)?;
                    self.builder
                        .build_store(destination, value)
                        .map_err(llvm_error("store assignment"))?;
                }
            }
            self.terminator(&block.terminator.kind, body, &blocks)?;
        }
        Ok(())
    }

    fn terminator(
        &mut self,
        terminator: &TerminatorKind,
        body: &Body,
        blocks: &[LlvmBlock<'ctx>],
    ) -> Result<(), BackendError> {
        match terminator {
            TerminatorKind::Goto { target } => self.branch(blocks, *target),
            TerminatorKind::SwitchInt { discr, targets } => {
                let value = self.operand(discr, body)?.into_int_value();
                self.switch(value, targets, blocks)
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
            } => {
                let callee = *self.functions.get(func).ok_or_else(|| {
                    BackendError::InvalidMir(format!("call references unknown function {func:?}"))
                })?;
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
                if let Some(target) = target {
                    self.branch(blocks, *target)
                } else {
                    self.builder
                        .build_unreachable()
                        .map_err(llvm_error("unreachable"))?;
                    Ok(())
                }
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
                let function = self
                    .builder
                    .get_insert_block()
                    .and_then(|block| block.get_parent())
                    .ok_or_else(|| {
                        BackendError::InvalidMir("assert outside function".to_owned())
                    })?;
                let trap = self.context.append_basic_block(function, "assert.fail");
                let continuation = blocks[target.index()];
                self.builder
                    .build_conditional_branch(condition, continuation, trap)
                    .map_err(llvm_error("assert branch"))?;
                self.builder.position_at_end(trap);
                let trap_fn = self.module.get_function("llvm.trap").unwrap_or_else(|| {
                    self.module.add_function(
                        "llvm.trap",
                        self.context.void_type().fn_type(&[], false),
                        None,
                    )
                });
                self.builder
                    .build_call(trap_fn, &[], "")
                    .map_err(llvm_error("trap call"))?;
                self.builder
                    .build_unreachable()
                    .map_err(llvm_error("trap unreachable"))?;
                Ok(())
            }
            TerminatorKind::Return => {
                let return_place = self.local_slots[&crate::middle::mir::RETURN_PLACE];
                let return_type = self.llvm_type(body.return_ty())?;
                let value = self
                    .builder
                    .build_load(return_type, return_place, "return")
                    .map_err(llvm_error("load return value"))?;
                self.builder
                    .build_return(Some(&value))
                    .map_err(llvm_error("return"))?;
                Ok(())
            }
            TerminatorKind::Unreachable => {
                self.builder
                    .build_unreachable()
                    .map_err(llvm_error("unreachable"))?;
                Ok(())
            }
            TerminatorKind::EndOfBody => Err(BackendError::InvalidMir(
                "Runtime MIR contains end-of-body terminator".to_owned(),
            )),
        }
    }

    fn branch(
        &mut self,
        blocks: &[LlvmBlock<'ctx>],
        target: BasicBlock,
    ) -> Result<(), BackendError> {
        self.builder
            .build_unconditional_branch(blocks[target.index()])
            .map_err(llvm_error("branch"))?;
        Ok(())
    }

    fn switch(
        &mut self,
        discr: inkwell::values::IntValue<'ctx>,
        targets: &SwitchTargets,
        blocks: &[LlvmBlock<'ctx>],
    ) -> Result<(), BackendError> {
        let cases = targets
            .iter()
            .map(|(value, target)| {
                Ok((
                    bigint_constant(discr.get_type(), value),
                    blocks[target.index()],
                ))
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        self.builder
            .build_switch(discr, blocks[targets.otherwise().index()], &cases)
            .map_err(llvm_error("switch"))?;
        Ok(())
    }

    fn rvalue(
        &mut self,
        rvalue: &Rvalue,
        body: &Body,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        match rvalue {
            Rvalue::Use(operand) => self.operand(operand, body),
            Rvalue::UnaryOp(op, operand) => {
                let value = self.operand(operand, body)?;
                match (op, value) {
                    (UnOp::Neg, BasicValueEnum::IntValue(value)) => self
                        .builder
                        .build_int_neg(value, "neg")
                        .map(Into::into)
                        .map_err(llvm_error("integer negation")),
                    (UnOp::Neg, BasicValueEnum::FloatValue(value)) => self
                        .builder
                        .build_float_neg(value, "fneg")
                        .map(Into::into)
                        .map_err(llvm_error("float negation")),
                    (UnOp::Not, BasicValueEnum::IntValue(value))
                        if operand_type(operand, body)?.is_bool() =>
                    {
                        let zero = value.get_type().const_zero();
                        self.builder
                            .build_int_compare(IntPredicate::EQ, value, zero, "not")
                            .map(Into::into)
                            .map_err(llvm_error("logical not"))
                    }
                    (UnOp::Not, BasicValueEnum::IntValue(value)) => self
                        .builder
                        .build_not(value, "bitnot")
                        .map(Into::into)
                        .map_err(llvm_error("bitwise not")),
                    _ => Err(BackendError::Unsupported(format!(
                        "unary operation {op:?} for operand"
                    ))),
                }
            }
            Rvalue::BinaryOp(op, operands) => {
                let lhs = self.operand(&operands.0, body)?;
                let rhs = self.operand(&operands.1, body)?;
                self.binary(*op, lhs, rhs, operand_type(&operands.0, body)?)
            }
            Rvalue::Overflows(op, operands) => {
                let lhs = self.operand(&operands.0, body)?.into_int_value();
                let rhs = self.operand(&operands.1, body)?.into_int_value();
                let ty = operand_type(&operands.0, body)?;
                self.overflow(*op, lhs, rhs, ty).map(Into::into)
            }
            Rvalue::Cast(kind, operand, target) => {
                let value = self.operand(operand, body)?;
                self.cast(*kind, value, target, operand_type(operand, body)?)
            }
            Rvalue::AddressOf(_, place) => {
                let address = self.place_ptr(place, body)?;
                Ok(address.into())
            }
        }
    }

    fn binary(
        &self,
        op: BinOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
        ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        if let (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) = (lhs, rhs) {
            let value = match op {
                BinOp::Add => self
                    .builder
                    .build_float_add(lhs, rhs, "fadd")
                    .map(Into::into),
                BinOp::Sub => self
                    .builder
                    .build_float_sub(lhs, rhs, "fsub")
                    .map(Into::into),
                BinOp::Mul => self
                    .builder
                    .build_float_mul(lhs, rhs, "fmul")
                    .map(Into::into),
                BinOp::Div => self
                    .builder
                    .build_float_div(lhs, rhs, "fdiv")
                    .map(Into::into),
                BinOp::Rem => self
                    .builder
                    .build_float_rem(lhs, rhs, "frem")
                    .map(Into::into),
                BinOp::Eq => self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, lhs, rhs, "feq")
                    .map(Into::into),
                BinOp::Ne => self
                    .builder
                    .build_float_compare(FloatPredicate::ONE, lhs, rhs, "fne")
                    .map(Into::into),
                BinOp::Lt => self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, lhs, rhs, "flt")
                    .map(Into::into),
                BinOp::Le => self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, lhs, rhs, "fle")
                    .map(Into::into),
                BinOp::Gt => self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, lhs, rhs, "fgt")
                    .map(Into::into),
                BinOp::Ge => self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, lhs, rhs, "fge")
                    .map(Into::into),
                _ => return Err(BackendError::Unsupported(format!("float operator {op:?}"))),
            };
            return value.map_err(llvm_error("floating-point operation"));
        }

        if let (BasicValueEnum::PointerValue(lhs), BasicValueEnum::PointerValue(rhs)) = (lhs, rhs) {
            let int_type = self
                .context
                .custom_width_int_type(
                    std::num::NonZero::new(self.target.pointer_width()).ok_or_else(|| {
                        BackendError::Unsupported("zero pointer width".to_owned())
                    })?,
                )
                .map_err(|error| BackendError::Unsupported(error.to_string()))?;
            let lhs = self
                .builder
                .build_ptr_to_int(lhs, int_type, "ptr.lhs")
                .map_err(llvm_error("left pointer comparison cast"))?;
            let rhs = self
                .builder
                .build_ptr_to_int(rhs, int_type, "ptr.rhs")
                .map_err(llvm_error("right pointer comparison cast"))?;
            return match op {
                BinOp::Eq => self
                    .builder
                    .build_int_compare(IntPredicate::EQ, lhs, rhs, "ptr.eq")
                    .map(Into::into)
                    .map_err(llvm_error("pointer equality")),
                BinOp::Ne => self
                    .builder
                    .build_int_compare(IntPredicate::NE, lhs, rhs, "ptr.ne")
                    .map(Into::into)
                    .map_err(llvm_error("pointer inequality")),
                _ => Err(BackendError::Unsupported(format!(
                    "pointer operator {op:?}"
                ))),
            };
        }

        let (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) = (lhs, rhs) else {
            return Err(BackendError::Unsupported(format!(
                "operator {op:?} on non-scalar operands"
            )));
        };
        let unsigned = !ty.is_signed_integer();
        let rhs = if op.is_shift() && lhs.get_type() != rhs.get_type() {
            self.builder
                .build_int_cast(rhs, lhs.get_type(), "shift.amount")
                .map_err(llvm_error("shift amount conversion"))?
        } else {
            rhs
        };
        let value = match op {
            BinOp::Add => self.builder.build_int_add(lhs, rhs, "add").map(Into::into),
            BinOp::Sub => self.builder.build_int_sub(lhs, rhs, "sub").map(Into::into),
            BinOp::Mul => self.builder.build_int_mul(lhs, rhs, "mul").map(Into::into),
            BinOp::Div if unsigned => self
                .builder
                .build_int_unsigned_div(lhs, rhs, "udiv")
                .map(Into::into),
            BinOp::Div => self
                .builder
                .build_int_signed_div(lhs, rhs, "sdiv")
                .map(Into::into),
            BinOp::Rem if unsigned => self
                .builder
                .build_int_unsigned_rem(lhs, rhs, "urem")
                .map(Into::into),
            BinOp::Rem => self
                .builder
                .build_int_signed_rem(lhs, rhs, "srem")
                .map(Into::into),
            BinOp::BitAnd => self.builder.build_and(lhs, rhs, "and").map(Into::into),
            BinOp::BitOr => self.builder.build_or(lhs, rhs, "or").map(Into::into),
            BinOp::BitXor => self.builder.build_xor(lhs, rhs, "xor").map(Into::into),
            BinOp::Shl => self
                .builder
                .build_left_shift(lhs, rhs, "shl")
                .map(Into::into),
            BinOp::Shr => self
                .builder
                .build_right_shift(lhs, rhs, !unsigned, "shr")
                .map(Into::into),
            BinOp::Eq => self
                .builder
                .build_int_compare(IntPredicate::EQ, lhs, rhs, "eq")
                .map(Into::into),
            BinOp::Ne => self
                .builder
                .build_int_compare(IntPredicate::NE, lhs, rhs, "ne")
                .map(Into::into),
            BinOp::Lt => self
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::ULT
                    } else {
                        IntPredicate::SLT
                    },
                    lhs,
                    rhs,
                    "lt",
                )
                .map(Into::into),
            BinOp::Le => self
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::ULE
                    } else {
                        IntPredicate::SLE
                    },
                    lhs,
                    rhs,
                    "le",
                )
                .map(Into::into),
            BinOp::Gt => self
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::UGT
                    } else {
                        IntPredicate::SGT
                    },
                    lhs,
                    rhs,
                    "gt",
                )
                .map(Into::into),
            BinOp::Ge => self
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::UGE
                    } else {
                        IntPredicate::SGE
                    },
                    lhs,
                    rhs,
                    "ge",
                )
                .map(Into::into),
        };
        value.map_err(llvm_error("integer operation"))
    }

    fn overflow(
        &self,
        op: BinOp,
        lhs: inkwell::values::IntValue<'ctx>,
        rhs: inkwell::values::IntValue<'ctx>,
        ty: &Type,
    ) -> Result<inkwell::values::IntValue<'ctx>, BackendError> {
        let width = lhs.get_type().get_bit_width();
        let double_width = width.checked_mul(2).ok_or_else(|| {
            BackendError::Unsupported("integer type width is too large".to_owned())
        })?;
        let double_width = std::num::NonZero::new(double_width)
            .ok_or_else(|| BackendError::Unsupported("zero-width integer".to_owned()))?;
        let extended_type = self
            .context
            .custom_width_int_type(double_width)
            .map_err(|error| BackendError::Unsupported(error.to_string()))?;
        let signed = ty.is_signed_integer();
        let extend = |value| {
            if signed {
                self.builder
                    .build_int_s_extend(value, extended_type, "overflow.extend")
            } else {
                self.builder
                    .build_int_z_extend(value, extended_type, "overflow.extend")
            }
            .map_err(llvm_error("overflow operand extension"))
        };
        let lhs_extended = extend(lhs)?;
        let rhs_extended = extend(rhs)?;
        let full_result = match op {
            BinOp::Add => self
                .builder
                .build_int_add(lhs_extended, rhs_extended, "overflow.add"),
            BinOp::Sub => self
                .builder
                .build_int_sub(lhs_extended, rhs_extended, "overflow.sub"),
            BinOp::Mul => self
                .builder
                .build_int_mul(lhs_extended, rhs_extended, "overflow.mul"),
            _ => {
                return Err(BackendError::InvalidMir(format!(
                    "overflow check for unsupported operation {op:?}"
                )));
            }
        }
        .map_err(llvm_error("extended overflow operation"))?;
        let narrowed = self
            .builder
            .build_int_truncate(full_result, lhs.get_type(), "overflow.narrow")
            .map_err(llvm_error("overflow truncation"))?;
        let restored = extend(narrowed)?;
        self.builder
            .build_int_compare(IntPredicate::NE, full_result, restored, "overflow")
            .map_err(llvm_error("overflow comparison"))
    }

    fn cast(
        &self,
        kind: CastKind,
        value: BasicValueEnum<'ctx>,
        target: &Type,
        source: &Type,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let target_type = self.llvm_type(target)?;
        match kind {
            CastKind::IntToInt => {
                let value = value.into_int_value();
                let target_type = target_type.into_int_type();
                if value.get_type().get_bit_width() == target_type.get_bit_width() {
                    return Ok(value.into());
                } else if value.get_type().get_bit_width() > target_type.get_bit_width() {
                    self.builder.build_int_truncate(value, target_type, "trunc")
                } else if source.is_signed_integer() {
                    self.builder.build_int_s_extend(value, target_type, "sext")
                } else {
                    self.builder.build_int_z_extend(value, target_type, "zext")
                }
                .map(Into::into)
                .map_err(llvm_error("integer cast"))
            }
            CastKind::IntToFloat => {
                let value = value.into_int_value();
                let cast = if source.is_signed_integer() {
                    self.builder.build_signed_int_to_float(
                        value,
                        target_type.into_float_type(),
                        "sitofp",
                    )
                } else {
                    self.builder.build_unsigned_int_to_float(
                        value,
                        target_type.into_float_type(),
                        "uitofp",
                    )
                };
                cast.map(Into::into)
                    .map_err(llvm_error("integer to float cast"))
            }
            CastKind::FloatToInt => {
                let value = value.into_float_value();
                let cast = if target.is_signed_integer() {
                    self.builder.build_float_to_signed_int(
                        value,
                        target_type.into_int_type(),
                        "fptosi",
                    )
                } else {
                    self.builder.build_float_to_unsigned_int(
                        value,
                        target_type.into_int_type(),
                        "fptoui",
                    )
                };
                cast.map(Into::into)
                    .map_err(llvm_error("float to integer cast"))
            }
            CastKind::FloatToFloat => {
                let value = value.into_float_value();
                if value.get_type() == target_type.into_float_type() {
                    Ok(value.into())
                } else {
                    self.builder
                        .build_float_cast(value, target_type.into_float_type(), "fpcast")
                        .map(Into::into)
                        .map_err(llvm_error("float cast"))
                }
            }
            CastKind::PtrToPtr => Ok(value),
        }
    }

    fn operand(
        &mut self,
        operand: &Operand,
        body: &Body,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        match operand {
            Operand::Copy(place) => {
                let ty = crate::middle::mir::typing::place_ty(place, body)
                    .map_err(|error| BackendError::InvalidMir(error.to_string()))?;
                self.builder
                    .build_load(self.llvm_type(ty)?, self.place_ptr(place, body)?, "load")
                    .map_err(llvm_error("load operand"))
            }
            Operand::Constant(constant) => self.constant(constant),
        }
    }

    fn constant(&self, constant: &Constant) -> Result<BasicValueEnum<'ctx>, BackendError> {
        let ty = self.llvm_type(&constant.ty)?;
        match &constant.value {
            ConstValue::Bool(value) => Ok(self
                .context
                .bool_type()
                .const_int(u64::from(*value), false)
                .into()),
            ConstValue::Int(value) => {
                let int_type = ty.into_int_type();
                Ok(bigint_constant(int_type, value).into())
            }
            ConstValue::Float(value) => Ok(ty.into_float_type().const_float(*value).into()),
            ConstValue::Address(value) => {
                if constant.ty.is_indirect() {
                    let pointer_int = self
                        .context
                        .custom_width_int_type(
                            std::num::NonZero::new(self.target.pointer_width()).ok_or_else(
                                || BackendError::Unsupported("zero pointer width".to_owned()),
                            )?,
                        )
                        .map_err(|error| BackendError::Unsupported(error.to_string()))?;
                    self.builder
                        .build_int_to_ptr(
                            bigint_constant(pointer_int, value),
                            self.context.ptr_type(AddressSpace::default()),
                            "address",
                        )
                        .map(Into::into)
                        .map_err(llvm_error("address constant conversion"))
                } else {
                    Ok(bigint_constant(ty.into_int_type(), value).into())
                }
            }
        }
    }

    fn place_ptr(&self, place: &Place, body: &Body) -> Result<PointerValue<'ctx>, BackendError> {
        let mut pointer = *self.local_slots.get(&place.local).ok_or_else(|| {
            BackendError::InvalidMir(format!("missing stack slot for {:?}", place.local))
        })?;
        let mut ty = &body.local_decls()[place.local].ty;
        for projection in &place.projection {
            match projection {
                ProjectionElem::Deref => {
                    let value = self
                        .builder
                        .build_load(self.llvm_type(ty)?, pointer, "deref")
                        .map_err(llvm_error("load pointer"))?;
                    pointer = value.into_pointer_value();
                    ty = ty.pointee().ok_or_else(|| {
                        BackendError::InvalidMir("dereference of non-pointer place".to_owned())
                    })?;
                }
            }
        }
        Ok(pointer)
    }

    fn llvm_type(&self, ty: &Type) -> Result<BasicTypeEnum<'ctx>, BackendError> {
        Ok(match ty {
            Type::Bool => self.context.bool_type().into(),
            Type::Int(width) | Type::UInt(width) => {
                self.context
                    .custom_width_int_type(std::num::NonZero::new(*width).ok_or_else(|| {
                        BackendError::Unsupported("zero-width integer".to_owned())
                    })?)
                    .map_err(|error| BackendError::Unsupported(error.to_string()))?
                    .into()
            }
            Type::USize | Type::ISize => self
                .context
                .custom_width_int_type(
                    std::num::NonZero::new(self.target.pointer_width()).ok_or_else(|| {
                        BackendError::Unsupported("zero pointer width".to_owned())
                    })?,
                )
                .map_err(|error| BackendError::Unsupported(error.to_string()))?
                .into(),
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

fn bigint_constant<'ctx>(
    int_type: inkwell::types::IntType<'ctx>,
    value: &num_bigint::BigInt,
) -> inkwell::values::IntValue<'ctx> {
    let width = int_type.get_bit_width() as usize;
    let bytes = value.to_signed_bytes_le();
    let mut words = vec![
        if value.sign() == num_bigint::Sign::Minus {
            u64::MAX
        } else {
            0
        };
        width.div_ceil(64)
    ];
    for (index, byte) in bytes.iter().enumerate() {
        let word = index / 8;
        let shift = (index % 8) * 8;
        if let Some(bits) = words.get_mut(word) {
            *bits = (*bits & !(0xff_u64 << shift)) | (u64::from(*byte) << shift);
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
    crate::middle::mir::typing::operand_ty(operand, body)
        .map_err(|error| BackendError::InvalidMir(error.to_string()))
}

fn llvm_error(
    operation: &'static str,
) -> impl FnOnce(inkwell::builder::BuilderError) -> BackendError {
    move |error| BackendError::InvalidMir(format!("{operation}: {error:?}"))
}
