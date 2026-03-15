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
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("unsupported type: `{ty}` at {span:?}")]
    UnsupportedType { ty: String, span: Span },
    #[error("unsupported operator: `{op}` at {span:?}")]
    UnsupportedOperator { op: String, span: Span },
    #[error("undefined variable: `{name}`")]
    UndefinedVariable { name: String },
    #[error("undefined function: `{name}`")]
    UndefinedFunction { name: String },
    /// An inkwell builder call returned an error.
    #[error("LLVM builder error in `{operation}`: {message}")]
    LlvmBuilder {
        operation: &'static str,
        message: String,
    },
    /// The IR is in an unexpected state (e.g. missing insert block).
    #[error("invalid IR state: {0}")]
    InvalidIrState(&'static str),
    #[error("target initialization failed: {0}")]
    TargetInit(String),
    #[error("target error: {0}")]
    TargetError(String),
    #[error("target machine creation failed")]
    TargetMachineCreation,
    #[error("output file error: {0}")]
    OutputFile(String),
    #[error("{0}")]
    Other(String),
}

pub type CodegenResult<T> = Result<T, CodegenError>;
