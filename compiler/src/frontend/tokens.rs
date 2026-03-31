use crate::error::{ParseError, ParseResult};
use logos::Logos;
use num_bigint::BigInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// A zero-width span at position 0, used as a placeholder when no source
    /// location is available (e.g. synthetic AST nodes in tests).
    pub const ZERO: Span = Span { start: 0, end: 0 };
}

impl TokenKind {
    /// Returns `true` for all assignment operator tokens (`=`, `+=`, `-=`, …).
    pub fn is_assign_op(self) -> bool {
        matches!(
            self,
            TokenKind::Eq
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
                | TokenKind::PowEq
                | TokenKind::AndEq
                | TokenKind::OrEq
                | TokenKind::XorEq
                | TokenKind::LShiftEq
                | TokenKind::RShiftEq
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
        )
    }
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
    Until,
    Loop,
    For,
    Break,
    Continue,
    Do,
    True,
    False,
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
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    AndEq,
    OrEq,
    XorEq,
    LShiftEq,
    RShiftEq,
    PowEq,
    PercentEq,
    PlusPlus,
    MinusMinus,

    Arrow,
    EqEq,
    NotEq,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    XorXor,
    LShift,
    RShift,
    Pow,
    // Single-char operators
    Eq,
    Lt,
    Gt,
    Plus,
    Minus,
    Star,
    Slash,
    Bang,
    Tilde,
    And,
    Or,
    Xor,
    Percent,
    // Value-bearing
    Ident,
    Int,
    Float,
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
    Int(BigInt),
    Float(f64),
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

    pub fn int_value(&self) -> ParseResult<&BigInt> {
        match &self.value {
            Some(TokenValue::Int(v)) => Ok(v),
            _ => Err(ParseError::new("expected integer value", self.span)),
        }
    }

    pub fn float_value(&self) -> ParseResult<f64> {
        match &self.value {
            Some(TokenValue::Float(v)) => Ok(*v),
            _ => Err(ParseError::new("expected float value", self.span)),
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
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("loop")]
    Loop,
    #[token("while")]
    While,
    #[token("until")]
    Until,
    #[token("for")]
    For,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("do")]
    Do,

    // ---------- Float literals (before plain integer regex) ----------
    #[regex(r"(?:[0-9]+\.[0-9]*|[0-9]*\.[0-9]+)(?:[eE][+-]?[0-9]+)?", parse_float)]
    FloatLit(f64),

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
    #[token("<<=")]
    LShiftEq,
    #[token(">>=")]
    RShiftEq,
    #[token("**=")]
    PowEq,
    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("&=")]
    AndEq,
    #[token("|=")]
    OrEq,
    #[token("^=")]
    XorEq,
    #[token("%=")]
    PercentEq,

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
    #[token("^^")]
    XorXor,
    #[token("<<")]
    LShift,
    #[token(">>")]
    RShift,
    #[token("**")]
    Pow,
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
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    #[token("~")]
    Tilde,
    #[token("&")]
    And,
    #[token("|")]
    Or,
    #[token("^")]
    Xor,

    // ---------- Literals ----------
    // Integer: decimal, hex (0x), binary (0b), octal (0o)
    #[regex(r"0[xX][0-9a-fA-F]+|0[bB][01]+|0[oO][0-7]+|[0-9]+", parse_int)]
    Int(BigInt),

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

fn parse_float(lex: &mut logos::Lexer<RawKind>) -> Option<f64> {
    lex.slice().parse().ok()
}

fn parse_int(lex: &mut logos::Lexer<RawKind>) -> Option<BigInt> {
    let s = lex.slice();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        BigInt::parse_bytes(hex.as_bytes(), 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        BigInt::parse_bytes(bin.as_bytes(), 2)
    } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        BigInt::parse_bytes(oct.as_bytes(), 8)
    } else {
        BigInt::parse_bytes(s.as_bytes(), 10)
    }
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
        RawKind::Until => (TokenKind::Until, None),
        RawKind::Loop => (TokenKind::Loop, None),
        RawKind::For => (TokenKind::For, None),
        RawKind::Break => (TokenKind::Break, None),
        RawKind::Continue => (TokenKind::Continue, None),
        RawKind::Do => (TokenKind::Do, None),
        RawKind::True => (TokenKind::True, None),
        RawKind::False => (TokenKind::False, None),
        RawKind::FloatLit(f) => (TokenKind::Float, Some(TokenValue::Float(f))),
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
        RawKind::PlusEq => (TokenKind::PlusEq, None),
        RawKind::MinusEq => (TokenKind::MinusEq, None),
        RawKind::StarEq => (TokenKind::StarEq, None),
        RawKind::SlashEq => (TokenKind::SlashEq, None),
        RawKind::AndEq => (TokenKind::AndEq, None),
        RawKind::OrEq => (TokenKind::OrEq, None),
        RawKind::XorEq => (TokenKind::XorEq, None),
        RawKind::LShiftEq => (TokenKind::LShiftEq, None),
        RawKind::RShiftEq => (TokenKind::RShiftEq, None),
        RawKind::PowEq => (TokenKind::PowEq, None),
        RawKind::PercentEq => (TokenKind::PercentEq, None),

        RawKind::PlusPlus => (TokenKind::PlusPlus, None),
        RawKind::MinusMinus => (TokenKind::MinusMinus, None),
        RawKind::Arrow => (TokenKind::Arrow, None),
        RawKind::EqEq => (TokenKind::EqEq, None),
        RawKind::NotEq => (TokenKind::NotEq, None),
        RawKind::LtEq => (TokenKind::LtEq, None),
        RawKind::GtEq => (TokenKind::GtEq, None),
        RawKind::AndAnd => (TokenKind::AndAnd, None),
        RawKind::OrOr => (TokenKind::OrOr, None),
        RawKind::XorXor => (TokenKind::XorXor, None),
        RawKind::LShift => (TokenKind::LShift, None),
        RawKind::RShift => (TokenKind::RShift, None),
        RawKind::Pow => (TokenKind::Pow, None),
        RawKind::Eq => (TokenKind::Eq, None),
        RawKind::Lt => (TokenKind::Lt, None),
        RawKind::Gt => (TokenKind::Gt, None),
        RawKind::Plus => (TokenKind::Plus, None),
        RawKind::Minus => (TokenKind::Minus, None),
        RawKind::Star => (TokenKind::Star, None),
        RawKind::Slash => (TokenKind::Slash, None),
        RawKind::Bang => (TokenKind::Bang, None),
        RawKind::Tilde => (TokenKind::Tilde, None),
        RawKind::And => (TokenKind::And, None),
        RawKind::Or => (TokenKind::Or, None),
        RawKind::Xor => (TokenKind::Xor, None),
        RawKind::Percent => (TokenKind::Percent, None),
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
