use crate::tokens::{Span, Token, tokenize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexError {
    pub span: Span,
}

type LexResult<T> = Result<T, LexError>;

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lexing error at span {:?}", self.span)
    }
}

impl std::error::Error for LexError {}

pub fn lex(input: &str) -> LexResult<Vec<Token>> {
    tokenize(input).map_err(|span| LexError { span })
}
