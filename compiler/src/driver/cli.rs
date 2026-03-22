use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Xenon compiler CLI.
#[derive(Parser, Debug)]
#[command(
    name = "xenonc",
    version,
    about = "Xenon compiler",
    long_about = "A compiler for the Xenon programming language",
    arg_required_else_help = true,
    disable_help_subcommand = true,
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Compilation-only subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Compile and (optionally) link source files.
    Compile(CompileArgs),
    /// Run frontend/midend checks without full codegen+link.
    Check(CheckArgs),
}

/// Full compilation pipeline entrypoint.
#[derive(Args, Debug)]
pub struct CompileArgs {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub session: SessionArgs,

    #[command(flatten)]
    pub pipeline: CompilePipelineArgs,

    #[command(flatten)]
    pub output: CompileOutputArgs,

    #[command(flatten)]
    pub codegen: CodegenArgs,

    #[command(flatten)]
    pub linker: LinkerArgs,

    #[command(flatten)]
    pub diagnostics: DiagnosticsArgs,

    #[command(flatten)]
    pub internal: InternalArgs,
}

/// Frontend/midend checking entrypoint.
#[derive(Args, Debug)]
pub struct CheckArgs {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub session: SessionArgs,

    #[command(flatten)]
    pub pipeline: CheckPipelineArgs,

    #[command(flatten)]
    pub output: CheckOutputArgs,

    #[command(flatten)]
    pub diagnostics: DiagnosticsArgs,

    #[command(flatten)]
    pub internal: InternalArgs,
}

/// Input files and crate identity.
#[derive(Args, Debug)]
pub struct InputArgs {
    /// Source files to compile/check.
    #[arg(required = true, num_args = 1.., value_name = "FILE")]
    pub source: Vec<PathBuf>,

    /// Override inferred crate/module name.
    #[arg(long, value_name = "NAME", alias = "module-name")]
    pub crate_name: Option<String>,

    /// Artifact type to produce.
    #[arg(long, value_enum, default_value_t = CrateType::Bin, alias = "output-type")]
    pub crate_type: CrateType,

    /// Language edition.
    #[arg(long, value_enum, default_value_t = Edition::E2026)]
    pub edition: Edition,
}

/// Build/session configuration that affects compilation outputs.
#[derive(Args, Debug)]
pub struct SessionArgs {
    /// Target triple (for example: x86_64-unknown-linux-gnu).
    #[arg(long, value_name = "TRIPLE")]
    pub target: Option<String>,

    /// Override compiler sysroot.
    #[arg(long, value_name = "PATH")]
    pub sysroot: Option<PathBuf>,

    /// Add configuration predicates (repeatable), e.g. --cfg feature=\"foo\".
    #[arg(long = "cfg", action = ArgAction::Append, value_name = "KEY[=\"VALUE\"]")]
    pub cfg: Vec<String>,

    /// Enable named features (repeatable).
    #[arg(long = "feature", action = ArgAction::Append, value_name = "FEATURE")]
    pub features: Vec<String>,

    /// Add an import/include path (repeatable).
    #[arg(short = 'I', long = "include", action = ArgAction::Append, value_name = "PATH")]
    pub include_path: Vec<PathBuf>,

    /// Add a library/search path (repeatable). Supports rustc-like KIND=PATH.
    #[arg(short = 'L', action = ArgAction::Append, value_name = "[KIND=]PATH")]
    pub search_path: Vec<String>,

    /// Specify external dependencies (repeatable), e.g. --extern foo=path/to/libfoo.rlib.
    #[arg(long = "extern", action = ArgAction::Append, value_name = "NAME=PATH")]
    pub externs: Vec<String>,
    /// Print compiler metadata and exit (repeatable).
    #[arg(long = "print", action = ArgAction::Append, value_enum, value_delimiter = ',', value_name = "KIND[,KIND...]")]
    pub print: Vec<PrintKind>,
}

/// Pipeline control for `compile`.
#[derive(Args, Debug)]
pub struct CompilePipelineArgs {
    /// Stop compilation after this stage.
    #[arg(long, value_enum, default_value_t = CompileStage::Link)]
    pub stage: CompileStage,
}

/// Pipeline control for `check`.
#[derive(Args, Debug)]
pub struct CheckPipelineArgs {
    /// Stop checking after this stage.
    #[arg(long, value_enum, default_value_t = CheckStage::Borrowck)]
    pub stage: CheckStage,
}

/// Output selection for `compile`.
#[derive(Args, Debug)]
pub struct CompileOutputArgs {
    /// Output kinds to emit (comma-separated), e.g. --emit asm,obj,llvm-ir.
    #[arg(
        long,
        action = ArgAction::Append,
        value_enum,
        value_delimiter = ',',
        num_args = 1..,
        default_value = "link",
        value_name = "KIND[,KIND...]"
    )]
    pub emit: Vec<CompileEmitKind>,

    /// Write final artifact to this path.
    #[arg(short = 'o', long, value_name = "PATH", conflicts_with = "out_dir")]
    pub output: Option<PathBuf>,

    /// Directory for emitted artifacts.
    #[arg(long, value_name = "DIR", conflicts_with = "output")]
    pub out_dir: Option<PathBuf>,

    /// Write dependency info to this file.
    #[arg(long, value_name = "PATH")]
    pub dep_info: Option<PathBuf>,
}

/// Output selection for `check`.
#[derive(Args, Debug)]
pub struct CheckOutputArgs {
    /// Output kinds to emit (comma-separated), e.g. --emit metadata,hir,mir.
    #[arg(
        long,
        action = ArgAction::Append,
        value_enum,
        value_delimiter = ',',
        num_args = 1..,
        default_value = "metadata",
        value_name = "KIND[,KIND...]"
    )]
    pub emit: Vec<CheckEmitKind>,

    /// Write dependency info to this file.
    #[arg(long, value_name = "PATH")]
    pub dep_info: Option<PathBuf>,
}

/// Code generation options.
#[derive(Args, Debug)]
pub struct CodegenArgs {
    /// Optimization level.
    #[arg(short = 'O', long, value_enum, default_value_t = OptLevel::O0)]
    pub opt_level: OptLevel,

    /// Debug info level.
    #[arg(short = 'g', long, value_enum, default_value_t = DebugInfo::None)]
    pub debuginfo: DebugInfo,

    /// Incremental compilation state directory.
    #[arg(long, value_name = "DIR")]
    pub incremental: Option<PathBuf>,

    /// Link-time optimization mode.
    #[arg(long, value_enum, default_value_t = LtoMode::Off)]
    pub lto: LtoMode,

    /// Code model to use for code generation.
    #[arg(long, value_enum, default_value_t = CodeModel::Small)]
    pub code_model: CodeModel,

    /// Relocation model for generated code.
    #[arg(long, value_enum, default_value_t = RelocationModel::Pic)]
    pub relocation_model: RelocationModel,

    /// Number of codegen jobs/threads.
    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Additional rustc-style codegen options (repeatable), e.g. -C target-cpu=native.
    #[arg(short = 'C', action = ArgAction::Append, value_name = "OPT[=VALUE]")]
    pub codegen_opt: Vec<String>,
}

/// Linker configuration.
#[derive(Args, Debug)]
pub struct LinkerArgs {
    /// Linker executable to invoke.
    #[arg(long, value_name = "PATH")]
    pub linker: Option<PathBuf>,

    /// Pass an argument to the linker (repeatable).
    #[arg(long, action = ArgAction::Append, value_name = "ARG")]
    pub link_arg: Vec<String>,

    /// Prefer dynamic linking where possible.
    #[arg(long, conflicts_with = "prefer_static")]
    pub prefer_dynamic: bool,

    /// Prefer static linking where possible.
    #[arg(long, conflicts_with = "prefer_dynamic")]
    pub prefer_static: bool,
}

/// Diagnostics and warning control.
#[derive(Args, Debug)]
pub struct DiagnosticsArgs {
    /// Error output format.
    #[arg(long, value_enum, default_value_t = ErrorFormat::Human)]
    pub error_format: ErrorFormat,

    /// Color diagnostics.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Treat all warnings as errors.
    #[arg(long)]
    pub warnings_as_errors: bool,

    /// Emit informational diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Suppress non-error output.
    #[arg(short = 'q', long, conflicts_with = "verbose")]
    pub quiet: bool,
}

/// Internal/unstable compiler flags.
#[derive(Args, Debug)]
pub struct InternalArgs {
    /// Internal unstable options (repeatable), rustc-style -Z.
    #[arg(short = 'Z', action = ArgAction::Append, value_name = "FLAG[=VALUE]")]
    pub unstable: Vec<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorFormat {
    Human,
    Json,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileStage {
    Parse,
    Expand,
    Resolve,
    Typeck,
    Borrowck,
    Lower,
    Hir,
    Mir,
    Codegen,
    Link,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckStage {
    Parse,
    Expand,
    Resolve,
    Typeck,
    Borrowck,
    Hir,
    Mir,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileEmitKind {
    Ast,
    Hir,
    Mir,
    #[value(name = "ir", alias = "llvm-ir")]
    Ir,
    #[value(name = "bitcode", alias = "bc", alias = "llvm-bc")]
    Bitcode,
    Asm,
    Obj,
    Metadata,
    DepInfo,
    Link,
    Tokens,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckEmitKind {
    Ast,
    Hir,
    Mir,
    Metadata,
    DepInfo,
    Tokens,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintKind {
    /// List supported compilation targets.
    TargetList,
    /// Show the default host target.
    HostTarget,
    /// Show the sysroot path in use.
    Sysroot,
    /// List supported code models.
    CodeModels,
    /// List supported relocation models.
    RelocationModels,
    /// Show available codegen options.
    CodegenOptions,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrateType {
    Bin,
    Lib,
    Rlib,
    Dylib,
    Staticlib,
    Cdylib,
    ProcMacro,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edition {
    #[value(name = "2026")]
    E2026,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptLevel {
    #[value(name = "0")]
    O0,
    #[value(name = "1")]
    O1,
    #[value(name = "2")]
    O2,
    #[value(name = "3")]
    O3,
    #[value(name = "s")]
    Os,
    #[value(name = "z")]
    Oz,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugInfo {
    None,
    LineTablesOnly,
    Limited,
    Full,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum LtoMode {
    Off,
    Thin,
    Fat,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeModel {
    Tiny,
    Small,
    Kernel,
    Medium,
    Large,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelocationModel {
    Static,
    Pic,
    Pie,
    DynamicNoPic,
}
