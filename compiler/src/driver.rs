use crate::cli::{Cli, Commands};
use crate::config::{CheckConfig, CompileConfig};
use crate::pipeline::{check, compile};
use crate::session::Session;

pub fn run(cli: Cli) -> i32 {
    match &cli.command {
        Commands::Compile(args) => {
            let config = CompileConfig::from(args);
            let session = Session::from_compile_config(config);
            compile(&session)
        }
        Commands::Check(args) => {
            let config = CheckConfig::from(args);
            let session = Session::from_check_config(config);
            check(&session)
        }
    }
}
