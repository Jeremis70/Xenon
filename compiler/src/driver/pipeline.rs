use std::fmt;
use std::path::PathBuf;

#[cfg(feature = "llvm-backend")]
use crate::backend::contract::Backend;
#[cfg(feature = "llvm-backend")]
use crate::backend::contract::CodegenOptions;
use crate::backend::contract::{ArtifactKind, OutputRequest};
#[cfg(feature = "llvm-backend")]
use crate::backend::llvm::LlvmBackend;
use crate::driver::config::{CheckEmitKind, CompileEmitKind, OptLevel, StopAfter};
use crate::driver::diagnostics;
use crate::driver::session::Session;
use crate::error::SemanticError;
use crate::frontend::lexer::lex;
use crate::frontend::parser::Parser;
use crate::frontend::tokens::Token;

use crate::backend::link::link_executable;
use crate::frontend::ast::Program;
use crate::middle::constant_fold::fold_constants;
use crate::middle::mir::analysis::{
    AnalysisErrors, OverflowMode, RuntimeCheckPolicy, analyze_program, normalize_runtime_checks,
};
use crate::middle::mir::build::build_mir;
use crate::middle::mir::pretty::mir_program_to_string;
use crate::middle::target::TargetSpec;
use crate::middle::typecheck::{check_program, validate_entry_point};

/// Parsed program plus combined source and primary path for diagnostics.
pub struct ParsedSource {
    pub program: Program,
    pub combined_source: String,
    pub first_path: String,
}

/// Lex, parse, and fold source-level constants. Semantic checks are performed
/// by the typed MIR pipeline rather than the legacy AST validator.
pub fn parse_source(session: &Session) -> Option<ParsedSource> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut combined_source = String::new();
    let first_path = session
        .source
        .first()
        .map(|s| s.path.display().to_string())
        .unwrap_or_else(|| "<unknown>".into());

    for source in &session.source {
        if session.verbose {
            println!("Compiling source file: {:?}", source.path);
        }
        let source_tokens = match lex(&source.content) {
            Ok(tokens) => tokens,
            Err(err) => {
                diagnostics::emit_lex_error(
                    &err,
                    &source.path.display().to_string(),
                    &source.content,
                    session.error_format,
                    session.color,
                );
                return None;
            }
        };
        combined_source.push_str(&source.content);
        tokens.extend(source_tokens);
    }

    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.compile_emit);
    }

    let mut parser = Parser::new(&tokens);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            diagnostics::emit_parse_error(
                &e,
                &first_path,
                &combined_source,
                session.error_format,
                session.color,
            );
            return None;
        }
    };

    let program = match fold_constants(program) {
        Ok(p) => p,
        Err(e) => {
            diagnostics::emit_fold_error(
                &e,
                &first_path,
                &combined_source,
                session.error_format,
                session.color,
            );
            return None;
        }
    };

    if let Err(error) = validate_entry_point(&program) {
        diagnostics::emit_semantic_error(
            &error,
            &first_path,
            &combined_source,
            session.error_format,
            session.color,
        );
        return None;
    }

    Some(ParsedSource {
        program,
        combined_source,
        first_path,
    })
}

#[derive(Debug)]
enum MirPipelineError {
    Semantic(SemanticError),
    Analysis(AnalysisErrors),
    Internal(String),
}

impl fmt::Display for MirPipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Semantic(error) => write!(f, "{error}"),
            Self::Analysis(error) => write!(f, "{error}"),
            Self::Internal(error) => f.write_str(error),
        }
    }
}

fn emit_mir_pipeline_error(session: &Session, parsed: &ParsedSource, error: &MirPipelineError) {
    match error {
        MirPipelineError::Semantic(error) => diagnostics::emit_semantic_error(
            error,
            &parsed.first_path,
            &parsed.combined_source,
            session.error_format,
            session.color,
        ),
        MirPipelineError::Analysis(error) => diagnostics::emit_mir_analysis_error(
            error,
            &parsed.first_path,
            &parsed.combined_source,
            session.error_format,
            session.color,
        ),
        MirPipelineError::Internal(error) => {
            diagnostics::emit_internal_error(error, "compiler", session.error_format, session.color)
        }
    }
}

fn build_runtime_mir(
    session: &Session,
    parsed: &ParsedSource,
) -> Result<crate::middle::mir::MirProgram, MirPipelineError> {
    if session.target.is_some() {
        return Err(MirPipelineError::Internal(
            "custom compilation targets are not supported yet".to_owned(),
        ));
    }
    let target = TargetSpec::host();
    let thir = check_program(&parsed.program, &target).map_err(MirPipelineError::Semantic)?;
    let mut mir = build_mir(&thir)
        .map_err(|error| MirPipelineError::Internal(format!("MIR lowering failed: {error}")))?;
    analyze_program(&mut mir, &target).map_err(MirPipelineError::Analysis)?;
    let overflow = if session.opt_level.is_none_or(|level| level == OptLevel::O0) {
        OverflowMode::Checked
    } else {
        OverflowMode::Wrapping
    };
    normalize_runtime_checks(&mut mir, &target, RuntimeCheckPolicy { overflow }).map_err(
        |error| MirPipelineError::Internal(format!("MIR runtime normalization failed: {error}")),
    )?;
    Ok(mir)
}

pub fn compile(session: &Session) -> i32 {
    if !matches!(session.stop_after, StopAfter::Mir | StopAfter::Link) {
        diagnostics::emit_internal_error(
            "requested compile stage is not implemented",
            "stage",
            session.error_format,
            session.color,
        );
        return 1;
    }
    let parsed = match parse_source(session) {
        Some(p) => p,
        None => return 1,
    };
    let mir = match build_runtime_mir(session, &parsed) {
        Ok(mir) => mir,
        Err(error) => {
            emit_mir_pipeline_error(session, &parsed, &error);
            return 1;
        }
    };

    let out_dir: PathBuf = session
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));

    let mut outputs = Vec::new();
    let wants_link = session.compile_emit.contains(&CompileEmitKind::Link);
    let wants_object = wants_link || session.compile_emit.contains(&CompileEmitKind::Obj);
    let wants_ir = wants_link || session.compile_emit.contains(&CompileEmitKind::Ir);
    let wants_mir = session.compile_emit.contains(&CompileEmitKind::Mir);
    if session.stop_after == StopAfter::Mir
        && session
            .compile_emit
            .iter()
            .any(|kind| *kind != CompileEmitKind::Mir)
    {
        diagnostics::emit_internal_error(
            "compile outputs beyond MIR conflict with `--stage mir`",
            "stage",
            session.error_format,
            session.color,
        );
        return 1;
    }
    let single_artifact = [wants_object, wants_ir, wants_mir]
        .into_iter()
        .filter(|requested| *requested)
        .count()
        == 1;
    if session.output.is_some() && !wants_link && !single_artifact {
        diagnostics::emit_internal_error(
            "`--output` with multiple non-link artifacts is not supported",
            "output",
            session.error_format,
            session.color,
        );
        return 1;
    }
    if session.compile_emit.iter().any(|kind| {
        !matches!(
            kind,
            CompileEmitKind::Link
                | CompileEmitKind::Obj
                | CompileEmitKind::Ir
                | CompileEmitKind::Mir
        )
    }) {
        diagnostics::emit_internal_error(
            "requested compile output kind is not implemented",
            "output",
            session.error_format,
            session.color,
        );
        return 1;
    }

    let explicit_output = session.output.as_ref();
    let obj_path = if wants_object && !wants_link && single_artifact {
        explicit_output
            .cloned()
            .unwrap_or_else(|| out_dir.join("out.o"))
    } else {
        out_dir.join("out.o")
    };
    let ll_path = if wants_ir && !wants_link && single_artifact {
        explicit_output
            .cloned()
            .unwrap_or_else(|| out_dir.join("out.ll"))
    } else {
        out_dir.join("out.ll")
    };
    let mir_path = if wants_mir && single_artifact {
        explicit_output
            .cloned()
            .unwrap_or_else(|| out_dir.join("out.mir"))
    } else {
        out_dir.join("out.mir")
    };
    let exe_path = session
        .output
        .clone()
        .unwrap_or_else(|| out_dir.join("out"));
    if wants_object {
        outputs.push(OutputRequest {
            kind: ArtifactKind::Object,
            path: obj_path.clone(),
        });
    }
    if wants_ir {
        outputs.push(OutputRequest {
            kind: ArtifactKind::LlvmIr,
            path: ll_path.clone(),
        });
    }
    if wants_mir && let Err(error) = std::fs::write(&mir_path, mir_program_to_string(&mir)) {
        diagnostics::emit_internal_error(
            &format!("failed to write MIR output: {error}"),
            "output",
            session.error_format,
            session.color,
        );
        return 1;
    }
    if !outputs.is_empty() {
        #[cfg(feature = "llvm-backend")]
        {
            let optimization = optimization_level(session.opt_level);
            if let Err(error) = LlvmBackend.emit(
                &mir,
                &TargetSpec::host(),
                CodegenOptions { optimization },
                &outputs,
            ) {
                diagnostics::emit_internal_error(
                    &format!("{} backend error: {error}", LlvmBackend.name()),
                    "backend",
                    session.error_format,
                    session.color,
                );
                return 1;
            }
        }
        #[cfg(not(feature = "llvm-backend"))]
        {
            diagnostics::emit_internal_error(
                "native code generation is unavailable: build xenonc with the `llvm-backend` feature",
                "backend",
                session.error_format,
                session.color,
            );
            return 1;
        }
    }
    if wants_link && let Err(e) = link_executable(&obj_path, &exe_path) {
        diagnostics::emit_internal_error(
            &format!("link error: {e}"),
            "link",
            session.error_format,
            session.color,
        );
        return 1;
    }

    if !session.quiet {
        if wants_ir {
            println!("Wrote: {:?}", ll_path);
        }
        if wants_object {
            println!("Wrote: {:?}", obj_path);
        }
        if wants_mir {
            println!("Wrote: {:?}", mir_path);
        }
        if wants_link {
            println!("Wrote: {:?}", exe_path);
        }
    }

    0
}

pub fn check(session: &Session) -> i32 {
    if session.stop_after != StopAfter::Mir {
        diagnostics::emit_internal_error(
            "only `check --stage mir` is currently implemented",
            "stage",
            session.error_format,
            session.color,
        );
        return 1;
    }
    let Some(parsed) = parse_source(session) else {
        return 1;
    };
    let mir = match build_runtime_mir(session, &parsed) {
        Ok(mir) => mir,
        Err(error) => {
            emit_mir_pipeline_error(session, &parsed, &error);
            return 1;
        }
    };

    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.check_emit);
    }

    if session.check_emit.contains(&CheckEmitKind::Mir) {
        print!("{}", mir_program_to_string(&mir));
    }
    if session
        .check_emit
        .iter()
        .any(|kind| !matches!(kind, CheckEmitKind::Mir | CheckEmitKind::Metadata))
    {
        diagnostics::emit_internal_error(
            "requested check output kind is not implemented",
            "output",
            session.error_format,
            session.color,
        );
        return 1;
    }

    0
}

#[cfg(feature = "llvm-backend")]
fn optimization_level(level: Option<OptLevel>) -> u8 {
    match level {
        None | Some(OptLevel::O0) => 0,
        Some(OptLevel::O1) => 1,
        Some(OptLevel::O2) => 2,
        Some(OptLevel::O3 | OptLevel::Os | OptLevel::Oz) => 3,
    }
}
