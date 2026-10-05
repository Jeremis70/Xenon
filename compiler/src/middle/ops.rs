//! Operators shared by THIR and MIR.
//!
//! Both IRs speak the same operator vocabulary, so lowering THIR to MIR maps
//! operators one to one and never re-derives their meaning. Operator
//! *typing* rules live in [`crate::middle::mir::typing`].

use crate::types::Type;

/// Binary operators.
///
/// The logical operators `&&`, `||`, and `^^` are not binary operators:
/// THIR keeps them as [`LogicalOp`](crate::middle::thir::LogicalOp), and MIR
/// construction lowers today's eager forms to `BitAnd`, `BitOr`, and
/// `BitXor` on `bool` (short-circuiting forms would lower to control flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    /// `+ - * / %`
    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
        )
    }

    /// `& | ^`
    pub fn is_bitwise(self) -> bool {
        matches!(self, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
    }

    /// `<< >>`
    pub fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr)
    }

    /// `== != < <= > >=`; these produce `bool`.
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    /// Operators that [`Rvalue::Overflows`](crate::middle::mir::Rvalue::Overflows) accepts.
    pub fn is_overflow_checkable(self) -> bool {
        matches!(self, BinOp::Add | BinOp::Sub | BinOp::Mul)
    }

    /// The name used in pretty-printed MIR.
    pub fn name(self) -> &'static str {
        match self {
            BinOp::Add => "Add",
            BinOp::Sub => "Sub",
            BinOp::Mul => "Mul",
            BinOp::Div => "Div",
            BinOp::Rem => "Rem",
            BinOp::BitAnd => "BitAnd",
            BinOp::BitOr => "BitOr",
            BinOp::BitXor => "BitXor",
            BinOp::Shl => "Shl",
            BinOp::Shr => "Shr",
            BinOp::Eq => "Eq",
            BinOp::Ne => "Ne",
            BinOp::Lt => "Lt",
            BinOp::Le => "Le",
            BinOp::Gt => "Gt",
            BinOp::Ge => "Ge",
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// Arithmetic negation of an integer or float.
    Neg,
    /// Logical not of a `bool`, bitwise not of an integer.
    Not,
}

impl UnOp {
    /// The name used in pretty-printed MIR.
    pub fn name(self) -> &'static str {
        match self {
            UnOp::Neg => "Neg",
            UnOp::Not => "Not",
        }
    }
}

/// The conversion performed by an [`Rvalue::Cast`](crate::middle::mir::Rvalue::Cast) or a THIR
/// [`ExprKind::Cast`](crate::middle::thir::ExprKind::Cast).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastKind {
    /// Integer to integer: truncates, zero-extends, or sign-extends
    /// according to the widths and the source signedness.
    IntToInt,
    /// Float to float: rounds or extends.
    FloatToFloat,
    /// Integer to float.
    IntToFloat,
    /// Float to integer.
    FloatToInt,
    /// Pointer or reference to pointer or reference; the bits are unchanged.
    PtrToPtr,
}

/// Whether an address is a raw pointer (`*T`) or a reference (`&T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndirectionKind {
    /// `*T`, an opaque pointer.
    Pointer,
    /// `&T`, an auto-dereferenced reference.
    Reference,
}

impl IndirectionKind {
    /// The pointer type of this kind pointing at `pointee`.
    pub fn pointer_to(self, pointee: Type) -> Type {
        match self {
            IndirectionKind::Pointer => Type::Pointer(Box::new(pointee)),
            IndirectionKind::Reference => Type::Reference(Box::new(pointee)),
        }
    }
}
