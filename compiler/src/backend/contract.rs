//! Backend-neutral code-generation contract.

use std::path::PathBuf;

use thiserror::Error;

use crate::middle::mir::MirProgram;
use crate::middle::target::TargetSpec;

/// Artifact kinds understood by the current backend contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    /// LLVM textual IR, a backend-specific debug artifact.
    LlvmIr,
    /// Native object code.
    Object,
}

/// A requested output from a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRequest {
    /// The kind of artifact requested.
    pub kind: ArtifactKind,
    /// Destination path.
    pub path: PathBuf,
}

/// Options shared by backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodegenOptions {
    /// Requested optimization level, interpreted by the selected backend.
    pub optimization: u8,
}

/// A successfully produced backend artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Artifact kind.
    pub kind: ArtifactKind,
    /// Written file.
    pub path: PathBuf,
}

/// A backend-independent code-generation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackendError {
    /// The selected backend does not support an operation or type.
    #[error("backend capability error: {0}")]
    Unsupported(String),
    /// Backend initialization failed.
    #[error("backend initialization failed: {0}")]
    Initialization(String),
    /// The backend rejected the verified MIR.
    #[error("backend rejected MIR: {0}")]
    InvalidMir(String),
    /// An output artifact could not be written.
    #[error("failed to write backend artifact: {0}")]
    Output(String),
    /// The backend's target description does not match MIR target facts.
    #[error("backend target mismatch: {0}")]
    TargetMismatch(String),
}

/// A code generator that consumes verified MIR, never source syntax.
pub trait Backend {
    /// Name used by CLI diagnostics and metadata.
    fn name(&self) -> &'static str;

    /// Emits the requested artifacts from MIR for `target`.
    fn emit(
        &self,
        program: &MirProgram,
        target: &TargetSpec,
        options: CodegenOptions,
        outputs: &[OutputRequest],
    ) -> Result<Vec<Artifact>, BackendError>;
}
