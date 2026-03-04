use crate::error::{ParseError, ParseResult};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Period,
    // Multi-char operators
    Arrow,
    EqEq,
    NotEq,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    // Single-char operators
    Eq,
    Lt,
    Gt,
    Plus,
    Minus,
    Star,
    Slash,
    Bang,
    // Value-bearing
    Ident,
    Int,
    Str,
}

impl AsRef<[TokenKind]> for TokenKind {
    fn as_ref(&self) -> &[TokenKind] {
        std::slice::from_ref(self)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenValue {
    Ident(String),
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub value: Option<TokenValue>,
    pub span: Span,
}

impl Token {
    pub fn ident_value(&self) -> ParseResult<&str> {
        match &self.value {
            Some(TokenValue::Ident(s)) => Ok(s.as_str()),
            _ => Err(ParseError::new("expected identifier value", self.span)),
        }
    }

    pub fn int_value(&self) -> ParseResult<i64> {
        match &self.value {
            Some(TokenValue::Int(v)) => Ok(*v),
            _ => Err(ParseError::new("expected integer value", self.span)),
        }
    }

    pub fn str_value(&self) -> ParseResult<&str> {
        match &self.value {
            Some(TokenValue::Str(s)) => Ok(s.as_str()),
            _ => Err(ParseError::new("expected string value", self.span)),
        }
    }
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
enum RawKind {
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
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

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

fn parse_string(lex: &mut logos::Lexer<RawKind>) -> Option<String> {
    let s = lex.slice();
    // s includes quotes. MVP: strip quotes, keep escapes as-is.
    // Example: "\"a\\n\"" -> "a\\n"
    if s.len() >= 2 {
        Some(s[1..s.len() - 1].to_string())
    } else {
        None
    }
}

fn into_token_parts(raw: RawKind) -> (TokenKind, Option<TokenValue>) {
    match raw {
        RawKind::Fn => (TokenKind::Fn, None),
        RawKind::Return => (TokenKind::Return, None),
        RawKind::If => (TokenKind::If, None),
        RawKind::Else => (TokenKind::Else, None),
        RawKind::While => (TokenKind::While, None),
        RawKind::For => (TokenKind::For, None),
        RawKind::LParen => (TokenKind::LParen, None),
        RawKind::RParen => (TokenKind::RParen, None),
        RawKind::LBrace => (TokenKind::LBrace, None),
        RawKind::RBrace => (TokenKind::RBrace, None),
        RawKind::LBracket => (TokenKind::LBracket, None),
        RawKind::RBracket => (TokenKind::RBracket, None),
        RawKind::Comma => (TokenKind::Comma, None),
        RawKind::Semicolon => (TokenKind::Semicolon, None),
        RawKind::Colon => (TokenKind::Colon, None),
        RawKind::Period => (TokenKind::Period, None),
        RawKind::Arrow => (TokenKind::Arrow, None),
        RawKind::EqEq => (TokenKind::EqEq, None),
        RawKind::NotEq => (TokenKind::NotEq, None),
        RawKind::LtEq => (TokenKind::LtEq, None),
        RawKind::GtEq => (TokenKind::GtEq, None),
        RawKind::AndAnd => (TokenKind::AndAnd, None),
        RawKind::OrOr => (TokenKind::OrOr, None),
        RawKind::Eq => (TokenKind::Eq, None),
        RawKind::Lt => (TokenKind::Lt, None),
        RawKind::Gt => (TokenKind::Gt, None),
        RawKind::Plus => (TokenKind::Plus, None),
        RawKind::Minus => (TokenKind::Minus, None),
        RawKind::Star => (TokenKind::Star, None),
        RawKind::Slash => (TokenKind::Slash, None),
        RawKind::Bang => (TokenKind::Bang, None),
        RawKind::Ident(s) => (TokenKind::Ident, Some(TokenValue::Ident(s))),
        RawKind::Int(v) => (TokenKind::Int, Some(TokenValue::Int(v))),
        RawKind::Str(s) => (TokenKind::Str, Some(TokenValue::Str(s))),
        RawKind::LineComment => unreachable!("line comments are skipped by Logos"),
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, Span> {
    let mut tokens = Vec::new();
    let mut lexer = RawKind::lexer(input);

    while let Some(raw_res) = lexer.next() {
        match raw_res {
            Ok(raw) => {
                let (kind, value) = into_token_parts(raw);
                tokens.push(Token {
                    kind,
                    value,
                    span: lexer.span().into(),
                });
            }
            Err(_) => return Err(lexer.span().into()),
        }
    }

    Ok(tokens)
}
