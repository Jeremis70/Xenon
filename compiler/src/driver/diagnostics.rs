use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::driver::config::{ColorChoice, ErrorFormat};
use crate::driver::session::Session;
use crate::error::{FoldError, LexError, ParseError, SemanticError};
use crate::middle::mir::analysis::AnalysisErrors;
use crate::source::Span;

/// A single error to report.
struct Diagnostic {
    /// Machine-readable category, reported as `code` in JSON output.
    code: &'static str,
    /// Human-readable category, e.g. `parse error`.
    kind: &'static str,
    /// What went wrong.
    message: String,
    /// The source range to underline, if the error has one.
    span: Option<Span>,
    /// Label color in rendered reports.
    color: Color,
}

/// Reports diagnostics about one source text in the session's format.
pub struct Emitter<'a> {
    filename: &'a str,
    source: &'a str,
    format: ErrorFormat,
    color: ColorChoice,
}

impl<'a> Emitter<'a> {
    /// An emitter for errors located in `source`, named `filename`.
    pub fn new(session: &Session, filename: &'a str, source: &'a str) -> Self {
        Self {
            filename,
            source,
            format: session.error_format,
            color: session.color,
        }
    }

    /// An emitter for errors that are not located in any source.
    pub fn without_source(session: &Session) -> Self {
        Self::new(session, "", "")
    }

    pub fn lex_error(&self, err: &LexError) {
        self.emit(Diagnostic {
            code: "lex",
            kind: "lexing error",
            message: "invalid token".to_owned(),
            span: Some(err.span),
            color: Color::Red,
        });
    }

    pub fn parse_error(&self, err: &ParseError) {
        self.emit(Diagnostic {
            code: "parse",
            kind: "parse error",
            message: err.message.clone(),
            span: Some(err.span),
            color: Color::Red,
        });
    }

    pub fn fold_error(&self, err: &FoldError) {
        let span = match err {
            FoldError::DivisionByZero { span } => *span,
        };
        self.emit(Diagnostic {
            code: "fold",
            kind: "constant folding error",
            message: err.to_string(),
            span: Some(span),
            color: Color::Red,
        });
    }

    pub fn semantic_error(&self, err: &SemanticError) {
        self.emit(Diagnostic {
            code: "semantic",
            kind: "semantic error",
            message: err.to_string(),
            span: err.span(),
            color: Color::Yellow,
        });
    }

    /// Reports every MIR flow error, each at its own source location.
    pub fn analysis_errors(&self, errors: &AnalysisErrors) {
        for error in &errors.0 {
            self.emit(Diagnostic {
                code: "mir-analysis",
                kind: "flow error",
                message: format!("in `{}`: {}", error.function, error.kind),
                span: Some(error.span),
                color: Color::Red,
            });
        }
    }

    /// Reports a failure of the compiler itself (an unsupported request, a
    /// backend or I/O error) rather than of the program being compiled.
    pub fn internal_error(&self, message: &str, code: &'static str) {
        self.emit(Diagnostic {
            code,
            kind: "compiler error",
            message: message.to_owned(),
            span: None,
            color: Color::Red,
        });
    }

    fn emit(&self, diagnostic: Diagnostic) {
        match self.format {
            ErrorFormat::Human => self.emit_human(diagnostic),
            ErrorFormat::Json => emit_json(diagnostic),
        }
    }

    fn emit_human(&self, diagnostic: Diagnostic) {
        let Diagnostic {
            kind,
            message,
            span: Some(span),
            color,
            ..
        } = diagnostic
        else {
            eprintln!("{}: {}", diagnostic.kind, diagnostic.message);
            return;
        };
        let file = self.filename.to_owned();
        let range = span.start..span.end;
        let report = Report::build(ReportKind::Error, (file.clone(), range.clone()))
            .with_config(self.config())
            .with_message(format!("{kind}: {message}"))
            .with_label(
                Label::new((file.clone(), range))
                    .with_message(message)
                    .with_color(color),
            )
            .finish();
        if report.eprint((file, Source::from(self.source))).is_err() {
            eprintln!("error: (failed to render rich diagnostic)");
        }
    }

    /// Configures colour output based on the session's colour preference.
    fn config(&self) -> ariadne::Config {
        let config = ariadne::Config::default().with_char_set(ariadne::CharSet::Unicode);
        match self.color {
            ColorChoice::Always => config.with_color(true),
            ColorChoice::Never => config.with_color(false),
            ColorChoice::Auto => config,
        }
    }
}

#[derive(serde::Serialize)]
struct JsonDiagnostic {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<JsonSpan>,
    code: &'static str,
}

#[derive(serde::Serialize)]
struct JsonSpan {
    start: usize,
    end: usize,
}

/// Writes `diagnostic` as one JSON line on stderr.
fn emit_json(diagnostic: Diagnostic) {
    let json = JsonDiagnostic {
        kind: "error",
        message: diagnostic.message,
        span: diagnostic.span.map(|span| JsonSpan {
            start: span.start,
            end: span.end,
        }),
        code: diagnostic.code,
    };
    match serde_json::to_string(&json) {
        Ok(line) => eprintln!("{line}"),
        Err(e) => {
            eprintln!(r#"{{"type":"error","message":"failed to serialize diagnostic: {e}"}}"#)
        }
    }
}
