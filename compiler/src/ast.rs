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
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

impl BinOp {
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Plus => Some(BinOp::Add),
            TokenKind::Minus => Some(BinOp::Sub),
            TokenKind::Star => Some(BinOp::Mul),
            TokenKind::Slash => Some(BinOp::Div),
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::NotEq => Some(BinOp::NotEq),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::AndAnd => Some(BinOp::And),
            TokenKind::OrOr => Some(BinOp::Or),
            TokenKind::LtEq => Some(BinOp::LtEq),
            TokenKind::GtEq => Some(BinOp::GtEq),
            TokenKind::Percent => Some(BinOp::Mod),
            _ => None,
        }
    }

    // (left_bp, right_bp) — right > left means left-associative
    pub fn precedence(&self) -> (u8, u8) {
        match self {
            BinOp::Or => (1, 2),
            BinOp::And => (3, 4),
            BinOp::Eq | BinOp::NotEq => (5, 6),
            BinOp::Lt | BinOp::Gt => (7, 8),
            BinOp::LtEq | BinOp::GtEq => (7, 8),
            BinOp::Add | BinOp::Sub => (10, 11),
            BinOp::Mul | BinOp::Div => (20, 21),
            BinOp::Mod => (20, 21),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg, // -x
    Not, // !x
}

impl UnaryOp {
    pub fn from_token(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            UnaryOp::Neg | UnaryOp::Not => 30,
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
