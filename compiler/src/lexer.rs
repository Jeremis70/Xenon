use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")] // Ignore this regex pattern between tokens
pub enum Token {
    // Tokens can be literal strings, of any length.
    #[token("fast")]
    Fast,

    #[token(".")]
    Period,

    // Or regular expressions.
    #[regex("[a-zA-Z]+")]
    Text,
}

pub fn lex(input: &str) -> Vec<Token> {
    match Token::lexer(input).collect() {
        Ok(tokens) => tokens,
        Err(_) => {
            eprintln!("Lexing error");
            Vec::new()
        }
    }
}
