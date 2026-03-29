use crate::frontend::tokens::Span;

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
    #[error("undefined variable: `{name}` at {span:?}")]
    UndefinedVariable { name: String, span: Span },
    #[error("undefined function: `{name}` at {span:?}")]
    UndefinedFunction { name: String, span: Span },
    #[error("function `{name}` expects {expected} argument(s), got {got} at {span:?}")]
    ArgumentCountMismatch {
        name: String,
        expected: usize,
        got: usize,
        span: Span,
    },
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
    #[error("function `{name}` has no return statement and no named return variable at {span:?}")]
    MissingReturn { name: String, span: Span },
    #[error("division by zero at {span:?}")]
    DivisionByZero { span: Span },
    #[error("shift amount exceeds bit width at {span:?}")]
    ShiftOverflow { span: Span },
    #[error("integer overflow at {span:?}")]
    IntegerOverflow { span: Span },
    #[error("{0}")]
    Other(String),
}

pub type CodegenResult<T> = Result<T, CodegenError>;

#[derive(Debug, thiserror::Error)]
pub enum SemanticError {
    #[error("constant {value} is out of range for type `{ty}` in binding `{name}` (span {}..{})", span.start, span.end)]
    ConstantOutOfRange {
        name: String,
        value: num_bigint::BigInt,
        ty: crate::frontend::ast::Type,
        span: Span,
    },
}

pub type SemanticResult<T> = Result<T, SemanticError>;

#[derive(Debug, thiserror::Error)]
pub enum FoldError {
    #[error("division by zero in constant expression at {span:?}")]
    DivisionByZero { span: Span },
}

pub type FoldResult<T> = Result<T, FoldError>;
