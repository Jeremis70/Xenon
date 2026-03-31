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

fn try_eprint_report(
    report: Report<'_, (String, std::ops::Range<usize>)>,
    file_id: String,
    src: &str,
) {
    if report.eprint((file_id, Source::from(src))).is_err() {
        eprintln!("error: (failed to render rich diagnostic)");
    }
}

#[derive(serde::Serialize)]
struct JsonDiagnostic<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<JsonSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct JsonSpan {
    start: usize,
    end: usize,
}

fn emit_json(diag: JsonDiagnostic<'_>) {
    match serde_json::to_string(&diag) {
        Ok(s) => eprintln!("{s}"),
        Err(e) => {
            eprintln!(r#"{{"type":"error","message":"failed to serialize diagnostic: {e}"}}"#)
        }
    }
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
            let fid = filename.to_string();
            let report = Report::build(ReportKind::Error, (fid.clone(), span_range(err.span)))
                .with_config(color_config(color))
                .with_message("invalid token")
                .with_label(
                    Label::new((fid.clone(), span_range(err.span)))
                        .with_message("unexpected character(s)")
                        .with_color(Color::Red),
                )
                .finish();
            try_eprint_report(report, fid, src);
        }
        ErrorFormat::Json => emit_json(JsonDiagnostic {
            kind: "error",
            message: "lexing error".to_owned(),
            span: Some(JsonSpan {
                start: err.span.start,
                end: err.span.end,
            }),
            code: Some("lex"),
        }),
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
            let fid = filename.to_string();
            let report = Report::build(ReportKind::Error, (fid.clone(), span_range(err.span)))
                .with_config(color_config(color))
                .with_message(format!("parse error: {}", err.message))
                .with_label(
                    Label::new((fid.clone(), span_range(err.span)))
                        .with_message(&err.message)
                        .with_color(Color::Red),
                )
                .finish();
            try_eprint_report(report, fid, src);
        }
        ErrorFormat::Json => emit_json(JsonDiagnostic {
            kind: "error",
            message: err.message.clone(),
            span: Some(JsonSpan {
                start: err.span.start,
                end: err.span.end,
            }),
            code: Some("parse"),
        }),
    }
}

pub fn emit_semantic_error(
    err: &SemanticError,
    filename: &str,
    src: &str,
    format: ErrorFormat,
    color: ColorChoice,
) {
    let span = err.span();
    match format {
        ErrorFormat::Human => {
            if let Some(span) = span {
                let fid = filename.to_string();
                let report = Report::build(ReportKind::Error, (fid.clone(), span_range(span)))
                    .with_config(color_config(color))
                    .with_message(format!("semantic error: {err}"))
                    .with_label(
                        Label::new((fid.clone(), span_range(span)))
                            .with_message(err.to_string())
                            .with_color(Color::Yellow),
                    )
                    .finish();
                try_eprint_report(report, fid, src);
            } else {
                eprintln!("semantic error: {err}");
            }
        }
        ErrorFormat::Json => emit_json(JsonDiagnostic {
            kind: "error",
            message: err.to_string(),
            span: span.map(|s| JsonSpan {
                start: s.start,
                end: s.end,
            }),
            code: Some("semantic"),
        }),
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
            let fid = filename.to_string();
            let report = Report::build(ReportKind::Error, (fid.clone(), span_range(span)))
                .with_config(color_config(color))
                .with_message(format!("constant folding error: {err}"))
                .with_label(
                    Label::new((fid.clone(), span_range(span)))
                        .with_message(err.to_string())
                        .with_color(Color::Red),
                )
                .finish();
            try_eprint_report(report, fid, src);
        }
        ErrorFormat::Json => emit_json(JsonDiagnostic {
            kind: "error",
            message: err.to_string(),
            span: Some(JsonSpan {
                start: span.start,
                end: span.end,
            }),
            code: Some("fold"),
        }),
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
                let fid = filename.to_string();
                let report = Report::build(ReportKind::Error, (fid.clone(), span_range(span)))
                    .with_config(color_config(color))
                    .with_message(format!("codegen error: {err}"))
                    .with_label(
                        Label::new((fid.clone(), span_range(span)))
                            .with_message(err.to_string())
                            .with_color(Color::Red),
                    )
                    .finish();
                try_eprint_report(report, fid, src);
            } else {
                eprintln!("error: {err}");
            }
        }
        ErrorFormat::Json => emit_json(JsonDiagnostic {
            kind: "error",
            message: err.to_string(),
            span: span.map(|s| JsonSpan {
                start: s.start,
                end: s.end,
            }),
            code: Some("codegen"),
        }),
    }
}
