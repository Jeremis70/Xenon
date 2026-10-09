//! The vocabulary of MIR: statements, terminators, places, operands, and
//! rvalues.
//!
//! MIR is deliberately small. Every source construct is lowered to a
//! combination of these few primitives, so analyses, optimizations, and
//! backends only ever handle this closed set. Adding a language feature
//! should usually mean lowering it to existing primitives; extending this
//! module is reserved for genuinely new runtime capabilities.

use std::fmt;

use num_bigint::BigInt;

use crate::middle::ids::DefId;
use crate::types::Type;

pub use crate::middle::ops::{BinOp, CastKind, IndirectionKind, UnOp};

use super::body::{BasicBlock, Local, SourceInfo};

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

/// A non-branching operation inside a basic block.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// Where the statement came from.
    pub source_info: SourceInfo,
    /// What the statement does.
    pub kind: StatementKind,
}

impl Statement {
    /// The statement `place = rvalue`.
    pub fn assign(source_info: SourceInfo, place: Place, rvalue: Rvalue) -> Self {
        Self {
            source_info,
            kind: StatementKind::Assign(Box::new((place, rvalue))),
        }
    }
}

/// The operation performed by a [`Statement`].
#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    /// Evaluates the rvalue, then writes it to the place.
    Assign(Box<(Place, Rvalue)>),
    /// Marks the start of the local's live range; its value is uninitialized.
    StorageLive(Local),
    /// Marks the end of the local's live range; its value becomes invalid.
    StorageDead(Local),
    /// Does nothing. Passes replace statements with `Nop` instead of
    /// removing them so that [`Location`](super::Location)s stay stable.
    Nop,
}

// ---------------------------------------------------------------------------
// Terminators
// ---------------------------------------------------------------------------

/// The control-flow transfer that ends a basic block.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminator {
    /// Where the terminator came from.
    pub source_info: SourceInfo,
    /// Where control goes next.
    pub kind: TerminatorKind,
}

/// The kind of control-flow transfer performed by a [`Terminator`].
#[derive(Debug, Clone, PartialEq)]
pub enum TerminatorKind {
    /// Unconditionally jumps to `target`.
    Goto { target: BasicBlock },
    /// Jumps to the target matching the integer or boolean `discr`.
    SwitchInt {
        discr: Operand,
        targets: SwitchTargets,
    },
    /// Calls `func` with `args`, stores the result to `destination`, then
    /// continues at `target`. `target` is `None` if the callee never returns.
    Call {
        func: DefId,
        args: Vec<Operand>,
        destination: Place,
        target: Option<BasicBlock>,
    },
    /// Continues at `target` if `cond == expected`; otherwise aborts the
    /// program with the runtime error described by `kind`.
    Assert {
        cond: Operand,
        expected: bool,
        kind: AssertKind,
        target: BasicBlock,
    },
    /// Returns the value in the return place `_0` to the caller.
    Return,
    /// Marks code that can never execute. Reaching it is undefined behavior.
    Unreachable,
    /// Control reaches the end of the function body without a `return`.
    ///
    /// Only valid in [`MirPhase::Built`](super::MirPhase::Built): flow
    /// checking either reports it as a missing return or replaces it.
    EndOfBody,
}

impl TerminatorKind {
    /// The blocks control may transfer to, in a deterministic order.
    pub fn successors(&self) -> impl DoubleEndedIterator<Item = BasicBlock> + '_ {
        let (single, many): (Option<BasicBlock>, &[BasicBlock]) = match self {
            TerminatorKind::Goto { target } | TerminatorKind::Assert { target, .. } => {
                (Some(*target), &[])
            }
            TerminatorKind::Call { target, .. } => (*target, &[]),
            TerminatorKind::SwitchInt { targets, .. } => (None, targets.all_targets()),
            TerminatorKind::Return | TerminatorKind::Unreachable | TerminatorKind::EndOfBody => {
                (None, &[])
            }
        };
        single.into_iter().chain(many.iter().copied())
    }

    /// Mutable references to every successor, e.g. to retarget edges.
    pub fn successors_mut(&mut self) -> impl Iterator<Item = &mut BasicBlock> + '_ {
        let (single, many): (Option<&mut BasicBlock>, &mut [BasicBlock]) = match self {
            TerminatorKind::Goto { target } | TerminatorKind::Assert { target, .. } => {
                (Some(target), &mut [])
            }
            TerminatorKind::Call { target, .. } => (target.as_mut(), &mut []),
            TerminatorKind::SwitchInt { targets, .. } => (None, targets.all_targets_mut()),
            TerminatorKind::Return | TerminatorKind::Unreachable | TerminatorKind::EndOfBody => {
                (None, &mut [])
            }
        };
        single.into_iter().chain(many.iter_mut())
    }
}

/// The value-to-block table of a [`TerminatorKind::SwitchInt`].
///
/// Values are paired with targets positionally; the extra final target is
/// taken when no value matches. Booleans switch on `0` (false) and `1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTargets {
    values: Vec<BigInt>,
    // Invariant: `targets.len() == values.len() + 1`.
    targets: Vec<BasicBlock>,
}

impl SwitchTargets {
    /// Builds a table from `(value, target)` pairs and a fallback target.
    pub fn new(
        branches: impl IntoIterator<Item = (BigInt, BasicBlock)>,
        otherwise: BasicBlock,
    ) -> Self {
        let (values, mut targets): (Vec<_>, Vec<_>) = branches.into_iter().unzip();
        targets.push(otherwise);
        Self { values, targets }
    }

    /// The table of an `if`: `0` (false) goes to `else_`, anything else to
    /// `then`.
    pub fn bool(then: BasicBlock, else_: BasicBlock) -> Self {
        Self::new([(BigInt::ZERO, else_)], then)
    }

    /// The explicit `(value, target)` branches, excluding the fallback.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&BigInt, BasicBlock)> + '_ {
        self.values.iter().zip(self.targets.iter().copied())
    }

    /// The target taken when no explicit value matches.
    pub fn otherwise(&self) -> BasicBlock {
        // The constructors always push the fallback last, so this never
        // falls back to the default.
        self.targets
            .last()
            .copied()
            .unwrap_or(BasicBlock::from_u32(0))
    }

    /// Every target, explicit branches first, fallback last.
    pub fn all_targets(&self) -> &[BasicBlock] {
        &self.targets
    }

    /// Mutable access to every target, explicit branches first.
    pub fn all_targets_mut(&mut self) -> &mut [BasicBlock] {
        &mut self.targets
    }

    /// The block control reaches when the discriminant equals `value`.
    pub fn target_for_value(&self, value: &BigInt) -> BasicBlock {
        self.iter()
            .find_map(|(candidate, target)| (candidate == value).then_some(target))
            .unwrap_or_else(|| self.otherwise())
    }
}

/// The runtime failure reported when an [`TerminatorKind::Assert`] fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssertKind {
    /// The arithmetic operation overflowed its type.
    Overflow(BinOp),
    /// Integer division by zero.
    DivisionByZero,
    /// Integer remainder by zero.
    RemainderByZero,
    /// Signed division of the minimum value by `-1`.
    SignedDivisionOverflow,
    /// Signed remainder of the minimum value by `-1`.
    SignedRemainderOverflow,
    /// Shift amount is negative or not smaller than the bit width.
    ShiftOutOfRange,
}

impl AssertKind {
    /// The panic message reported to the user when the assertion fails.
    pub fn description(&self) -> &'static str {
        match self {
            AssertKind::Overflow(BinOp::Add) => "attempt to add with overflow",
            AssertKind::Overflow(BinOp::Sub) => "attempt to subtract with overflow",
            AssertKind::Overflow(BinOp::Mul) => "attempt to multiply with overflow",
            AssertKind::Overflow(_) => "arithmetic overflow",
            AssertKind::DivisionByZero => "attempt to divide by zero",
            AssertKind::RemainderByZero => {
                "attempt to calculate the remainder with a divisor of zero"
            }
            AssertKind::SignedDivisionOverflow => "attempt to divide with overflow",
            AssertKind::SignedRemainderOverflow => "attempt to calculate remainder with overflow",
            AssertKind::ShiftOutOfRange => "attempt to shift out of range",
        }
    }
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

/// A memory location: a local, optionally followed by projections.
///
/// `(*_1)` is `Place { local: _1, projection: [Deref] }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    /// The base local.
    pub local: Local,
    /// Projections applied left to right to the base local.
    pub projection: Vec<ProjectionElem>,
}

impl Place {
    /// Returns the place `*self`.
    pub fn deref(mut self) -> Self {
        self.projection.push(ProjectionElem::Deref);
        self
    }

    /// Returns the local if the place has no projections.
    pub fn as_local(&self) -> Option<Local> {
        self.projection.is_empty().then_some(self.local)
    }

    /// Returns `true` if accessing the place goes through a pointer.
    pub fn is_indirect(&self) -> bool {
        self.projection.contains(&ProjectionElem::Deref)
    }
}

impl From<Local> for Place {
    fn from(local: Local) -> Self {
        Self {
            local,
            projection: Vec::new(),
        }
    }
}

/// One step of a [`Place`] projection.
///
/// Arrays will add `Index(Local)` and `ConstantIndex`; structs will add
/// `Field`. Each new element only needs typing, verification, and backend
/// support; analyses see it through [`super::visit`] automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionElem {
    /// Follows a pointer or reference to its pointee.
    Deref,
}

// ---------------------------------------------------------------------------
// Operands and constants
// ---------------------------------------------------------------------------

/// A value consumed by an rvalue or terminator.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Reads the current value of a place.
    Copy(Place),
    /// A compile-time constant.
    Constant(Box<Constant>),
}

impl Operand {
    /// Builds a constant operand.
    pub fn constant(constant: Constant) -> Self {
        Operand::Constant(Box::new(constant))
    }

    /// Returns the place read by this operand, if any.
    pub fn place(&self) -> Option<&Place> {
        match self {
            Operand::Copy(place) => Some(place),
            Operand::Constant(_) => None,
        }
    }
}

/// A typed compile-time value.
#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    /// The type of the value.
    pub ty: Type,
    /// The value itself.
    pub value: ConstValue,
}

impl Constant {
    /// A `bool` constant.
    pub fn bool(value: bool) -> Self {
        Self {
            ty: Type::Bool,
            value: ConstValue::Bool(value),
        }
    }

    /// An integer constant of integer type `ty`.
    pub fn int(value: impl Into<BigInt>, ty: Type) -> Self {
        Self {
            ty,
            value: ConstValue::Int(value.into()),
        }
    }

    /// A floating-point constant of float type `ty`.
    pub fn float(value: f64, ty: Type) -> Self {
        Self {
            ty,
            value: ConstValue::Float(value),
        }
    }

    /// The zero value of `ty`: `false`, `0`, `0.0`, or the null address.
    ///
    /// Returns `None` for types without a zero value; every type of the
    /// language has one today, but `void` or `never` would not.
    pub fn zero(ty: &Type) -> Option<Self> {
        let value = if ty.is_bool() {
            ConstValue::Bool(false)
        } else if ty.is_integer() {
            ConstValue::Int(BigInt::ZERO)
        } else if ty.is_float() {
            ConstValue::Float(0.0)
        } else if ty.is_indirect() {
            ConstValue::Address(BigInt::ZERO)
        } else {
            return None;
        };
        Some(Self {
            ty: ty.clone(),
            value,
        })
    }

    /// A raw address constant of pointer type `ty`.
    pub fn address(value: impl Into<BigInt>, ty: Type) -> Self {
        Self {
            ty,
            value: ConstValue::Address(value.into()),
        }
    }
}

/// The payload of a [`Constant`].
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    /// A boolean.
    Bool(bool),
    /// A mathematical integer; must fit the constant's integer type.
    Int(BigInt),
    /// A floating-point value, rounded to the constant's float type by the
    /// backend.
    Float(f64),
    /// A machine address; must fit the target pointer width.
    Address(BigInt),
}

// ---------------------------------------------------------------------------
// Rvalues
// ---------------------------------------------------------------------------

/// A computation producing a value, written to a place by an assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum Rvalue {
    /// The operand's value, unchanged.
    Use(Operand),
    /// A unary operation.
    UnaryOp(UnOp, Operand),
    /// A binary operation; the result type follows [`BinOp`].
    BinaryOp(BinOp, Box<(Operand, Operand)>),
    /// `true` if the binary operation would overflow its integer type.
    ///
    /// Only `Add`, `Sub`, and `Mul` are allowed. Runtime lowering pairs it
    /// with an [`TerminatorKind::Assert`] to implement checked arithmetic.
    Overflows(BinOp, Box<(Operand, Operand)>),
    /// Converts the operand to the given type.
    Cast(CastKind, Operand, Type),
    /// Takes the address of a place, producing a `*T` or `&T`.
    AddressOf(IndirectionKind, Place),
}

impl Rvalue {
    /// The rvalue `op(lhs, rhs)`.
    pub fn binary(op: BinOp, lhs: Operand, rhs: Operand) -> Self {
        Rvalue::BinaryOp(op, Box::new((lhs, rhs)))
    }

    /// The rvalue `Overflows<op>(lhs, rhs)`.
    pub fn overflows(op: BinOp, lhs: Operand, rhs: Operand) -> Self {
        Rvalue::Overflows(op, Box::new((lhs, rhs)))
    }
}

// ---------------------------------------------------------------------------
// Textual forms (shared by the pretty-printer and diagnostics)
// ---------------------------------------------------------------------------

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for elem in self.projection.iter().rev() {
            match elem {
                ProjectionElem::Deref => f.write_str("(*")?,
            }
        }
        write!(f, "{}", self.local)?;
        for elem in &self.projection {
            match elem {
                ProjectionElem::Deref => f.write_str(")")?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            ConstValue::Bool(value) => write!(f, "const {value}"),
            ConstValue::Int(value) => write!(f, "const {value}_{}", self.ty),
            ConstValue::Float(value) => write!(f, "const {value:?}_{}", self.ty),
            ConstValue::Address(value) => write!(f, "const @{value:#x}: {}", self.ty),
        }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Copy(place) => write!(f, "copy {place}"),
            Operand::Constant(constant) => write!(f, "{constant}"),
        }
    }
}

impl fmt::Display for Rvalue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rvalue::Use(operand) => write!(f, "{operand}"),
            Rvalue::UnaryOp(op, operand) => write!(f, "{}({operand})", op.name()),
            Rvalue::BinaryOp(op, operands) => {
                write!(f, "{}({}, {})", op.name(), operands.0, operands.1)
            }
            Rvalue::Overflows(op, operands) => {
                write!(f, "Overflows{}({}, {})", op.name(), operands.0, operands.1)
            }
            Rvalue::Cast(kind, operand, ty) => write!(f, "{operand} as {ty} ({kind:?})"),
            Rvalue::AddressOf(IndirectionKind::Pointer, place) => write!(f, "@raw {place}"),
            Rvalue::AddressOf(IndirectionKind::Reference, place) => write!(f, "@ref {place}"),
        }
    }
}

impl fmt::Display for StatementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatementKind::Assign(assign) => write!(f, "{} = {}", assign.0, assign.1),
            StatementKind::StorageLive(local) => write!(f, "StorageLive({local})"),
            StatementKind::StorageDead(local) => write!(f, "StorageDead({local})"),
            StatementKind::Nop => f.write_str("nop"),
        }
    }
}

impl fmt::Display for TerminatorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerminatorKind::Goto { target } => write!(f, "goto -> {target}"),
            TerminatorKind::SwitchInt { discr, targets } => {
                write!(f, "switchInt({discr}) -> [")?;
                for (value, target) in targets.iter() {
                    write!(f, "{value}: {target}, ")?;
                }
                write!(f, "otherwise: {}]", targets.otherwise())
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
            } => write_call(f, func, args, destination, *target),
            TerminatorKind::Assert {
                cond,
                expected,
                kind,
                target,
            } => {
                let negation = if *expected { "" } else { "!" };
                write!(
                    f,
                    "assert({negation}{cond}, \"{}\") -> {target}",
                    kind.description()
                )
            }
            TerminatorKind::Return => f.write_str("return"),
            TerminatorKind::Unreachable => f.write_str("unreachable"),
            TerminatorKind::EndOfBody => f.write_str("end_of_body"),
        }
    }
}

/// Writes a call terminator as `destination = callee(args) -> target`.
///
/// `callee` is a parameter so that dumps can print a function name where
/// plain `Display` only knows the [`DefId`].
pub(super) fn write_call(
    out: &mut dyn fmt::Write,
    callee: impl fmt::Display,
    args: &[Operand],
    destination: &Place,
    target: Option<BasicBlock>,
) -> fmt::Result {
    write!(out, "{destination} = {callee}(")?;
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            out.write_str(", ")?;
        }
        write!(out, "{arg}")?;
    }
    match target {
        Some(target) => write!(out, ") -> {target}"),
        None => out.write_str(") -> !"),
    }
}
