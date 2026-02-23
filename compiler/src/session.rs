use crate::config::{
    CheckConfig, CheckEmitKind, CodeModel, ColorChoice, CompileConfig, CompileEmitKind, CrateType,
    DebugInfo, Edition, ErrorFormat, LtoMode, OptLevel, RelocationModel, StopAfter,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub content: String,
}

fn load_source_files(paths: Vec<PathBuf>) -> Result<Vec<SourceFile>, String> {
    paths
        .into_iter()
        .map(|path| {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("couldn't read {:?}: {}", path, e))?;
            Ok(SourceFile { path, content })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Session {
    pub source: Vec<SourceFile>,
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
    pub error_format: ErrorFormat,
    pub color: ColorChoice,
    pub warnings_as_errors: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub unstable: Vec<String>,
    pub stop_after: StopAfter,
    pub compile_emit: Vec<CompileEmitKind>,
    pub check_emit: Vec<CheckEmitKind>,
    pub output: Option<PathBuf>,
    pub out_dir: Option<PathBuf>,
    pub dep_info: Option<PathBuf>,
    pub opt_level: Option<OptLevel>,
    pub debuginfo: Option<DebugInfo>,
    pub incremental: Option<PathBuf>,
    pub lto: Option<LtoMode>,
    pub code_model: Option<CodeModel>,
    pub relocation_model: Option<RelocationModel>,
    pub jobs: Option<usize>,
    pub codegen_opt: Vec<String>,
    pub linker: Option<PathBuf>,
    pub link_arg: Vec<String>,
    pub prefer_dynamic: bool,
    pub prefer_static: bool,
}

impl Session {
    pub fn from_compile_config(config: CompileConfig) -> Result<Self, String> {
        Ok(Self {
            source: load_source_files(config.source)?,
            crate_name: config.crate_name,
            crate_type: config.crate_type,
            edition: config.edition,
            target: config.target,
            sysroot: config.sysroot,
            cfg: config.cfg,
            features: config.features,
            include_path: config.include_path,
            search_path: config.search_path,
            externs: config.externs,
            error_format: config.error_format,
            color: config.color,
            warnings_as_errors: config.warnings_as_errors,
            verbose: config.verbose,
            quiet: config.quiet,
            unstable: config.unstable,
            stop_after: config.stop_after,
            compile_emit: config.emit,
            check_emit: Vec::new(),
            output: config.output,
            out_dir: config.out_dir,
            dep_info: config.dep_info,
            opt_level: Some(config.opt_level),
            debuginfo: Some(config.debuginfo),
            incremental: config.incremental,
            lto: Some(config.lto),
            code_model: Some(config.code_model),
            relocation_model: Some(config.relocation_model),
            jobs: config.jobs,
            codegen_opt: config.codegen_opt,
            linker: config.linker,
            link_arg: config.link_arg,
            prefer_dynamic: config.prefer_dynamic,
            prefer_static: config.prefer_static,
        })
    }

    pub fn from_check_config(config: CheckConfig) -> Result<Self, String> {
        Ok(Self {
            source: load_source_files(config.source)?,
            crate_name: config.crate_name,
            crate_type: config.crate_type,
            edition: config.edition,
            target: config.target,
            sysroot: config.sysroot,
            cfg: config.cfg,
            features: config.features,
            include_path: config.include_path,
            search_path: config.search_path,
            externs: config.externs,
            error_format: config.error_format,
            color: config.color,
            warnings_as_errors: config.warnings_as_errors,
            verbose: config.verbose,
            quiet: config.quiet,
            unstable: config.unstable,
            stop_after: config.stop_after,
            compile_emit: Vec::new(),
            check_emit: config.emit,
            output: None,
            out_dir: None,
            dep_info: config.dep_info,
            opt_level: None,
            debuginfo: None,
            incremental: None,
            lto: None,
            code_model: None,
            relocation_model: None,
            jobs: None,
            codegen_opt: Vec::new(),
            linker: None,
            link_arg: Vec::new(),
            prefer_dynamic: false,
            prefer_static: false,
        })
    }
}
