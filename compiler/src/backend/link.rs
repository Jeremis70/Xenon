use std::path::Path;
use std::process::Command;

use crate::backend::contract::RelocationModel;

/// Links the object `obj` into the executable `out_exe` with the system C
/// toolchain, as a PIE or not to match how the object was generated.
pub fn link_executable(
    obj: &Path,
    out_exe: &Path,
    relocation_model: RelocationModel,
) -> Result<(), String> {
    // Select linker based on platform
    #[cfg(target_os = "windows")]
    let linker = "clang"; // works if LLVM/Clang is installed

    #[cfg(target_os = "linux")]
    let linker = "cc";

    #[cfg(target_os = "macos")]
    let linker = "cc";

    let mut command = Command::new(linker);
    command.arg(obj).arg("-o").arg(out_exe);
    command.args(pie_flags(relocation_model));
    let status = command.status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Linker '{linker}' failed with status: {status}")),
        Err(_) => Err(format!(
            "Linker '{linker}' not found. Please install a C toolchain (Clang/GCC)."
        )),
    }
}

/// Linker flags that make the executable's kind agree with the object.
///
/// Many Linux toolchains link PIEs by default, which absolute relocations in
/// non-PIC objects cannot go into. PIC objects fit either kind, so `Pic`
/// keeps the toolchain's default.
#[cfg(target_os = "linux")]
fn pie_flags(relocation_model: RelocationModel) -> &'static [&'static str] {
    match relocation_model {
        RelocationModel::Pie => &["-pie"],
        RelocationModel::Static | RelocationModel::DynamicNoPic => &["-no-pie"],
        RelocationModel::Pic => &[],
    }
}

#[cfg(not(target_os = "linux"))]
fn pie_flags(_relocation_model: RelocationModel) -> &'static [&'static str] {
    &[]
}
