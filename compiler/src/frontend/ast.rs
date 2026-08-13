use crate::error::TypeError;
use crate::frontend::tokens::{Span, TokenKind};
use num_bigint::BigInt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int(u32),  // Any bit-width integer
    UInt(u32), // Any bit-width integer

    USize,
    ISize,

    Float16,
    BFloat16,
    Float32,
    Float64,
    Float128,

    Bool,
}

impl Type {
    /// Returns `(min, max)` bounds for integer types using BigInt.
    /// Returns `None` for non-integer types.
    pub fn bounds(&self) -> Option<(BigInt, BigInt)> {
        match self {
            Type::UInt(n) => {
                let max = (BigInt::from(1) << n) - 1;
                Some((BigInt::ZERO, max))
            }
            Type::Int(n) => {
                let min = -(BigInt::from(1) << (n - 1));
                let max = (BigInt::from(1) << (n - 1)) - 1;
                Some((min, max))
            }
            _ => None,
        }
    }
}

impl FromStr for Type {
    type Err = TypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "usize" => return Ok(Type::USize),
            "isize" => return Ok(Type::ISize),
            "bool" => return Ok(Type::Bool),
            "f16" => return Ok(Type::Float16),
            "bf16" => return Ok(Type::BFloat16),
            "f32" => return Ok(Type::Float32),
            "f64" => return Ok(Type::Float64),
            "f128" => return Ok(Type::Float128),
            _ => {}
        }

        // Parameterised integer types: (i|u)<width>
        let (signed, digits) = if let Some(rest) = s.strip_prefix('i') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('u') {
            (false, rest)
        } else {
            return Err(TypeError::Unknown(s.to_owned()));
        };

        if digits.is_empty() {
            return Err(TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "missing bit width",
            });
        }

        let width = digits
            .parse::<u32>()
            .map_err(|_| TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width must be a positive integer",
            })?;

        if width == 0 {
            return Err(TypeError::InvalidBitWidth {
                raw: s.to_owned(),
                reason: "bit width must be non-zero",
            });
        }

        Ok(if signed {
            Type::Int(width)
        } else {
            Type::UInt(width)
        })
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int(w) => write!(f, "i{w}"),
            Type::UInt(w) => write!(f, "u{w}"),
            Type::USize => write!(f, "usize"),
            Type::ISize => write!(f, "isize"),
            Type::Float16 => write!(f, "f16"),
            Type::BFloat16 => write!(f, "bf16"),
            Type::Float32 => write!(f, "f32"),
            Type::Float64 => write!(f, "f64"),
            Type::Float128 => write!(f, "f128"),
            Type::Bool => write!(f, "bool"),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Return(Box<Expr>),
    Expr(Box<Expr>),

    /// Variable declaration: `<type> <name> = <expr>;`
    VarDecl(Binding),
    /// Assignment: `x = <value>`. Compound operators (`x += e`) are desugared
    /// by the parser into `x = x + e` before reaching this node.
    Assign {
        name: String,
        value: Box<Expr>,
    },
    If {
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

    // Variable reference
    Ident(String),

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

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
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
    /// Maps a compound-assignment token (`+=`, `-=`, …) to the corresponding
    /// [`BinOp`], returning `None` for plain `=`.
    pub fn from_assign_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Mod),
            TokenKind::PowEq => Some(BinOp::Pow),
            TokenKind::AndEq => Some(BinOp::BitwiseAnd),
            TokenKind::OrEq => Some(BinOp::BitwiseOr),
            TokenKind::XorEq => Some(BinOp::BitwiseXor),
            TokenKind::LShiftEq => Some(BinOp::LShift),
            TokenKind::RShiftEq => Some(BinOp::RShift),
            TokenKind::PlusPlus => Some(BinOp::Add),
            TokenKind::MinusMinus => Some(BinOp::Sub),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,        // -x
    Not,        // !x (logical NOT)
    BitwiseNot, // ~x
}

impl UnaryOp {
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Tilde => Some(UnaryOp::BitwiseNot),
            _ => None,
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitwiseNot => 23,
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
    pub return_type: Binding,
    pub body: Vec<Stmt>,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params == other.params
            && self.return_type == other.return_type
            && self.body == other.body
            && self.attributes == other.attributes
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
}
