#[derive(Debug)]
pub enum Expr {
    Int(i64),
    Ident(String),
    Return(Box<Expr>),
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
