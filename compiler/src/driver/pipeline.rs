use std::path::PathBuf;

use crate::backend::contract::{ArtifactKind, OutputRequest};
use crate::backend::link::link_executable;
use crate::driver::config::{CheckEmitKind, CompileEmitKind, OptLevel, StopAfter};
use crate::driver::diagnostics::Emitter;
use crate::driver::session::Session;
use crate::frontend::ast::Program;
use crate::frontend::lexer::lex;
use crate::frontend::parser::Parser;
use crate::middle::constant_fold::fold_constants;
use crate::middle::mir::MirProgram;
use crate::middle::mir::analysis::{
    OverflowMode, RuntimeCheckPolicy, analyze_program, normalize_runtime_checks,
};
use crate::middle::mir::build::build_mir;
use crate::middle::mir::pretty::mir_program_to_string;
use crate::middle::target::TargetSpec;
use crate::middle::typecheck::{check_program, validate_entry_point};

/// Why a command failed.
enum Failure {
    /// The error has already been reported.
    Reported,
    /// A failure of the compiler itself, still to be reported.
    Internal { code: &'static str, message: String },
}

fn internal(code: &'static str, message: impl Into<String>) -> Failure {
    Failure::Internal {
        code,
        message: message.into(),
    }
}

/// Reports `result` and turns it into a process exit code.
fn exit_code(session: &Session, result: Result<(), Failure>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(Failure::Reported) => 1,
        Err(Failure::Internal { code, message }) => {
            Emitter::without_source(session).internal_error(&message, code);
            1
        }
    }
}

pub fn compile(session: &Session) -> i32 {
    exit_code(session, run_compile(session))
}

pub fn check(session: &Session) -> i32 {
    exit_code(session, run_check(session))
}

fn run_compile(session: &Session) -> Result<(), Failure> {
    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.compile_emit);
    }
    let emits = |kind| session.compile_emit.contains(&kind);
    let wants_link = emits(CompileEmitKind::Link);
    let wants_object = wants_link || emits(CompileEmitKind::Obj);
    let wants_ir = wants_link || emits(CompileEmitKind::Ir);
    let wants_mir = emits(CompileEmitKind::Mir);
    let single_artifact = [wants_object, wants_ir, wants_mir]
        .into_iter()
        .filter(|&requested| requested)
        .count()
        == 1;

    if !matches!(session.stop_after, StopAfter::Mir | StopAfter::Link) {
        return Err(internal(
            "stage",
            "requested compile stage is not implemented",
        ));
    }
    if session.stop_after == StopAfter::Mir
        && session
            .compile_emit
            .iter()
            .any(|kind| *kind != CompileEmitKind::Mir)
    {
        return Err(internal(
            "stage",
            "compile outputs beyond MIR conflict with `--stage mir`",
        ));
    }
    if session.output.is_some() && !wants_link && !single_artifact {
        return Err(internal(
            "output",
            "`--output` with multiple non-link artifacts is not supported",
        ));
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
        return Err(internal(
            "output",
            "requested compile output kind is not implemented",
        ));
    }

    let target = target_spec(session)?;
    let mir = lower_to_runtime_mir(session, &target)?;

    // `--output` names the executable, or else the only requested artifact.
    let out_dir = session
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let artifact_path = |requested: bool, file_name: &str| match &session.output {
        Some(output) if requested && !wants_link && single_artifact => output.clone(),
        _ => out_dir.join(file_name),
    };
    let obj_path = artifact_path(wants_object, "out.o");
    let ll_path = artifact_path(wants_ir, "out.ll");
    let mir_path = artifact_path(wants_mir, "out.mir");
    let exe_path = session
        .output
        .clone()
        .unwrap_or_else(|| out_dir.join("out"));

    if wants_mir {
        std::fs::write(&mir_path, mir_program_to_string(&mir))
            .map_err(|error| internal("output", format!("failed to write MIR output: {error}")))?;
    }
    let mut outputs = Vec::new();
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
    if !outputs.is_empty() {
        emit_native(session, &mir, &target, &outputs)?;
    }
    if wants_link {
        link_executable(&obj_path, &exe_path)
            .map_err(|error| internal("link", format!("link error: {error}")))?;
    }

    if !session.quiet {
        let written = [
            (wants_ir, &ll_path),
            (wants_object, &obj_path),
            (wants_mir, &mir_path),
            (wants_link, &exe_path),
        ];
        for (_, path) in written.iter().filter(|(requested, _)| *requested) {
            println!("Wrote: {path:?}");
        }
    }
    Ok(())
}

fn run_check(session: &Session) -> Result<(), Failure> {
    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.check_emit);
    }
    if session.stop_after != StopAfter::Mir {
        return Err(internal(
            "stage",
            "only `check --stage mir` is currently implemented",
        ));
    }
    if session
        .check_emit
        .iter()
        .any(|kind| !matches!(kind, CheckEmitKind::Mir | CheckEmitKind::Metadata))
    {
        return Err(internal(
            "output",
            "requested check output kind is not implemented",
        ));
    }

    let target = target_spec(session)?;
    let mir = lower_to_runtime_mir(session, &target)?;
    if session.check_emit.contains(&CheckEmitKind::Mir) {
        print!("{}", mir_program_to_string(&mir));
    }
    Ok(())
}

/// The compilation target. Only the host is supported so far.
fn target_spec(session: &Session) -> Result<TargetSpec, Failure> {
    match session.target {
        Some(_) => Err(internal(
            "target",
            "custom compilation targets are not supported yet",
        )),
        None => Ok(TargetSpec::host()),
    }
}

/// Parsed program plus combined source and primary path for diagnostics.
struct ParsedSource {
    program: Program,
    combined_source: String,
    first_path: String,
}

/// Lexes, parses, and folds source-level constants, and validates the entry
/// point.
fn parse_source(session: &Session) -> Result<ParsedSource, Failure> {
    let mut tokens = Vec::new();
    let mut combined_source = String::new();
    for source in &session.source {
        if session.verbose {
            println!("Compiling source file: {:?}", source.path);
        }
        let path = source.path.display().to_string();
        let source_tokens = lex(&source.content).map_err(|error| {
            Emitter::new(session, &path, &source.content).lex_error(&error);
            Failure::Reported
        })?;
        combined_source.push_str(&source.content);
        tokens.extend(source_tokens);
    }
    let first_path = session
        .source
        .first()
        .map_or_else(|| "<unknown>".into(), |s| s.path.display().to_string());

    let emitter = Emitter::new(session, &first_path, &combined_source);
    let program = Parser::new(&tokens).parse_program().map_err(|error| {
        emitter.parse_error(&error);
        Failure::Reported
    })?;
    let program = fold_constants(program).map_err(|error| {
        emitter.fold_error(&error);
        Failure::Reported
    })?;
    validate_entry_point(&program).map_err(|error| {
        emitter.semantic_error(&error);
        Failure::Reported
    })?;

    Ok(ParsedSource {
        program,
        combined_source,
        first_path,
    })
}

/// Runs the frontend and the MIR pipeline up to Runtime MIR, reporting any
/// error.
fn lower_to_runtime_mir(session: &Session, target: &TargetSpec) -> Result<MirProgram, Failure> {
    let parsed = parse_source(session)?;
    let emitter = Emitter::new(session, &parsed.first_path, &parsed.combined_source);

    let thir = check_program(&parsed.program, target).map_err(|error| {
        emitter.semantic_error(&error);
        Failure::Reported
    })?;
    let mut mir = build_mir(&thir)
        .map_err(|error| internal("compiler", format!("MIR lowering failed: {error}")))?;
    analyze_program(&mut mir, target).map_err(|errors| {
        emitter.analysis_errors(&errors);
        Failure::Reported
    })?;

    let overflow = match session.opt_level {
        None | Some(OptLevel::O0) => OverflowMode::Checked,
        Some(_) => OverflowMode::Wrapping,
    };
    normalize_runtime_checks(&mut mir, target, RuntimeCheckPolicy { overflow }).map_err(
        |error| {
            internal(
                "compiler",
                format!("MIR runtime normalization failed: {error}"),
            )
        },
    )?;
    Ok(mir)
}

#[cfg(feature = "llvm-backend")]
fn emit_native(
    session: &Session,
    mir: &MirProgram,
    target: &TargetSpec,
    outputs: &[OutputRequest],
) -> Result<(), Failure> {
    use crate::backend::contract::{Backend, CodegenOptions};
    use crate::backend::llvm::LlvmBackend;

    let optimization = match session.opt_level {
        None | Some(OptLevel::O0) => 0,
        Some(OptLevel::O1) => 1,
        Some(OptLevel::O2) => 2,
        Some(OptLevel::O3 | OptLevel::Os | OptLevel::Oz) => 3,
    };
    let backend = LlvmBackend;
    backend
        .emit(mir, target, CodegenOptions { optimization }, outputs)
        .map_err(|error| {
            internal(
                "backend",
                format!("{} backend error: {error}", backend.name()),
            )
        })?;
    Ok(())
}

#[cfg(not(feature = "llvm-backend"))]
fn emit_native(
    _session: &Session,
    _mir: &MirProgram,
    _target: &TargetSpec,
    _outputs: &[OutputRequest],
) -> Result<(), Failure> {
    Err(internal(
        "backend",
        "native code generation is unavailable: build xenonc with the `llvm-backend` feature",
    ))
}
