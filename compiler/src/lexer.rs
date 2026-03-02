use crate::tokens::{Token, TokenKind};
use logos::Logos;

pub fn lex(input: &str) -> Vec<Token> {
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
                eprintln!("Lexing error at span {:?}", lexer.span());
            }
        }
    }

    tokens
}
