use crate::cli::{Cli, Commands, PrintKind};
use crate::config::{CheckConfig, CompileConfig};
use crate::pipeline::{check, compile};
use crate::session::Session;
use std::path::Path;

pub fn run(cli: Cli) -> i32 {
    match &cli.command {
        Commands::Compile(args) => {
            if handle_print_requests(&args.session.print, args.session.sysroot.as_deref()) {
                return 0;
            }
            let config = CompileConfig::from(args);
            let session = match Session::from_compile_config(config) {
                Ok(session) => session,
                Err(err) => {
                    eprintln!("Error: {err}");
                    return 2;
                }
            };
            compile(&session)
        }
        Commands::Check(args) => {
            if handle_print_requests(&args.session.print, args.session.sysroot.as_deref()) {
                return 0;
            }
            let config = CheckConfig::from(args);
            let session = match Session::from_check_config(config) {
                Ok(session) => session,
                Err(err) => {
                    eprintln!("Error: {err}");
                    return 2;
                }
            };
            check(&session)
        }
    }
}

fn handle_print_requests(print_kinds: &[PrintKind], sysroot: Option<&Path>) -> bool {
    if print_kinds.is_empty() {
        return false;
    }

    for kind in print_kinds {
        match kind {
            PrintKind::TargetList => {
                println!("{}", host_target_triple());
            }
            PrintKind::HostTarget => {
                println!("{}", host_target_triple());
            }
            PrintKind::Sysroot => {
                if let Some(path) = sysroot {
                    println!("{}", path.display());
                } else {
                    println!("<sysroot-unset>");
                }
            }
            PrintKind::CodeModels => {
                for model in ["tiny", "small", "kernel", "medium", "large"] {
                    println!("{model}");
                }
            }
            PrintKind::RelocationModels => {
                for model in ["static", "pic", "pie", "dynamic-no-pic"] {
                    println!("{model}");
                }
            }
            PrintKind::CodegenOptions => {
                for option in [
                    "opt-level",
                    "debuginfo",
                    "incremental",
                    "lto",
                    "code-model",
                    "relocation-model",
                    "jobs",
                    "codegen-opt",
                ] {
                    println!("{option}");
                }
            }
        }
    }

    true
}

fn host_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        other => format!("{arch}-unknown-{other}"),
    }
}
