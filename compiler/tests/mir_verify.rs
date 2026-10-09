//! Tests for the MIR verifier: one valid body, then one test per invariant.

use num_bigint::BigInt;
use xenonc::index::Idx;
use xenonc::middle::ids::DefId;
use xenonc::middle::mir::typing::TypingError;
use xenonc::middle::mir::*;
use xenonc::middle::target::TargetSpec;
use xenonc::source::Span;
use xenonc::types::Type;

const SI: SourceInfo = SourceInfo::outermost(Span::ZERO);

fn int(value: i64, ty: Type) -> Operand {
    Operand::constant(Constant::int(value, ty))
}

fn copy(place: impl Into<Place>) -> Operand {
    Operand::Copy(place.into())
}

fn ptr(ty: Type) -> Type {
    Type::Pointer(Box::new(ty))
}

/// Declares `callee(i32) -> bool` (as `fn0`) and a function `f` with the
/// given signature, builds `f`'s body with `build`, and returns the kinds of
/// verification errors found on `target`.
fn verify_with(
    target: TargetSpec,
    inputs: Vec<Type>,
    output: Type,
    build: impl FnOnce(&mut BodyBuilder),
) -> Vec<VerifyErrorKind> {
    let mut program = MirProgram::new();
    program.declare(FnDecl {
        name: "callee".to_owned(),
        sig: FnSig {
            inputs: vec![Type::Int(32)],
            output: Type::Bool,
        },
        span: Span::ZERO,
    });
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: inputs.clone(),
            output: output.clone(),
        },
        span: Span::ZERO,
    });
    let args = inputs.into_iter().map(|ty| LocalDecl::temp(ty, Span::ZERO));
    let mut builder = BodyBuilder::new(def_id, Span::ZERO, output, args);
    build(&mut builder);
    let body = match builder.finish() {
        Ok(body) => body,
        Err(error) => panic!("test body is malformed: {error}"),
    };
    if let Err(error) = program.set_body(body) {
        panic!("test body could not be attached: {error}");
    }
    match verify_program(&program, &target) {
        Ok(()) => Vec::new(),
        Err(VerifyErrors(errors)) => errors.into_iter().map(|error| error.kind).collect(),
    }
}

/// Builds `fn f() -> bool` whose start block runs `build` then returns.
fn verify_stmts(build: impl FnOnce(&mut BodyBuilder)) -> Vec<VerifyErrorKind> {
    verify_with(TargetSpec::new(64), vec![], Type::Bool, |b| {
        build(b);
        if !b.is_terminated(START_BLOCK) {
            b.terminate(START_BLOCK, SI, TerminatorKind::Return);
        }
    })
}

fn assign(b: &mut BodyBuilder, place: impl Into<Place>, rvalue: Rvalue) {
    b.push_assign(START_BLOCK, SI, place.into(), rvalue);
}

#[test]
fn well_formed_body_has_no_errors() {
    let errors = verify_with(
        TargetSpec::new(64),
        vec![Type::Int(32), ptr(Type::USize)],
        Type::Bool,
        |b| {
            let (x, p) = (Local::new(1), Local::new(2));
            let n = b.new_temp(Type::USize, Span::ZERO);
            let ok = b.new_temp(Type::Bool, Span::ZERO);
            let next = b.new_block();
            b.storage_live(START_BLOCK, SI, n);
            assign(b, n, Rvalue::Cast(CastKind::IntToInt, copy(x), Type::USize));
            assign(b, Place::from(p).deref(), Rvalue::Use(copy(n)));
            let shifted = b.new_temp(Type::Int(32), Span::ZERO);
            assign(
                b,
                shifted,
                Rvalue::BinaryOp(
                    BinOp::Shl,
                    Box::new((int(1, Type::Int(32)), int(3, Type::UInt(8)))),
                ),
            );
            assign(
                b,
                ok,
                Rvalue::BinaryOp(BinOp::Lt, Box::new((copy(x), int(-5, Type::Int(32))))),
            );
            b.terminate(
                START_BLOCK,
                SI,
                TerminatorKind::Call {
                    func: DefId::new(0),
                    args: vec![copy(x)],
                    destination: RETURN_PLACE.into(),
                    target: Some(next),
                },
            );
            b.storage_dead(next, SI, n);
            b.terminate(
                next,
                SI,
                TerminatorKind::SwitchInt {
                    discr: copy(x),
                    targets: SwitchTargets::new(
                        [(BigInt::from(-1), next), (BigInt::from(7), next)],
                        next,
                    ),
                },
            );
        },
    );
    assert_eq!(errors, vec![]);
}

#[test]
fn signature_mismatch() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![Type::Int(8)],
            output: Type::Bool,
        },
        span: Span::ZERO,
    });
    let args = [Type::Int(16), Type::Bool].map(|ty| LocalDecl::temp(ty, Span::ZERO));
    let mut b = BodyBuilder::new(def_id, Span::ZERO, Type::Int(32), args);
    b.terminate(START_BLOCK, SI, TerminatorKind::Return);
    let set = b
        .finish()
        .map_err(|e| e.to_string())
        .and_then(|body| program.set_body(body).map_err(|e| e.to_string()));
    assert_eq!(set, Ok(()));

    let errors = match verify_program(&program, &TargetSpec::host()) {
        Err(VerifyErrors(errors)) => errors,
        Ok(()) => panic!("expected verification errors"),
    };
    let kinds: Vec<_> = errors.iter().map(|e| e.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            VerifyErrorKind::ReturnTypeMismatch {
                expected: Type::Bool,
                found: Type::Int(32),
            },
            VerifyErrorKind::ArgCountMismatch {
                expected: 1,
                found: 2,
            },
            VerifyErrorKind::ArgTypeMismatch {
                local: Local::new(1),
                expected: Type::Int(8),
                found: Type::Int(16),
            },
        ]
    );
    assert_eq!(errors[0].function, "f");
    assert_eq!(errors[0].site, verify::ErrorSite::Body);
}

#[test]
fn assign_type_mismatch() {
    let errors = verify_stmts(|b| {
        assign(b, RETURN_PLACE, Rvalue::Use(int(1, Type::Int(32))));
    });
    assert_eq!(
        errors,
        vec![VerifyErrorKind::AssignTypeMismatch {
            place: Type::Bool,
            rvalue: Type::Int(32),
        }]
    );
}

#[test]
fn unknown_local_is_reported_once() {
    let errors = verify_stmts(|b| {
        assign(b, RETURN_PLACE, Rvalue::Use(copy(Local::new(9))));
    });
    assert_eq!(errors, vec![VerifyErrorKind::UnknownLocal(Local::new(9))]);
}

#[test]
fn deref_of_non_pointer() {
    let errors = verify_stmts(|b| {
        let place = Place::from(RETURN_PLACE).deref();
        assign(b, place, Rvalue::Use(int(1, Type::Int(32))));
    });
    assert_eq!(
        errors,
        vec![VerifyErrorKind::Typing(TypingError::DerefOfNonPointer(
            Type::Bool
        ))]
    );
}

#[test]
fn ill_typed_rvalues() {
    let errors = verify_stmts(|b| {
        let t = b.new_temp(Type::Int(32), Span::ZERO);
        let operands = Box::new((int(1, Type::Int(32)), int(1, Type::Int(64))));
        assign(b, t, Rvalue::BinaryOp(BinOp::Add, operands));
        let operands = Box::new((copy(t), copy(t)));
        assign(b, RETURN_PLACE, Rvalue::Overflows(BinOp::Div, operands));
        assign(b, t, Rvalue::UnaryOp(UnOp::Neg, copy(RETURN_PLACE)));
        assign(
            b,
            t,
            Rvalue::Cast(CastKind::FloatToInt, copy(t), Type::Int(32)),
        );
    });
    assert_eq!(
        errors,
        vec![
            VerifyErrorKind::Typing(TypingError::OperandMismatch {
                op: BinOp::Add,
                lhs: Type::Int(32),
                rhs: Type::Int(64),
            }),
            VerifyErrorKind::Typing(TypingError::UncheckableOverflowOp(BinOp::Div)),
            VerifyErrorKind::Typing(TypingError::InvalidUnaryOperand {
                op: UnOp::Neg,
                ty: Type::Bool,
            }),
            VerifyErrorKind::Typing(TypingError::InvalidCast {
                kind: CastKind::FloatToInt,
                from: Type::Int(32),
                to: Type::Int(32),
            }),
        ]
    );
}

#[test]
fn storage_markers_on_return_place_and_arguments() {
    let errors = verify_with(TargetSpec::new(64), vec![Type::Bool], Type::Bool, |b| {
        b.storage_live(START_BLOCK, SI, RETURN_PLACE);
        b.storage_dead(START_BLOCK, SI, Local::new(1));
        b.terminate(START_BLOCK, SI, TerminatorKind::Return);
    });
    assert_eq!(
        errors,
        vec![
            VerifyErrorKind::StorageMarkerOnFixedLocal(RETURN_PLACE),
            VerifyErrorKind::StorageMarkerOnFixedLocal(Local::new(1)),
        ]
    );
}

#[test]
fn jump_to_unknown_block() {
    let errors = verify_stmts(|b| b.goto(START_BLOCK, SI, BasicBlock::new(7)));
    assert_eq!(
        errors,
        vec![VerifyErrorKind::UnknownBlock(BasicBlock::new(7))]
    );
}

#[test]
fn invalid_switches() {
    let switch = |discr: Operand, values: Vec<i64>| {
        verify_stmts(move |b| {
            let targets = values.into_iter().map(|v| (BigInt::from(v), START_BLOCK));
            b.terminate(
                START_BLOCK,
                SI,
                TerminatorKind::SwitchInt {
                    discr,
                    targets: SwitchTargets::new(targets, START_BLOCK),
                },
            );
        })
    };

    let float = Operand::constant(Constant::float(1.0, Type::Float32));
    assert_eq!(
        switch(float, vec![]),
        vec![VerifyErrorKind::InvalidSwitchDiscr(Type::Float32)]
    );
    assert_eq!(
        switch(int(0, Type::UInt(8)), vec![256, 3, 3]),
        vec![
            VerifyErrorKind::SwitchValueOutOfRange {
                value: BigInt::from(256),
                ty: Type::UInt(8),
            },
            VerifyErrorKind::DuplicateSwitchValue(BigInt::from(3)),
        ]
    );
    assert_eq!(
        switch(Operand::constant(Constant::bool(true)), vec![2]),
        vec![VerifyErrorKind::SwitchValueOutOfRange {
            value: BigInt::from(2),
            ty: Type::Bool,
        }]
    );
}

#[test]
fn assert_condition_must_be_bool() {
    let errors = verify_stmts(|b| {
        let next = b.new_block();
        b.terminate(
            START_BLOCK,
            SI,
            TerminatorKind::Assert {
                cond: int(1, Type::Int(32)),
                expected: true,
                kind: AssertKind::DivisionByZero,
                target: next,
            },
        );
        b.terminate(next, SI, TerminatorKind::Return);
    });
    assert_eq!(
        errors,
        vec![VerifyErrorKind::NonBoolAssertCond(Type::Int(32))]
    );
}

#[test]
fn calls_must_match_callee_signature() {
    let call = |func: DefId, args: Vec<Operand>, destination: Place| {
        verify_stmts(move |b| {
            b.terminate(
                START_BLOCK,
                SI,
                TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    target: None,
                },
            );
        })
    };
    let callee = DefId::new(0);

    assert_eq!(
        call(DefId::new(9), vec![], RETURN_PLACE.into()),
        vec![VerifyErrorKind::UnknownCallee(DefId::new(9))]
    );
    assert_eq!(
        call(callee, vec![], RETURN_PLACE.into()),
        vec![VerifyErrorKind::CallArgCountMismatch {
            callee: "callee".to_owned(),
            expected: 1,
            found: 0,
        }]
    );
    assert_eq!(
        call(callee, vec![int(1, Type::Int(8))], RETURN_PLACE.into()),
        vec![VerifyErrorKind::CallArgTypeMismatch {
            callee: "callee".to_owned(),
            index: 0,
            expected: Type::Int(32),
            found: Type::Int(8),
        }]
    );

    let errors = verify_stmts(|b| {
        let dest = b.new_temp(Type::Int(32), Span::ZERO);
        b.terminate(
            START_BLOCK,
            SI,
            TerminatorKind::Call {
                func: callee,
                args: vec![int(1, Type::Int(32))],
                destination: dest.into(),
                target: None,
            },
        );
    });
    assert_eq!(
        errors,
        vec![VerifyErrorKind::CallDestinationMismatch {
            callee: "callee".to_owned(),
            expected: Type::Bool,
            found: Type::Int(32),
        }]
    );
}

#[test]
fn constants_must_fit_their_type() {
    let check = |target: TargetSpec, constant: Constant| {
        verify_with(target, vec![], Type::Bool, move |b| {
            let t = b.new_temp(constant.ty.clone(), Span::ZERO);
            b.push_assign(
                START_BLOCK,
                SI,
                t.into(),
                Rvalue::Use(Operand::constant(constant)),
            );
            b.terminate(START_BLOCK, SI, TerminatorKind::Return);
        })
    };
    let wide = TargetSpec::new(64);
    let narrow = TargetSpec::new(16);

    assert_eq!(
        check(wide, Constant::int(256, Type::UInt(8))),
        vec![VerifyErrorKind::ConstantOutOfRange {
            value: BigInt::from(256),
            ty: Type::UInt(8),
        }]
    );
    assert_eq!(
        check(wide, Constant::int(-129, Type::Int(8))),
        vec![VerifyErrorKind::ConstantOutOfRange {
            value: BigInt::from(-129),
            ty: Type::Int(8),
        }]
    );
    // `usize` and addresses depend on the target pointer width.
    assert_eq!(check(wide, Constant::int(70_000, Type::USize)), vec![]);
    assert_eq!(
        check(narrow, Constant::int(70_000, Type::USize)),
        vec![VerifyErrorKind::ConstantOutOfRange {
            value: BigInt::from(70_000),
            ty: Type::USize,
        }]
    );
    let ptr_ty = ptr(Type::UInt(8));
    assert_eq!(
        check(wide, Constant::address(0x1_0000, ptr_ty.clone())),
        vec![]
    );
    assert_eq!(
        check(narrow, Constant::address(0x1_0000, ptr_ty.clone())),
        vec![VerifyErrorKind::ConstantOutOfRange {
            value: BigInt::from(0x1_0000),
            ty: ptr_ty,
        }]
    );
    assert_eq!(
        check(wide, Constant::int(1, Type::Float32)),
        vec![VerifyErrorKind::ConstantKindMismatch(Type::Float32)]
    );
    assert_eq!(
        check(
            wide,
            Constant {
                ty: Type::Int(8),
                value: ConstValue::Bool(true),
            }
        ),
        vec![VerifyErrorKind::ConstantKindMismatch(Type::Int(8))]
    );
}

#[test]
fn end_of_body_is_only_valid_while_built() {
    let mut program = MirProgram::new();
    let def_id = program.declare(FnDecl {
        name: "f".to_owned(),
        sig: FnSig {
            inputs: vec![],
            output: Type::Bool,
        },
        span: Span::ZERO,
    });
    let mut b = BodyBuilder::new(def_id, Span::ZERO, Type::Bool, []);
    b.terminate(START_BLOCK, SI, TerminatorKind::EndOfBody);
    let attached = b
        .finish()
        .map_err(|e| e.to_string())
        .and_then(|body| program.set_body(body).map_err(|e| e.to_string()));
    assert_eq!(attached, Ok(()));
    let target = TargetSpec::host();
    assert_eq!(verify_program(&program, &target), Ok(()));

    let body = program.body_mut(def_id);
    let advanced = body.map(|body| body.advance_phase(MirPhase::Checked));
    assert_eq!(advanced, Some(Ok(())));
    let errors = match verify_program(&program, &target) {
        Err(VerifyErrors(errors)) => errors,
        Ok(()) => panic!("expected verification errors"),
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].kind,
        VerifyErrorKind::InvalidInPhase {
            terminator: "end_of_body",
            phase: MirPhase::Checked,
        }
    );
    assert_eq!(
        errors[0].to_string(),
        "invalid MIR in `f` at bb0[0]: `end_of_body` is not allowed in phase `checked`"
    );
}

#[test]
fn invalid_scopes() {
    let errors = verify_stmts(|b| {
        let scope = b.new_scope(OUTERMOST_SOURCE_SCOPE, Span::ZERO);
        let bad = SourceInfo {
            span: Span::ZERO,
            scope: SourceScope::from_u32(scope.as_u32() + 5),
        };
        b.push_assign(
            START_BLOCK,
            bad,
            RETURN_PLACE.into(),
            Rvalue::Use(copy(RETURN_PLACE)),
        );
    });
    assert_eq!(
        errors,
        vec![VerifyErrorKind::UnknownScope(SourceScope::new(6))]
    );
}
