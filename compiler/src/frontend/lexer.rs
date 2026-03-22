use crate::error::{LexError, LexResult};
use crate::frontend::tokens::{Token, tokenize};

pub fn lex(input: &str) -> LexResult<Vec<Token>> {
    tokenize(input).map_err(|span| LexError { span })
}
