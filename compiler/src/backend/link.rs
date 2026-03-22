use std::path::Path;
use std::process::Command;

pub fn link_executable(obj: &Path, out_exe: &Path) -> Result<(), String> {
    // Select linker based on platform
    #[cfg(target_os = "windows")]
    let linker = "clang"; // works if LLVM/Clang is installed

    #[cfg(target_os = "linux")]
    let linker = "cc";

    #[cfg(target_os = "macos")]
    let linker = "cc";

    let status = Command::new(linker)
        .arg(obj)
        .arg("-o")
        .arg(out_exe)
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Linker '{linker}' failed with status: {status}")),
        Err(_) => Err(format!(
            "Linker '{linker}' not found. Please install a C toolchain (Clang/GCC)."
        )),
    }
}
