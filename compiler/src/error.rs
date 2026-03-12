use crate::tokens::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("lexing error at {span:?}")]
pub struct LexError {
    pub span: Span,
}

pub type LexResult<T> = Result<T, LexError>;

#[derive(Debug, thiserror::Error)]
#[error("{message} (span {}..{})", span.start, span.end)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

pub type ParseResult<T> = Result<T, ParseError>;

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("unknown type: `{0}`")]
    Unknown(String),
    #[error("invalid bit width in `{raw}`: {reason}")]
    InvalidBitWidth { raw: String, reason: &'static str },
}
