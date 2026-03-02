mod ast;
mod cli;
mod codegen;
mod config;
mod driver;
mod lexer;
mod link;
mod parser;
mod pipeline;
mod session;
use clap::Parser;

use crate::cli::Cli;

fn main() {
    let cli = Cli::parse();
    let exit_code = driver::run(cli);
    // 0 for success, 1 for compilation error, 2 for internal error
    std::process::exit(exit_code);
}
