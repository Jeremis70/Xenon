use crate::lexer::lex;
use crate::session::Session;

pub fn compile(session: &Session) -> i32 {
    for source in &session.source {
        println!("Compiling source file: {:?}", source.path);
        let tokens = lex(&source.content);
        println!("Tokens: {:?}", tokens);
    }

    println!("Stage: {:?}", session.stop_after);
    println!("Emit: {:?}", session.compile_emit);
    println!("Crate type: {:?}", session.crate_type);
    println!("Edition: {:?}", session.edition);
    if let Some(opt_level) = session.opt_level {
        println!("Opt level: {:?}", opt_level);
    }
    if let Some(debuginfo) = session.debuginfo {
        println!("Debug info: {:?}", debuginfo);
    }
    if let Some(lto) = session.lto {
        println!("LTO: {:?}", lto);
    }
    if let Some(code_model) = session.code_model {
        println!("Code model: {:?}", code_model);
    }
    if let Some(relocation_model) = session.relocation_model {
        println!("Relocation model: {:?}", relocation_model);
    }

    print_common_session_data(session);

    if let Some(output) = &session.output {
        println!("Output file: {:?}", output);
    } else if let Some(out_dir) = &session.out_dir {
        println!("Output directory: {:?}", out_dir);
    } else {
        println!("No output file specified, using default.");
    }

    if let Some(dep_info) = &session.dep_info {
        println!("Dep-info: {:?}", dep_info);
    }

    if let Some(incremental) = &session.incremental {
        println!("Incremental dir: {:?}", incremental);
    }
    if let Some(jobs) = session.jobs {
        println!("Codegen jobs: {}", jobs);
    }
    if !session.codegen_opt.is_empty() {
        println!("Codegen opts (-C): {:?}", session.codegen_opt);
    }

    if let Some(linker) = &session.linker {
        println!("Linker: {:?}", linker);
    }
    if !session.link_arg.is_empty() {
        println!("Link args: {:?}", session.link_arg);
    }
    if session.prefer_dynamic {
        println!("Link preference: dynamic");
    }
    if session.prefer_static {
        println!("Link preference: static");
    }
    0
}

pub fn check(session: &Session) -> i32 {
    for source in &session.source {
        println!("Compiling source file: {:?}", source.path);
        let tokens = lex(&source.content);
        println!("Tokens: {:?}", tokens);
    }

    println!("Stage: {:?}", session.stop_after);
    println!("Emit: {:?}", session.check_emit);
    println!("Crate type: {:?}", session.crate_type);
    println!("Edition: {:?}", session.edition);

    print_common_session_data(session);

    if let Some(dep_info) = &session.dep_info {
        println!("Dep-info: {:?}", dep_info);
    }

    0
}

fn print_common_session_data(session: &Session) {
    if let Some(crate_name) = &session.crate_name {
        println!("Crate name: {}", crate_name);
    }

    if let Some(target) = &session.target {
        println!("Target: {}", target);
    }
    if let Some(sysroot) = &session.sysroot {
        println!("Sysroot: {:?}", sysroot);
    }
    if !session.cfg.is_empty() {
        println!("cfg: {:?}", session.cfg);
    }
    if !session.features.is_empty() {
        println!("features: {:?}", session.features);
    }
    if !session.include_path.is_empty() {
        println!("include paths: {:?}", session.include_path);
    }
    if !session.search_path.is_empty() {
        println!("search paths: {:?}", session.search_path);
    }
    if !session.externs.is_empty() {
        println!("externs: {:?}", session.externs);
    }
    println!(
        "Diagnostics: format={:?}, color={:?}, warnings_as_errors={}, verbose={}, quiet={}",
        session.error_format,
        session.color,
        session.warnings_as_errors,
        session.verbose,
        session.quiet
    );

    if !session.unstable.is_empty() {
        println!("Internal unstable flags (-Z): {:?}", session.unstable);
    }
}
