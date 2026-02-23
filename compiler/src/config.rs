use crate::cli;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StopAfter {
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CompileEmitKind {
    Ast,
    Hir,
    Mir,
    Ir,
    Bitcode,
    Asm,
    Obj,
    Metadata,
    DepInfo,
    Link,
    Tokens,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CheckEmitKind {
    Ast,
    Hir,
    Mir,
    Metadata,
    DepInfo,
    Tokens,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PrintKind {
    TargetList,
    HostTarget,
    Sysroot,
    CodeModels,
    RelocationModels,
    CodegenOptions,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CrateType {
    Bin,
    Lib,
    Rlib,
    Dylib,
    Staticlib,
    Cdylib,
    ProcMacro,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Edition {
    E2026,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OptLevel {
    O0,
    O1,
    O2,
    O3,
    Os,
    Oz,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DebugInfo {
    None,
    LineTablesOnly,
    Limited,
    Full,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LtoMode {
    Off,
    Thin,
    Fat,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CodeModel {
    Tiny,
    Small,
    Kernel,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RelocationModel {
    Static,
    Pic,
    Pie,
    DynamicNoPic,
}

#[derive(Debug, Clone)]
pub struct CompileConfig {
    pub source: Vec<PathBuf>,
    pub crate_name: Option<String>,
    pub crate_type: CrateType,
    pub edition: Edition,
    pub target: Option<String>,
    pub sysroot: Option<PathBuf>,
    pub cfg: Vec<String>,
    pub features: Vec<String>,
    pub include_path: Vec<PathBuf>,
    pub search_path: Vec<String>,
    pub externs: Vec<String>,
    pub print: Vec<PrintKind>,
    pub error_format: ErrorFormat,
    pub color: ColorChoice,
    pub warnings_as_errors: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub unstable: Vec<String>,
    pub stop_after: StopAfter,
    pub emit: Vec<CompileEmitKind>,
    pub output: Option<PathBuf>,
    pub out_dir: Option<PathBuf>,
    pub dep_info: Option<PathBuf>,
    pub opt_level: OptLevel,
    pub debuginfo: DebugInfo,
    pub incremental: Option<PathBuf>,
    pub lto: LtoMode,
    pub code_model: CodeModel,
    pub relocation_model: RelocationModel,
    pub jobs: Option<usize>,
    pub codegen_opt: Vec<String>,
    pub linker: Option<PathBuf>,
    pub link_arg: Vec<String>,
    pub prefer_dynamic: bool,
    pub prefer_static: bool,
}

#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub source: Vec<PathBuf>,
    pub crate_name: Option<String>,
    pub crate_type: CrateType,
    pub edition: Edition,
    pub target: Option<String>,
    pub sysroot: Option<PathBuf>,
    pub cfg: Vec<String>,
    pub features: Vec<String>,
    pub include_path: Vec<PathBuf>,
    pub search_path: Vec<String>,
    pub externs: Vec<String>,
    pub print: Vec<PrintKind>,
    pub error_format: ErrorFormat,
    pub color: ColorChoice,
    pub warnings_as_errors: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub unstable: Vec<String>,
    pub stop_after: StopAfter,
    pub emit: Vec<CheckEmitKind>,
    pub dep_info: Option<PathBuf>,
}

impl From<cli::ErrorFormat> for ErrorFormat {
    fn from(value: cli::ErrorFormat) -> Self {
        match value {
            cli::ErrorFormat::Human => Self::Human,
            cli::ErrorFormat::Json => Self::Json,
        }
    }
}

impl From<cli::ColorChoice> for ColorChoice {
    fn from(value: cli::ColorChoice) -> Self {
        match value {
            cli::ColorChoice::Auto => Self::Auto,
            cli::ColorChoice::Always => Self::Always,
            cli::ColorChoice::Never => Self::Never,
        }
    }
}

impl From<cli::CompileStage> for StopAfter {
    fn from(value: cli::CompileStage) -> Self {
        match value {
            cli::CompileStage::Parse => Self::Parse,
            cli::CompileStage::Expand => Self::Expand,
            cli::CompileStage::Resolve => Self::Resolve,
            cli::CompileStage::Typeck => Self::Typeck,
            cli::CompileStage::Borrowck => Self::Borrowck,
            cli::CompileStage::Lower => Self::Lower,
            cli::CompileStage::Hir => Self::Hir,
            cli::CompileStage::Mir => Self::Mir,
            cli::CompileStage::Codegen => Self::Codegen,
            cli::CompileStage::Link => Self::Link,
        }
    }
}

impl From<cli::CheckStage> for StopAfter {
    fn from(value: cli::CheckStage) -> Self {
        match value {
            cli::CheckStage::Parse => Self::Parse,
            cli::CheckStage::Expand => Self::Expand,
            cli::CheckStage::Resolve => Self::Resolve,
            cli::CheckStage::Typeck => Self::Typeck,
            cli::CheckStage::Borrowck => Self::Borrowck,
            cli::CheckStage::Hir => Self::Hir,
            cli::CheckStage::Mir => Self::Mir,
        }
    }
}

impl From<cli::CompileEmitKind> for CompileEmitKind {
    fn from(value: cli::CompileEmitKind) -> Self {
        match value {
            cli::CompileEmitKind::Ast => Self::Ast,
            cli::CompileEmitKind::Hir => Self::Hir,
            cli::CompileEmitKind::Mir => Self::Mir,
            cli::CompileEmitKind::Ir => Self::Ir,
            cli::CompileEmitKind::Bitcode => Self::Bitcode,
            cli::CompileEmitKind::Asm => Self::Asm,
            cli::CompileEmitKind::Obj => Self::Obj,
            cli::CompileEmitKind::Metadata => Self::Metadata,
            cli::CompileEmitKind::DepInfo => Self::DepInfo,
            cli::CompileEmitKind::Link => Self::Link,
            cli::CompileEmitKind::Tokens => Self::Tokens,
        }
    }
}

impl From<cli::CheckEmitKind> for CheckEmitKind {
    fn from(value: cli::CheckEmitKind) -> Self {
        match value {
            cli::CheckEmitKind::Ast => Self::Ast,
            cli::CheckEmitKind::Hir => Self::Hir,
            cli::CheckEmitKind::Mir => Self::Mir,
            cli::CheckEmitKind::Metadata => Self::Metadata,
            cli::CheckEmitKind::DepInfo => Self::DepInfo,
            cli::CheckEmitKind::Tokens => Self::Tokens,
        }
    }
}

impl From<cli::PrintKind> for PrintKind {
    fn from(value: cli::PrintKind) -> Self {
        match value {
            cli::PrintKind::TargetList => Self::TargetList,
            cli::PrintKind::HostTarget => Self::HostTarget,
            cli::PrintKind::Sysroot => Self::Sysroot,
            cli::PrintKind::CodeModels => Self::CodeModels,
            cli::PrintKind::RelocationModels => Self::RelocationModels,
            cli::PrintKind::CodegenOptions => Self::CodegenOptions,
        }
    }
}

impl From<cli::CrateType> for CrateType {
    fn from(value: cli::CrateType) -> Self {
        match value {
            cli::CrateType::Bin => Self::Bin,
            cli::CrateType::Lib => Self::Lib,
            cli::CrateType::Rlib => Self::Rlib,
            cli::CrateType::Dylib => Self::Dylib,
            cli::CrateType::Staticlib => Self::Staticlib,
            cli::CrateType::Cdylib => Self::Cdylib,
            cli::CrateType::ProcMacro => Self::ProcMacro,
        }
    }
}

impl From<cli::Edition> for Edition {
    fn from(value: cli::Edition) -> Self {
        match value {
            cli::Edition::E2026 => Self::E2026,
        }
    }
}

impl From<cli::OptLevel> for OptLevel {
    fn from(value: cli::OptLevel) -> Self {
        match value {
            cli::OptLevel::O0 => Self::O0,
            cli::OptLevel::O1 => Self::O1,
            cli::OptLevel::O2 => Self::O2,
            cli::OptLevel::O3 => Self::O3,
            cli::OptLevel::Os => Self::Os,
            cli::OptLevel::Oz => Self::Oz,
        }
    }
}

impl From<cli::DebugInfo> for DebugInfo {
    fn from(value: cli::DebugInfo) -> Self {
        match value {
            cli::DebugInfo::None => Self::None,
            cli::DebugInfo::LineTablesOnly => Self::LineTablesOnly,
            cli::DebugInfo::Limited => Self::Limited,
            cli::DebugInfo::Full => Self::Full,
        }
    }
}

impl From<cli::LtoMode> for LtoMode {
    fn from(value: cli::LtoMode) -> Self {
        match value {
            cli::LtoMode::Off => Self::Off,
            cli::LtoMode::Thin => Self::Thin,
            cli::LtoMode::Fat => Self::Fat,
        }
    }
}

impl From<cli::CodeModel> for CodeModel {
    fn from(value: cli::CodeModel) -> Self {
        match value {
            cli::CodeModel::Tiny => Self::Tiny,
            cli::CodeModel::Small => Self::Small,
            cli::CodeModel::Kernel => Self::Kernel,
            cli::CodeModel::Medium => Self::Medium,
            cli::CodeModel::Large => Self::Large,
        }
    }
}

impl From<cli::RelocationModel> for RelocationModel {
    fn from(value: cli::RelocationModel) -> Self {
        match value {
            cli::RelocationModel::Static => Self::Static,
            cli::RelocationModel::Pic => Self::Pic,
            cli::RelocationModel::Pie => Self::Pie,
            cli::RelocationModel::DynamicNoPic => Self::DynamicNoPic,
        }
    }
}

impl From<&cli::CompileArgs> for CompileConfig {
    fn from(args: &cli::CompileArgs) -> Self {
        Self {
            source: args.input.source.clone(),
            crate_name: args.input.crate_name.clone(),
            crate_type: args.input.crate_type.into(),
            edition: args.input.edition.into(),
            target: args.session.target.clone(),
            sysroot: args.session.sysroot.clone(),
            cfg: args.session.cfg.clone(),
            features: args.session.features.clone(),
            include_path: args.session.include_path.clone(),
            search_path: args.session.search_path.clone(),
            externs: args.session.externs.clone(),
            print: args.session.print.iter().copied().map(Into::into).collect(),
            error_format: args.diagnostics.error_format.into(),
            color: args.diagnostics.color.into(),
            warnings_as_errors: args.diagnostics.warnings_as_errors,
            verbose: args.diagnostics.verbose,
            quiet: args.diagnostics.quiet,
            unstable: args.internal.unstable.clone(),
            stop_after: args.pipeline.stage.into(),
            emit: args.output.emit.iter().copied().map(Into::into).collect(),
            output: args.output.output.clone(),
            out_dir: args.output.out_dir.clone(),
            dep_info: args.output.dep_info.clone(),
            opt_level: args.codegen.opt_level.into(),
            debuginfo: args.codegen.debuginfo.into(),
            incremental: args.codegen.incremental.clone(),
            lto: args.codegen.lto.into(),
            code_model: args.codegen.code_model.into(),
            relocation_model: args.codegen.relocation_model.into(),
            jobs: args.codegen.jobs,
            codegen_opt: args.codegen.codegen_opt.clone(),
            linker: args.linker.linker.clone(),
            link_arg: args.linker.link_arg.clone(),
            prefer_dynamic: args.linker.prefer_dynamic,
            prefer_static: args.linker.prefer_static,
        }
    }
}

impl From<&cli::CheckArgs> for CheckConfig {
    fn from(args: &cli::CheckArgs) -> Self {
        Self {
            source: args.input.source.clone(),
            crate_name: args.input.crate_name.clone(),
            crate_type: args.input.crate_type.into(),
            edition: args.input.edition.into(),
            target: args.session.target.clone(),
            sysroot: args.session.sysroot.clone(),
            cfg: args.session.cfg.clone(),
            features: args.session.features.clone(),
            include_path: args.session.include_path.clone(),
            search_path: args.session.search_path.clone(),
            externs: args.session.externs.clone(),
            print: args.session.print.iter().copied().map(Into::into).collect(),
            error_format: args.diagnostics.error_format.into(),
            color: args.diagnostics.color.into(),
            warnings_as_errors: args.diagnostics.warnings_as_errors,
            verbose: args.diagnostics.verbose,
            quiet: args.diagnostics.quiet,
            unstable: args.internal.unstable.clone(),
            stop_after: args.pipeline.stage.into(),
            emit: args.output.emit.iter().copied().map(Into::into).collect(),
            dep_info: args.output.dep_info.clone(),
        }
    }
}
