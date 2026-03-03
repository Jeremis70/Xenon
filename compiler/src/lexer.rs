use crate::tokens::{Span, Token, TokenKind};
use logos::Logos;

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
    let mut tokens = Vec::new();
    let mut lexer = TokenKind::lexer(input);

    while let Some(kind_res) = lexer.next() {
        match kind_res {
            Ok(kind) => {
                tokens.push(Token {
                    kind,
                    span: lexer.span().into(),
                });
            }
            Err(_) => {
                return Err(LexError {
                    span: lexer.span().into(),
                });
            }
        }
    }

    Ok(tokens)
}
