use std::path::PathBuf;

use crate::driver::config::OptLevel;
use crate::driver::diagnostics;
use crate::driver::session::Session;
use crate::frontend::lexer::lex;
use crate::frontend::parser::Parser;
use crate::frontend::tokens::Token;

use crate::backend::codegen::{default_output_paths, emit_object_and_ir};
use crate::backend::link::link_executable;

use crate::middle::constant_fold::fold_constants;

use crate::middle::validate::validate_program;

pub fn compile(session: &Session) -> i32 {
    let mut tokens: Vec<Token> = Vec::new();
    let mut combined_source = String::new();
    let first_path = session
        .source
        .first()
        .map(|s| s.path.display().to_string())
        .unwrap_or_else(|| "<unknown>".into());

    for source in &session.source {
        if session.verbose {
            println!("Compiling source file: {:?}", source.path);
        }
        let source_tokens = match lex(&source.content) {
            Ok(tokens) => tokens,
            Err(err) => {
                diagnostics::emit_lex_error(
                    &err,
                    &source.path.display().to_string(),
                    &source.content,
                    session.error_format,
                    session.color,
                );
                return 1;
            }
        };
        combined_source.push_str(&source.content);
        tokens.extend(source_tokens);
    }

    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.compile_emit);
    }

    let mut parser = Parser::new(&tokens);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            diagnostics::emit_parse_error(
                &e,
                &first_path,
                &combined_source,
                session.error_format,
                session.color,
            );
            return 1;
        }
    };

    let program = match fold_constants(program) {
        Ok(p) => p,
        Err(e) => {
            diagnostics::emit_fold_error(
                &e,
                &first_path,
                &combined_source,
                session.error_format,
                session.color,
            );
            return 1;
        }
    };

    if let Err(e) = validate_program(&program) {
        diagnostics::emit_semantic_error(
            &e,
            &first_path,
            &combined_source,
            session.error_format,
            session.color,
        );
        return 1;
    }

    let out_dir: PathBuf = session
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));

    let (obj_path, ll_path) = default_output_paths(&out_dir);
    let exe_path = out_dir.join("out");

    let debug_checks = session.opt_level.is_none_or(|o| o == OptLevel::O0);
    if let Err(e) = emit_object_and_ir(&program, &obj_path, Some(&ll_path), debug_checks) {
        diagnostics::emit_codegen_error(
            &e,
            &first_path,
            &combined_source,
            session.error_format,
            session.color,
        );
        return 1;
    }

    if let Err(e) = link_executable(&obj_path, &exe_path) {
        eprintln!("Link error: {e}");
        return 1;
    }

    if !session.quiet {
        println!("Wrote: {:?}", ll_path);
        println!("Wrote: {:?}", obj_path);
        println!("Wrote: {:?}", exe_path);
    }

    0
}

pub fn check(session: &Session) -> i32 {
    let mut combined_source = String::new();
    let _first_path = session
        .source
        .first()
        .map(|s| s.path.display().to_string())
        .unwrap_or_else(|| "<unknown>".into());

    for source in &session.source {
        if session.verbose {
            println!("Compiling source file: {:?}", source.path);
        }
        let tokens = match lex(&source.content) {
            Ok(tokens) => tokens,
            Err(err) => {
                diagnostics::emit_lex_error(
                    &err,
                    &source.path.display().to_string(),
                    &source.content,
                    session.error_format,
                    session.color,
                );
                return 1;
            }
        };
        combined_source.push_str(&source.content);

        if session.verbose {
            println!("Tokens: {:?}", tokens);
        }
    }

    if session.verbose {
        println!("Stage: {:?}", session.stop_after);
        println!("Emit: {:?}", session.check_emit);
    }

    0
}
