use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::driver::config::{ColorChoice, ErrorFormat};
use crate::error::{CodegenError, FoldError, LexError, ParseError, SemanticError};
use crate::frontend::tokens::Span;

/// Configures colour output based on the session's colour preference.
fn color_config(color: ColorChoice) -> ariadne::Config {
    let charset = ariadne::CharSet::Unicode;
    match color {
        ColorChoice::Always => ariadne::Config::default()
            .with_char_set(charset)
            .with_color(true),
        ColorChoice::Never => ariadne::Config::default()
            .with_char_set(charset)
            .with_color(false),
        ColorChoice::Auto => ariadne::Config::default().with_char_set(charset),
    }
}

fn span_range(span: Span) -> std::ops::Range<usize> {
    span.start..span.end
}

pub fn emit_lex_error(
    err: &LexError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    match format {
        ErrorFormat::Human => {
            Report::build(ReportKind::Error, (filename, span_range(err.span)))
                .with_config(color_config(color))
                .with_message("invalid token")
                .with_label(
                    Label::new((filename, span_range(err.span)))
                        .with_message("unexpected character(s)")
                        .with_color(Color::Red),
                )
                .finish()
                .eprint((filename, Source::from(src)))
                .unwrap();
        }
        ErrorFormat::Json => {
            eprintln!(
                r#"{{"type":"error","message":"lexing error","span":{{"start":{},"end":{}}}}}"#,
                err.span.start, err.span.end,
            );
        }
    }
}

pub fn emit_parse_error(
    err: &ParseError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    match format {
        ErrorFormat::Human => {
            Report::build(ReportKind::Error, (filename, span_range(err.span)))
                .with_config(color_config(color))
                .with_message(format!("parse error: {}", err.message))
                .with_label(
                    Label::new((filename, span_range(err.span)))
                        .with_message(&err.message)
                        .with_color(Color::Red),
                )
                .finish()
                .eprint((filename, Source::from(src)))
                .unwrap();
        }
        ErrorFormat::Json => {
            eprintln!(
                r#"{{"type":"error","message":"{}","span":{{"start":{},"end":{}}}}}"#,
                err.message, err.span.start, err.span.end,
            );
        }
    }
}

pub fn emit_semantic_error(
    err: &SemanticError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    let span = match err {
        SemanticError::ConstantOutOfRange { span, .. } => *span,
    };
    match format {
        ErrorFormat::Human => {
            Report::build(ReportKind::Error, (filename, span_range(span)))
                .with_config(color_config(color))
                .with_message(format!("semantic error: {err}"))
                .with_label(
                    Label::new((filename, span_range(span)))
                        .with_message(err.to_string())
                        .with_color(Color::Yellow),
                )
                .finish()
                .eprint((filename, Source::from(src)))
                .unwrap();
        }
        ErrorFormat::Json => {
            eprintln!(
                r#"{{"type":"error","message":"{}","span":{{"start":{},"end":{}}}}}"#,
                err, span.start, span.end,
            );
        }
    }
}

pub fn emit_fold_error(
    err: &FoldError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    let span = match err {
        FoldError::DivisionByZero { span } => *span,
    };
    match format {
        ErrorFormat::Human => {
            Report::build(ReportKind::Error, (filename, span_range(span)))
                .with_config(color_config(color))
                .with_message(format!("constant folding error: {err}"))
                .with_label(
                    Label::new((filename, span_range(span)))
                        .with_message(err.to_string())
                        .with_color(Color::Red),
                )
                .finish()
                .eprint((filename, Source::from(src)))
                .unwrap();
        }
        ErrorFormat::Json => {
            eprintln!(
                r#"{{"type":"error","message":"{}","span":{{"start":{},"end":{}}}}}"#,
                err, span.start, span.end,
            );
        }
    }
}

pub fn emit_codegen_error(
    err: &CodegenError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    // Extract span from error variants that carry one.
    let span = match err {
        CodegenError::UnsupportedType { span, .. }
        | CodegenError::UnsupportedOperator { span, .. }
        | CodegenError::UndefinedVariable { span, .. }
        | CodegenError::UndefinedFunction { span, .. }
        | CodegenError::ArgumentCountMismatch { span, .. }
        | CodegenError::MissingReturn { span, .. }
        | CodegenError::DivisionByZero { span }
        | CodegenError::ShiftOverflow { span }
        | CodegenError::IntegerOverflow { span } => Some(*span),
        _ => None,
    };
    match format {
        ErrorFormat::Human => {
            if let Some(span) = span {
                Report::build(ReportKind::Error, (filename, span_range(span)))
                    .with_config(color_config(color))
                    .with_message(format!("codegen error: {err}"))
                    .with_label(
                        Label::new((filename, span_range(span)))
                            .with_message(err.to_string())
                            .with_color(Color::Red),
                    )
                    .finish()
                    .eprint((filename, Source::from(src)))
                    .unwrap();
            } else {
                eprintln!("error: {err}");
            }
        }
        ErrorFormat::Json => {
            if let Some(span) = span {
                eprintln!(
                    r#"{{"type":"error","message":"{}","span":{{"start":{},"end":{}}}}}"#,
                    err, span.start, span.end,
                );
            } else {
                eprintln!(r#"{{"type":"error","message":"{}"}}"#, err);
            }
        }
    }
}
