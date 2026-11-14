use crate::frontend::precedence::UNARY_BP;
use crate::frontend::tokens::TokenKind;
use crate::source::Span;
use num_bigint::BigInt;

pub use crate::types::Type;

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: Option<String>,
    pub ty: Type,
    pub default: Option<Box<Expr>>,
    pub span: Span,
}

impl PartialEq for Binding {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ty == other.ty && self.default == other.default
    }
}

/// The left-hand side of a destructuring `let`.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `<type> <name>`: declares a variable.
    Binding(Binding),
    /// `p1, p2, ..` or `(p1, p2, ..)`: destructures a tuple of the same
    /// arity. `(p,)` is a one-element tuple; `(p)` is just `p`.
    Tuple(Vec<Pattern>, Span),
    /// `_`: ignores the value.
    Wildcard(Span),
}

impl Pattern {
    /// The source range of the pattern.
    pub fn span(&self) -> Span {
        match self {
            Pattern::Binding(binding) => binding.span,
            Pattern::Tuple(_, span) | Pattern::Wildcard(span) => *span,
        }
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Pattern::Binding(a), Pattern::Binding(b)) => a == b,
            (Pattern::Tuple(a, _), Pattern::Tuple(b, _)) => a == b,
            (Pattern::Wildcard(_), Pattern::Wildcard(_)) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `return value;`, or a bare `return;` (`None`), which returns the
    /// named results, or `()`.
    Return(Option<Box<Expr>>),
    Expr(Box<Expr>),

    /// Variable declaration: `<type> <name> = <expr>;`
    VarDecl(Binding),
    /// Destructuring declaration: `let i32 a, (bool b, _) = <expr>;`.
    ///
    /// `pattern` is never a plain [`Pattern::Binding`]; that form is a
    /// [`StmtKind::VarDecl`].
    Destructure {
        pattern: Pattern,
        init: Box<Expr>,
    },
    /// Assignment: `<target> = <value>`. The target is a place expression
    /// ([`ExprKind::Ident`], [`ExprKind::Deref`], [`ExprKind::Field`]), `_`,
    /// or a tuple of targets: `a, b = b, a` assigns in parallel.
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    /// Compound assignment: `<place> <op>= <value>`, e.g. `x += 1`.
    ///
    /// Deliberately *not* rewritten to `<place> = <place> <op> <value>`: the
    /// AST mirrors the source, and later stages own the lowering so that the
    /// target place is evaluated exactly once.
    CompoundAssign {
        target: Box<Expr>,
        op: BinOp,
        value: Box<Expr>,
    },
    /// Increment or decrement: `<place>++` / `<place>--`.
    IncDec {
        target: Box<Expr>,
        op: IncDecOp,
    },
    /// `if c { .. } else { .. }`, or `unless c { .. }` when `inverted`.
    ///
    /// An inverted `if` runs `then_branch` when the condition is false and
    /// never has an `else_branch`.
    If {
        inverted: bool,
        condition: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    Break(Option<Box<Expr>>),
    Continue,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl PartialEq for Stmt {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    // Literals
    Int(BigInt),
    /// Boolean literal (`true` / `false`).
    Bool(bool),
    /// Floating-point literal; lowered to the context type (e.g. `f32`, `f64`).
    Float(f64),
    /// Address literal (`@0x1234`). Builds a pointer from a constant machine
    /// address rather than from an addressable location.
    Address(BigInt),
    // Variable reference
    Ident(String),
    /// Address-of operator (`@x`). The expected type at the use site decides
    /// whether this produces a pointer (`*T`) or a reference (`&T`).
    AddressOf(Box<Expr>),
    /// Dereference operator (`*p`). Also valid as an assignment target.
    Deref(Box<Expr>),
    /// Tuple expression: `(a, b)`, `(a,)`, the unit value `()`, or a bare
    /// list `a, b` where a statement allows one.
    Tuple(Vec<Expr>),
    /// Tuple element access `t.0`. A place when `base` is one.
    Field {
        base: Box<Expr>,
        index: u32,
    },
    /// `_`. Only valid as an assignment target, where it discards a value.
    Wildcard,

    // Function call
    Call {
        name: String,
        args: Vec<Expr>,
    },

    // Arithmetic / logic
    BinOp {
        lhs: Box<Expr>,
        op: BinOp,
        rhs: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    /// Conversion `<operand> as <ty>`. The value must survive the
    /// conversion exactly: a cast that would change it panics at runtime.
    Cast {
        operand: Box<Expr>,
        ty: Type,
    },

    // Control flow
    IfElse {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Loop {
        body: Vec<Stmt>,
    },
    CondLoop {
        post: bool,
        inverted: bool,
        condition: Box<Expr>,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Expr {
    /// Returns `true` when this expression denotes an assignable location:
    /// a variable, a dereference, or an element of a place.
    pub fn is_place(&self) -> bool {
        match &self.kind {
            ExprKind::Ident(_) | ExprKind::Deref(_) => true,
            ExprKind::Field { base, .. } => base.is_place(),
            _ => false,
        }
    }

    /// Returns `true` when this expression can be assigned to with `=`: a
    /// place, `_`, or a non-empty tuple of such targets.
    pub fn is_assign_target(&self) -> bool {
        match &self.kind {
            ExprKind::Wildcard => true,
            ExprKind::Tuple(elems) => !elems.is_empty() && elems.iter().all(Expr::is_assign_target),
            _ => self.is_place(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    LogicalAnd,
    LogicalOr,
    LogicalXor,
    LShift,
    RShift,
}

impl BinOp {
    /// Maps a compound-assignment token (`+=`, `-=`, …) to the operator it
    /// applies, returning `None` for any other token (including plain `=`,
    /// `++`, and `--`).
    pub fn from_compound_assign_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Mod),
            TokenKind::AmpersandEq => Some(BinOp::BitwiseAnd),
            TokenKind::PipeEq => Some(BinOp::BitwiseOr),
            TokenKind::CaretEq => Some(BinOp::BitwiseXor),
            TokenKind::LShiftEq => Some(BinOp::LShift),
            TokenKind::RShiftEq => Some(BinOp::RShift),
            _ => None,
        }
    }
}

/// The `++` and `--` operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    Increment,
    Decrement,
}

impl IncDecOp {
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::PlusPlus => Some(IncDecOp::Increment),
            TokenKind::MinusMinus => Some(IncDecOp::Decrement),
            _ => None,
        }
    }

    /// The arithmetic applied between the target and `1`.
    pub fn to_binop(self) -> BinOp {
        match self {
            IncDecOp::Increment => BinOp::Add,
            IncDecOp::Decrement => BinOp::Sub,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,        // -x
    Not,        // not x (logical NOT)
    BitwiseNot, // ~x
}

impl UnaryOp {
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Not => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitwiseNot),
            _ => None,
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitwiseNot => UNARY_BP,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Binding>,
    /// The results after `->`, in order: `-> i32`, `-> i32 r`, or
    /// `-> i32 q, i32 r`. Either every result is named or none is. Empty
    /// when the `->` is omitted, which means `-> ()`.
    pub returns: Vec<Binding>,
    pub body: Vec<Stmt>,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params == other.params
            && self.returns == other.returns
            && self.body == other.body
            && self.attributes == other.attributes
    }
}

impl Function {
    /// The return type: `()` without results, the type of a single result,
    /// or the tuple of several results' types.
    pub fn return_ty(&self) -> Type {
        match self.returns.as_slice() {
            [] => Type::unit(),
            [single] => single.ty.clone(),
            results => Type::Tuple(results.iter().map(|r| r.ty.clone()).collect()),
        }
    }

    /// Whether the results are named (`-> i32 q, i32 r`).
    pub fn has_named_results(&self) -> bool {
        self.returns.iter().any(|result| result.name.is_some())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
}
