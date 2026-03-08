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

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg, // -x
    Not, // !x
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
