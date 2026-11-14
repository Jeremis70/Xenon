//! Tests for MIR constant evaluation and the known-panics pass.

use num_bigint::BigInt;
use xenonc::frontend::lexer::lex;
use xenonc::frontend::parser::Parser;
use xenonc::index::{Idx, IndexVec};
use xenonc::middle::mir::analysis::{analyze_program, check_known_panics};
use xenonc::middle::mir::{
    AssertKind, BinOp, CastKind, Constant, EvalResult, Local, LocalDecl, Operand, Rvalue, UnOp,
    build_mir, eval_rvalue,
};
use xenonc::middle::target::TargetSpec;
use xenonc::middle::typecheck::check_program;
use xenonc::source::Span;
use xenonc::types::Type;

const TARGET: TargetSpec = TargetSpec::new(64);

// ---------------------------------------------------------------------------
// Constant evaluation
// ---------------------------------------------------------------------------

fn int(value: i64, ty: Type) -> Operand {
    Operand::constant(Constant::int(value, ty))
}

fn eval(rvalue: Rvalue) -> EvalResult {
    let decls: IndexVec<Local, LocalDecl> = IndexVec::new();
    let value_of = |operand: &Operand| match operand {
        Operand::Constant(constant) => Some((**constant).clone()),
        Operand::Copy(_) => None,
    };
    eval_rvalue(&rvalue, &decls, &TARGET, value_of).expect("rvalue should type")
}

fn value(value: i64, ty: Type) -> EvalResult {
    EvalResult::Value(Constant::int(value, ty))
}

fn binary(op: BinOp, lhs: i64, rhs: i64, ty: Type) -> EvalResult {
    eval(Rvalue::binary(op, int(lhs, ty.clone()), int(rhs, ty)))
}

#[test]
fn arithmetic_is_checked_against_the_type() {
    assert_eq!(
        binary(BinOp::Add, 200, 55, Type::UInt(8)),
        value(255, Type::UInt(8))
    );
    assert_eq!(
        binary(BinOp::Add, 200, 56, Type::UInt(8)),
        EvalResult::Trap(AssertKind::Overflow(BinOp::Add))
    );
    assert_eq!(
        binary(BinOp::Sub, 0, 1, Type::UInt(32)),
        EvalResult::Trap(AssertKind::Overflow(BinOp::Sub))
    );
    assert_eq!(
        binary(BinOp::Mul, 64, 2, Type::Int(8)),
        EvalResult::Trap(AssertKind::Overflow(BinOp::Mul))
    );
    assert_eq!(
        binary(BinOp::Mul, -64, 2, Type::Int(8)),
        value(-128, Type::Int(8))
    );
}

#[test]
fn division_truncates_and_traps() {
    assert_eq!(
        binary(BinOp::Div, -7, 2, Type::Int(32)),
        value(-3, Type::Int(32))
    );
    assert_eq!(
        binary(BinOp::Rem, -7, 2, Type::Int(32)),
        value(-1, Type::Int(32))
    );
    assert_eq!(
        binary(BinOp::Div, 1, 0, Type::Int(32)),
        EvalResult::Trap(AssertKind::DivisionByZero)
    );
    assert_eq!(
        binary(BinOp::Rem, 1, 0, Type::UInt(32)),
        EvalResult::Trap(AssertKind::RemainderByZero)
    );
    assert_eq!(
        binary(BinOp::Div, -128, -1, Type::Int(8)),
        EvalResult::Trap(AssertKind::SignedDivisionOverflow)
    );
    assert_eq!(
        binary(BinOp::Rem, -128, -1, Type::Int(8)),
        EvalResult::Trap(AssertKind::SignedRemainderOverflow)
    );
}

/// A known zero divisor traps whatever the dividend holds.
#[test]
fn zero_divisor_traps_with_unknown_dividend() {
    let unknown = Operand::Copy(Local::new(0).into());
    let decls: IndexVec<Local, LocalDecl> =
        IndexVec::from_raw(vec![LocalDecl::temp(Type::Int(32), Span::ZERO)]);
    let rvalue = Rvalue::binary(BinOp::Div, unknown, int(0, Type::Int(32)));
    let value_of = |operand: &Operand| match operand {
        Operand::Constant(constant) => Some((**constant).clone()),
        Operand::Copy(_) => None,
    };
    assert_eq!(
        eval_rvalue(&rvalue, &decls, &TARGET, value_of).expect("rvalue should type"),
        EvalResult::Trap(AssertKind::DivisionByZero)
    );
}

#[test]
fn shifts_follow_the_bit_width() {
    assert_eq!(
        binary(BinOp::Shl, 1, 7, Type::UInt(8)),
        value(128, Type::UInt(8))
    );
    assert_eq!(
        binary(BinOp::Shl, 3, 7, Type::UInt(8)),
        value(128, Type::UInt(8))
    );
    assert_eq!(
        binary(BinOp::Shl, 1, 7, Type::Int(8)),
        value(-128, Type::Int(8))
    );
    assert_eq!(
        binary(BinOp::Shr, -128, 7, Type::Int(8)),
        value(-1, Type::Int(8))
    );
    assert_eq!(
        binary(BinOp::Shr, 128, 7, Type::UInt(8)),
        value(1, Type::UInt(8))
    );
    for (lhs, amount, ty) in [
        (1, 8, Type::UInt(8)),
        (1, -1, Type::Int(8)),
        (1, 64, Type::Int(64)),
        (0, 0, Type::UInt(0)),
        (0, 0, Type::Int(0)),
    ] {
        assert_eq!(
            binary(BinOp::Shl, lhs, amount, ty),
            EvalResult::Trap(AssertKind::ShiftOutOfRange)
        );
    }
}

#[test]
fn unary_operations_follow_the_bit_width() {
    let not = |operand: i64, ty: Type| eval(Rvalue::UnaryOp(UnOp::Not, int(operand, ty)));
    assert_eq!(not(0, Type::UInt(8)), value(255, Type::UInt(8)));
    assert_eq!(not(0, Type::Int(8)), value(-1, Type::Int(8)));
    assert_eq!(not(0, Type::UInt(0)), value(0, Type::UInt(0)));
    assert_eq!(not(0, Type::Int(0)), value(0, Type::Int(0)));
    assert_eq!(
        eval(Rvalue::UnaryOp(UnOp::Neg, int(-128, Type::Int(8)))),
        EvalResult::Trap(AssertKind::Overflow(BinOp::Sub))
    );
    assert_eq!(
        eval(Rvalue::UnaryOp(UnOp::Neg, int(0, Type::Int(0)))),
        value(0, Type::Int(0))
    );
}

#[test]
fn comparisons_and_overflow_checks() {
    assert_eq!(
        binary(BinOp::Lt, -1, 0, Type::Int(8)),
        EvalResult::Value(Constant::bool(true))
    );
    assert_eq!(
        eval(Rvalue::overflows(
            BinOp::Add,
            int(255, Type::UInt(8)),
            int(1, Type::UInt(8))
        )),
        EvalResult::Value(Constant::bool(true))
    );
}

/// Casts are exact or trap: they never wrap.
#[test]
fn integer_casts_are_exact_or_trap() {
    let cast = |operand: i64, from: Type, to: Type| {
        eval(Rvalue::Cast(CastKind::IntToInt, int(operand, from), to))
    };
    let lossy = EvalResult::Trap(AssertKind::LossyCast);
    assert_eq!(cast(-1, Type::Int(32), Type::UInt(8)), lossy);
    assert_eq!(cast(255, Type::UInt(8), Type::Int(8)), lossy);
    assert_eq!(cast(300, Type::Int(32), Type::UInt(8)), lossy);
    assert_eq!(
        cast(255, Type::Int(32), Type::UInt(8)),
        value(255, Type::UInt(8))
    );
    assert_eq!(
        cast(-128, Type::Int(32), Type::Int(8)),
        value(-128, Type::Int(8))
    );
    assert_eq!(
        cast(0, Type::Int(32), Type::UInt(0)),
        value(0, Type::UInt(0))
    );
    assert_eq!(cast(1, Type::Int(32), Type::UInt(0)), lossy);
    assert_eq!(
        eval(Rvalue::CastLoses(
            CastKind::IntToInt,
            int(256, Type::Int(32)),
            Type::UInt(8)
        )),
        EvalResult::Value(Constant::bool(true))
    );
    assert_eq!(
        eval(Rvalue::CastLoses(
            CastKind::IntToInt,
            Operand::constant(Constant::int(BigInt::from(255), Type::UInt(8))),
            Type::Int(16)
        )),
        EvalResult::Value(Constant::bool(false))
    );
}

#[test]
fn float_casts_are_exact_or_trap() {
    let float = |value, ty| Operand::constant(Constant::float(value, ty));
    let lossy = EvalResult::Trap(AssertKind::LossyCast);
    let to_int = |value: f64| {
        eval(Rvalue::Cast(
            CastKind::FloatToInt,
            float(value, Type::Float64),
            Type::Int(32),
        ))
    };
    assert_eq!(to_int(2.0), value(2, Type::Int(32)));
    assert_eq!(to_int(-2147483648.0), value(-2147483648, Type::Int(32)));
    assert_eq!(to_int(2.5), lossy);
    assert_eq!(to_int(2147483648.0), lossy);
    assert_eq!(to_int(f64::NAN), lossy);
    assert_eq!(to_int(f64::INFINITY), lossy);

    let to_float = |operand: i64, to: Type| {
        eval(Rvalue::Cast(
            CastKind::IntToFloat,
            int(operand, Type::Int(64)),
            to,
        ))
    };
    // 2^24 + 1 needs 25 significand bits; `f32` has 24.
    assert_eq!(to_float(16_777_217, Type::Float32), lossy);
    assert_eq!(
        to_float(16_777_216, Type::Float32),
        EvalResult::Value(Constant::float(16_777_216.0, Type::Float32))
    );
    // 65504 is the largest finite `f16`; 65536 overflows it.
    assert!(matches!(
        to_float(65504, Type::Float16),
        EvalResult::Value(_)
    ));
    assert_eq!(to_float(65536, Type::Float16), lossy);

    let narrow = |value: f64| {
        eval(Rvalue::Cast(
            CastKind::FloatToFloat,
            float(value, Type::Float64),
            Type::Float32,
        ))
    };
    assert_eq!(narrow(0.1), lossy);
    assert_eq!(narrow(1e300), lossy);
    assert_eq!(
        narrow(0.5),
        EvalResult::Value(Constant::float(0.5, Type::Float32))
    );
    assert!(matches!(narrow(f64::NAN), EvalResult::Value(_)));
}

/// `f16` and `bf16` constants are rounded to their format before a cast.
#[test]
fn half_float_casts_round_to_their_format() {
    let lossy = EvalResult::Trap(AssertKind::LossyCast);
    let cast = |kind, value: f64, from: Type, to: Type| {
        eval(Rvalue::Cast(
            kind,
            Operand::constant(Constant::float(value, from)),
            to,
        ))
    };
    let narrow = |value, to| cast(CastKind::FloatToFloat, value, Type::Float64, to);
    // 0.1 rounds in both; 0.5, 2048, and 65504 (the largest `f16`) do not.
    assert_eq!(narrow(0.1, Type::Float16), lossy);
    assert_eq!(narrow(0.1, Type::BFloat16), lossy);
    for value in [0.5, 2048.0, 65504.0, -65504.0] {
        assert!(
            matches!(narrow(value, Type::Float16), EvalResult::Value(_)),
            "{value}"
        );
    }
    // 2049 needs 12 significand bits; 65520 rounds up to infinity.
    assert_eq!(narrow(2049.0, Type::Float16), lossy);
    assert_eq!(narrow(65520.0, Type::Float16), lossy);
    // 2^-24 is the smallest `f16` subnormal; 2^-25 rounds to zero.
    assert!(matches!(
        narrow(2f64.powi(-24), Type::Float16),
        EvalResult::Value(_)
    ));
    assert_eq!(narrow(2f64.powi(-25), Type::Float16), lossy);
    // bf16 keeps f32's range with 8 significand bits.
    assert!(matches!(
        narrow(2f64.powi(127), Type::BFloat16),
        EvalResult::Value(_)
    ));
    assert_eq!(narrow(2f64.powi(128), Type::BFloat16), lossy);
    assert_eq!(narrow(257.0, Type::BFloat16), lossy);
    assert!(matches!(
        narrow(256.0, Type::BFloat16),
        EvalResult::Value(_)
    ));

    // A typed `f16` literal holds its rounded value: 2.5 is exact, so
    // casting it to an integer traps, while 2049.0 rounds to 2048.
    let to_int = |value, from| cast(CastKind::FloatToInt, value, from, Type::Int(32));
    assert_eq!(to_int(2.5, Type::Float16), lossy);
    assert_eq!(to_int(2049.0, Type::Float16), value(2048, Type::Int(32)));
    assert_eq!(to_int(-3.0, Type::BFloat16), value(-3, Type::Int(32)));
}

#[test]
fn floats_are_not_evaluated() {
    let float = |value| Operand::constant(Constant::float(value, Type::Float64));
    assert_eq!(
        eval(Rvalue::binary(BinOp::Div, float(1.0), float(0.0))),
        EvalResult::Unknown
    );
}

// ---------------------------------------------------------------------------
// Known panics
// ---------------------------------------------------------------------------

/// The assertions that the operations of `src` are known to fail.
fn known_panics(src: &str) -> Vec<AssertKind> {
    let tokens = lex(src).expect("lexing should succeed");
    let ast = Parser::new(&tokens)
        .parse_program()
        .expect("parsing should succeed");
    let thir = check_program(&ast, &TARGET).expect("type checking should succeed");
    let mut mir = build_mir(&thir).expect("MIR construction should succeed");
    analyze_program(&mut mir, &TARGET).expect("flow analysis should succeed");
    check_known_panics(&mir, &TARGET)
        .expect("the pass should accept Checked MIR")
        .into_iter()
        .map(|panic| panic.kind)
        .collect()
}

#[test]
fn reports_operations_that_always_trap() {
    for (src, kind) in [
        ("fn f()->i32 { return 1 / 0; }", AssertKind::DivisionByZero),
        ("fn f()->i32 { return 1 % 0; }", AssertKind::RemainderByZero),
        (
            "fn f()->i32 { return 1 / (1 - 1); }",
            AssertKind::DivisionByZero,
        ),
        (
            "fn f(i32 a)->i32 { return a / 0; }",
            AssertKind::DivisionByZero,
        ),
        (
            "fn f()->u8 { let u8 x = 255; return x + 1; }",
            AssertKind::Overflow(BinOp::Add),
        ),
        (
            "fn f()->u8 { let u8 x = 200 + 100; return x; }",
            AssertKind::Overflow(BinOp::Add),
        ),
        (
            "fn f()->i8 { let i8 x = -128; return -x; }",
            AssertKind::Overflow(BinOp::Sub),
        ),
        (
            "fn f()->i8 { let i8 x = -128; return x / -1; }",
            AssertKind::SignedDivisionOverflow,
        ),
        (
            "fn f()->i64 { return 1 << 64; }",
            AssertKind::ShiftOutOfRange,
        ),
        (
            "fn f(u8 a)->u8 { return a << 8; }",
            AssertKind::ShiftOutOfRange,
        ),
        (
            "fn f(u0 a, u0 b)->u0 { return a / b; }",
            AssertKind::DivisionByZero,
        ),
        ("fn f()->u0 { return 0 << 0; }", AssertKind::ShiftOutOfRange),
        ("fn f()->i0 { return 0 >> 0; }", AssertKind::ShiftOutOfRange),
    ] {
        assert_eq!(known_panics(src), [kind], "{src}");
    }
}

#[test]
fn accepts_operations_that_may_succeed() {
    for src in [
        "fn f(i32 a)->i32 { return 1 / a; }",
        "fn f(bool set, i32 a)->i32 { if set { a = 0; } return 1 / a; }",
        "fn f(i32 a)->i32 { let i32 z = 0; z = a; return 1 / z; }",
        "fn f()->i32 { let i32 z = 0; let *i32 p = @z; *p = 1; return 1 / z; }",
        "fn f(i8 a, u8 b)->bool { return a < b; }",
        "fn f()->f64 { return 1.0 / 0.0; }",
        "fn f()->u8 { return 2 + 3; }",
        "fn f()->u8 { return ~0; }",
        "fn f()->u0 { return ~0; }",
        "fn f(i32 a)->i32 { if false { return 1 / 0; } return a; }",
        "fn f()->i32 { let i32 z = 0; if z != 0 { return 1 / z; } return 0; }",
    ] {
        assert_eq!(known_panics(src), [], "{src}");
    }
}

#[test]
fn reports_every_known_panic_in_order() {
    let src = "fn f()->i32 { let i32 a = 1 / 0; let i32 b = 1 % 0; return 0; }";
    assert_eq!(
        known_panics(src),
        [AssertKind::DivisionByZero, AssertKind::RemainderByZero]
    );
}
