use crate::tokens::TokenKind;

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

    // Statement-level wrapper
    Return(Box<Expr>),
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
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
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

    // (left_bp, right_bp) — right > left means left-associative
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

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub return_type: String,
    pub body: Vec<Expr>,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
