//! THIR: the typed high-level intermediate representation.
//!
//! THIR is the output of [type checking](crate::middle::typeck) and the
//! input of [MIR construction](crate::middle::mir::build). It keeps the tree
//! shape of the source program, but every question the AST leaves open has
//! been answered:
//!
//! - every name is resolved: variables to a [`BindingId`], functions to a
//!   [`DefId`]; shadowed variables are distinct bindings;
//! - every expression carries its concrete [`Type`]; literals have the type
//!   their context gave them and have been range-checked;
//! - every implicit conversion is explicit: integer widening is a
//!   [`ExprKind::Cast`], reading through a reference is an
//!   [`ExprKind::Deref`], and `@x` records whether it builds a `*T` or `&T`.
//!
//! Consumers therefore never re-infer types and never consult the AST.
//!
//! Module map:
//! - this module: the THIR data types;
//! - [`visit`]: a read-only [`Visitor`](visit::Visitor);
//! - [`pretty`]: deterministic textual dumps.

pub mod pretty;
pub mod visit;

use num_bigint::BigInt;

use crate::index::IndexVec;
use crate::middle::ids::{BindingId, DefId};
use crate::middle::ops::{BinOp, CastKind, IndirectionKind, UnOp};
use crate::source::Span;
use crate::types::Type;

/// A whole type-checked program.
#[derive(Debug, Clone, PartialEq)]
pub struct ThirProgram {
    /// Every function, indexed by [`DefId`] in source order.
    pub functions: IndexVec<DefId, Function>,
    /// The first function marked `#[entry]`, if any.
    pub entry: Option<DefId>,
}

/// A type-checked function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// The source-level name.
    pub name: String,
    /// The bindings of the parameters, in declaration order.
    ///
    /// Unnamed parameters still get a binding so that positions line up
    /// with the signature.
    pub params: Vec<BindingId>,
    /// The declared return type.
    pub return_ty: Type,
    /// The binding of a named return value (`-> i32 result`), if any.
    ///
    /// It is an ordinary zero-initialized variable; `return e` still
    /// returns `e`, not the binding.
    pub named_return: Option<BindingId>,
    /// Every binding of the function, indexed by [`BindingId`].
    pub bindings: IndexVec<BindingId, Binding>,
    /// The function body.
    pub body: Block,
    /// The span of the whole function.
    pub span: Span,
}

impl Function {
    /// The parameter types, in declaration order.
    pub fn param_tys(&self) -> impl ExactSizeIterator<Item = &Type> + '_ {
        self.params
            .iter()
            .map(|&binding| &self.bindings[binding].ty)
    }
}

/// A variable binding.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// The source-level name, `None` for unnamed parameters.
    pub name: Option<String>,
    /// The declared type. A binding of type `&T` stores the reference
    /// itself; uses of the variable go through an explicit
    /// [`ExprKind::Deref`].
    pub ty: Type,
    /// What introduced the binding.
    pub kind: BindingKind,
    /// Where the binding was declared.
    pub span: Span,
}

/// What introduced a [`Binding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// A function parameter.
    Param,
    /// A named return value.
    NamedReturn,
    /// A local variable declaration.
    Local,
}

/// A sequence of statements forming one lexical scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The statements, in execution order.
    pub stmts: Vec<Stmt>,
    /// The source range of the block.
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// What the statement does.
    pub kind: StmtKind,
    /// The source range of the statement.
    pub span: Span,
}

/// The kinds of [`Stmt`].
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// Declares `binding`, optionally initialized. The binding is in scope
    /// from the next statement onward, so `init` cannot refer to it.
    Let {
        binding: BindingId,
        init: Option<Expr>,
    },
    /// Evaluates an expression for its side effects.
    Expr(Expr),
    /// Evaluates `place`, then `value`, then stores `value` into `place`.
    Assign { place: Expr, value: Expr },
    /// `place op= value`: evaluates `place` once, then `value`, then reads
    /// the old value, applies `op`, and stores the result.
    ///
    /// `value` already has the type `op` requires. `x++` and `x--` are
    /// represented as `x += 1` and `x -= 1`.
    CompoundAssign { op: BinOp, place: Expr, value: Expr },
    /// Returns `value` from the function.
    Return(Expr),
    /// A conditional statement.
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// Leaves the innermost loop, giving it `value` as its result.
    ///
    /// `value` already has the loop's result type.
    Break(Option<Expr>),
    /// Starts the next iteration of the innermost loop.
    Continue,
}

/// A typed expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// What the expression computes.
    pub kind: ExprKind,
    /// The concrete type of the value.
    pub ty: Type,
    /// The source range of the expression.
    pub span: Span,
}

impl Expr {
    /// Returns `true` if the expression denotes a memory location rather
    /// than a value.
    pub fn is_place(&self) -> bool {
        matches!(self.kind, ExprKind::Binding(_) | ExprKind::Deref(_))
    }
}

/// The kinds of [`Expr`].
///
/// Place expressions ([`Binding`](ExprKind::Binding) and
/// [`Deref`](ExprKind::Deref)) denote memory locations; using one as a value
/// reads it. Every other expression produces a value.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// A constant of the expression's type.
    Literal(Literal),
    /// The storage of a binding. For a binding of type `&T` this is the
    /// reference itself; the auto-dereferenced `T` is a [`ExprKind::Deref`]
    /// of it.
    Binding(BindingId),
    /// The location a pointer or reference points to.
    Deref(Box<Expr>),
    /// The address of a place, as a `*T` or `&T`.
    AddressOf {
        kind: IndirectionKind,
        place: Box<Expr>,
    },
    /// A unary operation; the result has the operand's type.
    Unary { op: UnOp, operand: Box<Expr> },
    /// A binary operation whose operands already have the types MIR typing
    /// requires (identical, except for shift amounts).
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A logical operation on two `bool`s. Both operands are always
    /// evaluated, left to right.
    Logical {
        op: LogicalOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Converts the operand to the expression's type.
    Cast { kind: CastKind, operand: Box<Expr> },
    /// Calls a function; arguments already have the parameter types.
    Call { callee: DefId, args: Vec<Expr> },
    /// A conditional expression; both branches have the expression's type.
    If {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// A loop, optionally guarded by a condition.
    ///
    /// The loop's value is the value of the `break` that left it, or zero
    /// when it exits through its condition or a value-less `break`.
    Loop {
        condition: Option<Box<LoopCondition>>,
        body: Block,
    },
}

/// The value of an [`ExprKind::Literal`].
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// A boolean.
    Bool(bool),
    /// An integer that fits the expression's integer type.
    Int(BigInt),
    /// A float, rounded to the expression's float type by the backend.
    Float(f64),
    /// A machine address that fits the target pointer width.
    Address(BigInt),
}

/// Logical operators. Today they are eager: both operands are evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalOp {
    /// `&&`
    And,
    /// `||`
    Or,
    /// `^^`
    Xor,
}

impl LogicalOp {
    /// The name used in dumps.
    pub fn name(self) -> &'static str {
        match self {
            LogicalOp::And => "And",
            LogicalOp::Or => "Or",
            LogicalOp::Xor => "Xor",
        }
    }
}

/// The guard of a conditional loop.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopCondition {
    /// The `bool` condition.
    pub expr: Expr,
    /// Whether the condition is tested before or after each iteration.
    pub placement: ConditionPlacement,
    /// The condition value that keeps the loop running: `true` for `while`,
    /// `false` for `until`.
    pub continue_if: bool,
}

/// When a [`LoopCondition`] is tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionPlacement {
    /// Before each iteration (`while c { .. }`).
    Before,
    /// After each iteration (`do { .. } while c`).
    After,
}
