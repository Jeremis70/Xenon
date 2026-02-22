mod cli;

use clap::Parser;

use crate::cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Compile(args) => {
            for source in &args.input.source {
                println!("Compiling source file: {:?}", source);
            }

            println!("Stage: {:?}", args.pipeline.stage);
            println!("Emit: {:?}", args.output.emit);
            println!("Crate type: {:?}", args.input.crate_type);
            println!("Edition: {:?}", args.input.edition);
            println!("Opt level: {:?}", args.codegen.opt_level);
            println!("Debug info: {:?}", args.codegen.debuginfo);
            println!("LTO: {:?}", args.codegen.lto);

            if let Some(target) = &args.session.target {
                println!("Target: {}", target);
            }
            if let Some(sysroot) = &args.session.sysroot {
                println!("Sysroot: {:?}", sysroot);
            }
            if !args.session.cfg.is_empty() {
                println!("cfg: {:?}", args.session.cfg);
            }
            if !args.session.features.is_empty() {
                println!("features: {:?}", args.session.features);
            }
            if !args.session.include_path.is_empty() {
                println!("include paths: {:?}", args.session.include_path);
            }
            if !args.session.search_path.is_empty() {
                println!("search paths: {:?}", args.session.search_path);
            }
            if !args.session.externs.is_empty() {
                println!("externs: {:?}", args.session.externs);
            }
            if !args.session.print.is_empty() {
                println!("print: {:?}", args.session.print);
            }

            if let Some(output) = &args.output.output {
                println!("Output file: {:?}", output);
            } else if let Some(out_dir) = &args.output.out_dir {
                println!("Output directory: {:?}", out_dir);
            } else {
                println!("No output file specified, using default.");
            }

            if let Some(dep_info) = &args.output.dep_info {
                println!("Dep-info: {:?}", dep_info);
            }

            if let Some(incremental) = &args.codegen.incremental {
                println!("Incremental dir: {:?}", incremental);
            }
            if let Some(jobs) = args.codegen.jobs {
                println!("Codegen jobs: {}", jobs);
            }
            if !args.codegen.codegen_opt.is_empty() {
                println!("Codegen opts (-C): {:?}", args.codegen.codegen_opt);
            }

            if let Some(linker) = &args.linker.linker {
                println!("Linker: {:?}", linker);
            }
            if !args.linker.link_arg.is_empty() {
                println!("Link args: {:?}", args.linker.link_arg);
            }

            println!(
                "Diagnostics: format={:?}, color={:?}, warnings_as_errors={}, verbose={}, quiet={}",
                args.diagnostics.error_format,
                args.diagnostics.color,
                args.diagnostics.warnings_as_errors,
                args.diagnostics.verbose,
                args.diagnostics.quiet
            );

            if !args.internal.unstable.is_empty() {
                println!("Internal unstable flags (-Z): {:?}", args.internal.unstable);
            }
        }
        Commands::Check(args) => {
            for source in &args.input.source {
                println!("Checking source file: {:?}", source);
            }

            println!("Stage: {:?}", args.pipeline.stage);
            println!("Emit: {:?}", args.output.emit);
            println!("Crate type: {:?}", args.input.crate_type);
            println!("Edition: {:?}", args.input.edition);

            if let Some(target) = &args.session.target {
                println!("Target: {}", target);
            }
            if let Some(sysroot) = &args.session.sysroot {
                println!("Sysroot: {:?}", sysroot);
            }
            if !args.session.cfg.is_empty() {
                println!("cfg: {:?}", args.session.cfg);
            }
            if !args.session.features.is_empty() {
                println!("features: {:?}", args.session.features);
            }
            if !args.session.include_path.is_empty() {
                println!("include paths: {:?}", args.session.include_path);
            }
            if !args.session.search_path.is_empty() {
                println!("search paths: {:?}", args.session.search_path);
            }
            if !args.session.externs.is_empty() {
                println!("externs: {:?}", args.session.externs);
            }
            if !args.session.print.is_empty() {
                println!("print: {:?}", args.session.print);
            }

            if let Some(dep_info) = &args.output.dep_info {
                println!("Dep-info: {:?}", dep_info);
            }

            println!(
                "Diagnostics: format={:?}, color={:?}, warnings_as_errors={}, verbose={}, quiet={}",
                args.diagnostics.error_format,
                args.diagnostics.color,
                args.diagnostics.warnings_as_errors,
                args.diagnostics.verbose,
                args.diagnostics.quiet
            );

            if !args.internal.unstable.is_empty() {
                println!("Internal unstable flags (-Z): {:?}", args.internal.unstable);
            }
        }
    }
}
