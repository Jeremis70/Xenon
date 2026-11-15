use crate::frontend::precedence::UNARY_BP;
use crate::frontend::tokens::TokenKind;
use crate::source::Span;
use num_bigint::BigInt;

use crate::types::Type;

/// A type as written in the source, before names are resolved.
///
/// The parser cannot know which names are user-defined types, so the AST
/// keeps the syntax and the type checker resolves it to a [`Type`].
#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExprKind {
    /// A built-in scalar type, such as `i32` or `bool`.
    Primitive(Type),
    /// `*T`
    Pointer(Box<TypeExpr>),
    /// `&T`
    Reference(Box<TypeExpr>),
    /// `(T, U, ..)`; empty for the unit type `()`.
    Tuple(Vec<TypeExpr>),
    /// `[N]T`
    Array(Box<TypeExpr>, u32),
    /// A user-defined type, by name.
    Named(String),
}

impl PartialEq for TypeExpr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

/// Structural comparison with a resolved type. A [`TypeExprKind::Named`]
/// type is unresolved and equals no [`Type`].
impl PartialEq<Type> for TypeExpr {
    fn eq(&self, other: &Type) -> bool {
        match (&self.kind, other) {
            (TypeExprKind::Primitive(ty), other) => ty == other,
            (TypeExprKind::Pointer(inner), Type::Pointer(other))
            | (TypeExprKind::Reference(inner), Type::Reference(other)) => **inner == **other,
            (TypeExprKind::Tuple(elems), Type::Tuple(others)) => {
                elems.len() == others.len() && elems.iter().zip(others).all(|(a, b)| a == b)
            }
            (TypeExprKind::Array(elem, len), Type::Array(other, other_len)) => {
                len == other_len && **elem == **other
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TypeExprKind::Primitive(ty) => write!(f, "{ty}"),
            TypeExprKind::Pointer(inner) => write!(f, "*{inner}"),
            TypeExprKind::Reference(inner) => write!(f, "&{inner}"),
            TypeExprKind::Array(elem, len) => write!(f, "[{len}]{elem}"),
            TypeExprKind::Tuple(elems) => {
                f.write_str("(")?;
                for (index, elem) in elems.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{elem}")?;
                }
                // A one-tuple keeps its comma: `(i32,)` is not `(i32)`.
                if elems.len() == 1 {
                    f.write_str(",")?;
                }
                f.write_str(")")
            }
            TypeExprKind::Named(name) => f.write_str(name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: Option<String>,
    pub ty: TypeExpr,
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
    /// `for <header> { .. }`: runs the body once per element.
    For {
        header: ForHeader,
        body: Vec<Stmt>,
    },
    /// `repeat <count> { .. }`: runs the body `count` times.
    Repeat {
        count: Box<Expr>,
        body: Vec<Stmt>,
    },
}

/// What a `for` loop walks and the names it introduces:
/// `[type] name in iterable [with ..] [where ..]`.
#[derive(Debug, Clone)]
pub struct ForHeader {
    /// The written type of the loop variable, if any.
    pub ty: Option<TypeExpr>,
    /// The loop variable; `None` for `_`.
    pub name: Option<String>,
    /// The source range of the loop variable's name.
    pub name_span: Span,
    pub iterable: ForIterable,
    /// The `with` and `where` clauses, in source order.
    pub clauses: Vec<ForClause>,
}

impl PartialEq for ForHeader {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty
            && self.name == other.name
            && self.iterable == other.iterable
            && self.clauses == other.clauses
    }
}

/// What a `for` loop walks.
#[derive(Debug, Clone, PartialEq)]
pub enum ForIterable {
    /// `start to end` or `start until end`, with an optional `by` step.
    Range(RangeExpr),
    /// Any other expression, such as an array.
    Value(Box<Expr>),
}

/// An integer range in a `for` header.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    /// `true` for `to`, which includes `end`; `false` for `until`.
    pub inclusive: bool,
    pub step: Option<RangeStep>,
}

/// The literal after `by`: `by 4` or `by -1`.
#[derive(Debug, Clone)]
pub struct RangeStep {
    /// The absolute value of the step.
    pub magnitude: BigInt,
    /// Whether the step was written with a `-`.
    pub negative: bool,
    pub span: Span,
}

impl PartialEq for RangeStep {
    fn eq(&self, other: &Self) -> bool {
        self.magnitude == other.magnitude && self.negative == other.negative
    }
}

/// A clause after the iterable of a `for` loop.
#[derive(Debug, Clone)]
pub enum ForClause {
    /// `with [type] name`: counts the elements that reach the clause.
    With {
        ty: Option<TypeExpr>,
        name: String,
        span: Span,
    },
    /// `where condition`: skips the elements the condition rejects.
    Where(Expr),
}

impl PartialEq for ForClause {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ForClause::With { ty: a, name: x, .. }, ForClause::With { ty: b, name: y, .. }) => {
                a == b && x == y
            }
            (ForClause::Where(a), ForClause::Where(b)) => a == b,
            _ => false,
        }
    }
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
    /// A homogeneous array literal `[a, b, ..]`.
    Array(Vec<Expr>),
    /// `base[i]`, or the assignment-only selection `base[i, j, ..]`.
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
    },
    /// Tuple element access `t.0`. A place when `base` is one.
    Field {
        base: Box<Expr>,
        index: u32,
    },
    /// Member access by name, `r.width`. A place when `base` is one.
    ///
    /// The span of the expression runs from `base` to the name.
    Member {
        base: Box<Expr>,
        name: String,
        name_span: Span,
    },
    /// A call through a member name, `r.scale(2)` or `Rect.unit()`.
    ///
    /// Boxed to keep expression nodes small: the parser holds several on
    /// every level of nesting.
    MethodCall(Box<MethodCall>),
    /// `_`. Only valid as an assignment target, where it discards a value.
    Wildcard,

    // Function call
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// A call whose arguments are all named, `Rect(width = 1, height = 2)`,
    /// which builds a struct from its fields.
    NamedCall {
        name: String,
        args: Vec<NamedArg>,
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
        ty: TypeExpr,
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

/// `<receiver>.<name>(<args>)`. Whether `receiver` is a value or names a
/// struct is for the type checker to say.
#[derive(Debug, Clone)]
pub struct MethodCall {
    pub receiver: Expr,
    pub name: String,
    pub name_span: Span,
    pub args: Vec<Expr>,
}

impl PartialEq for MethodCall {
    fn eq(&self, other: &Self) -> bool {
        self.receiver == other.receiver && self.name == other.name && self.args == other.args
    }
}

/// One argument of a [`ExprKind::NamedCall`]: `<name> = <value>`.
#[derive(Debug, Clone)]
pub struct NamedArg {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
}

impl PartialEq for NamedArg {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value == other.value
    }
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Expr {
    /// Returns `true` when this expression denotes an assignable location:
    /// a variable, a dereference, or an element or member of a place.
    ///
    /// This is syntax only: whether a member is a stored field is for the
    /// type checker to say.
    pub fn is_place(&self) -> bool {
        match &self.kind {
            ExprKind::Ident(_) | ExprKind::Deref(_) => true,
            ExprKind::Field { base, .. } | ExprKind::Member { base, .. } => base.is_place(),
            ExprKind::Index { base, indices } => indices.len() == 1 && base.is_place(),
            _ => false,
        }
    }

    /// Returns `true` when this expression can be assigned to with `=`: a
    /// place, `_`, or a non-empty tuple of such targets.
    pub fn is_assign_target(&self) -> bool {
        match &self.kind {
            ExprKind::Wildcard => true,
            ExprKind::Tuple(elems) => !elems.is_empty() && elems.iter().all(Expr::is_assign_target),
            ExprKind::Index { base, indices } => !indices.is_empty() && base.is_place(),
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
    /// How a method takes the value it is called on; `None` for a function
    /// that is not a method. Not one of `params`.
    pub receiver: Option<Receiver>,
    pub span: Span,
}

/// The receiver of a method, written as its first parameter.
#[derive(Debug, Clone, Copy)]
pub enum Receiver {
    /// `self`: the method gets a copy of the value.
    Value(Span),
    /// `&self`: the method gets a reference to the value and can change it.
    Reference(Span),
}

impl PartialEq for Receiver {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params == other.params
            && self.returns == other.returns
            && self.body == other.body
            && self.attributes == other.attributes
            && self.receiver == other.receiver
    }
}

impl Function {
    /// Whether the results are named (`-> i32 q, i32 r`).
    pub fn has_named_results(&self) -> bool {
        self.returns.iter().any(|result| result.name.is_some())
    }
}

/// One field of a struct declaration: `<type> <name>;`.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub ty: TypeExpr,
    pub name: String,
    pub span: Span,
}

impl PartialEq for FieldDecl {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty && self.name == other.name
    }
}

/// `struct <name> { <fields> }`. The fields are stored in this order.
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

impl PartialEq for StructDecl {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}

/// `impl <target> { <members> }`: the methods and associated functions of
/// a struct. A struct may have several such blocks.
#[derive(Debug, Clone)]
pub struct ImplBlock {
    /// The name of the struct.
    pub target: String,
    pub target_span: Span,
    pub members: Vec<ImplMember>,
    pub span: Span,
}

impl PartialEq for ImplBlock {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target && self.members == other.members
    }
}

/// One member of an [`ImplBlock`].
#[derive(Debug, Clone, PartialEq)]
pub enum ImplMember {
    /// A method if it has a [`Receiver`], otherwise an associated function.
    Function(Function),
    /// A computed property.
    Property(PropertyDecl),
}

/// `<type> <name> { <body> }` in an `impl`: a member read like a field,
/// `r.name`, whose value the body computes from `self` on every read. It
/// takes no room in the struct.
#[derive(Debug, Clone)]
pub struct PropertyDecl {
    pub ty: TypeExpr,
    pub name: String,
    pub name_span: Span,
    pub body: Vec<Stmt>,
    pub span: Span,
}

impl PartialEq for PropertyDecl {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty && self.name == other.name && self.body == other.body
    }
}

/// Every item of a program, each kind in source order.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
    pub structs: Vec<StructDecl>,
    pub impls: Vec<ImplBlock>,
}
