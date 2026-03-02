use logos::Logos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Self {
            start: r.start,
            end: r.end,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
pub enum TokenKind {
    // ---------- Keywords ----------
    #[token("fn")]
    Fn,
    #[token("return")]
    Return,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("for")]
    For,

    // ---------- Delimiters ----------
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token(":")]
    Colon,
    #[token(".")]
    Period,

    // ---------- Multi-char operators (put before single-char) ----------
    #[token("->")]
    Arrow,

    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,

    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,

    // ---------- Single-char operators ----------
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,

    #[token("!")]
    Bang,

    // ---------- Literals ----------
    // Integer (decimal only for MVP)
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<u64>().ok())]
    Int(u64),

    // String literal with basic escapes allowed (we keep the raw content for MVP).
    // If you want decoded escapes, do it in the callback.
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    Str(String),

    // ---------- Identifiers ----------
    // Ident: letter/_ then letters/digits/_
    // Put AFTER keywords so keywords match first.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // ---------- Comments ----------
    // Line comment: skip
    #[regex(r"//[^\n]*", logos::skip, allow_greedy = true)]
    LineComment,
}

fn parse_string(lex: &mut logos::Lexer<TokenKind>) -> Option<String> {
    let s = lex.slice();
    // s includes quotes. MVP: strip quotes, keep escapes as-is.
    // Example: "\"a\\n\"" -> "a\\n"
    if s.len() >= 2 {
        Some(s[1..s.len() - 1].to_string())
    } else {
        None
    }
}
