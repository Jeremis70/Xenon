use crate::error::TypeError;
use crate::frontend::tokens::TokenKind;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int(u32),  // Any bit-width integer
    UInt(u32), // Any bit-width integer
    Float16,
    BFloat16,
    Float32,
    Float64,
    Float128,
    Bool,
}

impl FromStr for Type {
    type Err = TypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
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

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: Option<String>,
    pub ty: Type,
    pub default: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
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

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Literals
    Int(i64),

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
pub struct Function {
    pub name: String,
    pub params: Vec<Binding>,
    pub return_type: Binding,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
}
