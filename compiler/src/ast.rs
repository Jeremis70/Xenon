use crate::error::TypeError;
use crate::tokens::TokenKind;
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
pub enum Stmt {
    Return(Box<Expr>),
    Expr(Box<Expr>),

    VarDecl {
        name: String,
        ty: Type,
        value: Box<Expr>,
    },
    /// Assignment: `x = <value>`. Compound operators (`x += e`) are desugared
    /// by the parser into `x = x + e` before reaching this node.
    Assign {
        name: String,
        value: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Literals
    Int(i64),

    // Variable reference
    Ident(String),

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
            _ => None,
        }
    }

    pub fn from_op_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Plus => Some(BinOp::Add),
            TokenKind::Minus => Some(BinOp::Sub),
            TokenKind::Star => Some(BinOp::Mul),
            TokenKind::Slash => Some(BinOp::Div),
            TokenKind::Percent => Some(BinOp::Mod),
            TokenKind::Pow => Some(BinOp::Pow),
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::NotEq => Some(BinOp::NotEq),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::LtEq => Some(BinOp::LtEq),
            TokenKind::GtEq => Some(BinOp::GtEq),
            TokenKind::And => Some(BinOp::BitwiseAnd),
            TokenKind::Or => Some(BinOp::BitwiseOr),
            TokenKind::Xor => Some(BinOp::BitwiseXor),
            TokenKind::AndAnd => Some(BinOp::LogicalAnd),
            TokenKind::OrOr => Some(BinOp::LogicalOr),
            TokenKind::XorXor => Some(BinOp::LogicalXor),
            TokenKind::LShift => Some(BinOp::LShift),
            TokenKind::RShift => Some(BinOp::RShift),
            _ => None,
        }
    }

    // right_precedence > left_precedence = left-associative
    // left_precedence > right_precedence = right-associative
    pub fn precedence(&self) -> (u8, u8) {
        match self {
            BinOp::LogicalOr => (1, 2),
            BinOp::LogicalXor => (3, 4),
            BinOp::LogicalAnd => (5, 6),
            BinOp::Eq | BinOp::NotEq => (7, 8),
            BinOp::BitwiseOr => (9, 10),
            BinOp::BitwiseXor => (11, 12),
            BinOp::BitwiseAnd => (13, 14),
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => (15, 16),
            BinOp::LShift | BinOp::RShift => (17, 18),
            BinOp::Add | BinOp::Sub => (19, 20),
            BinOp::Mul | BinOp::Div | BinOp::Mod => (21, 22),
            BinOp::Pow => (24, 23), // right-associative
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

/// A single function parameter: `<type> <name>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
