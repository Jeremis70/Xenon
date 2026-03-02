use clap::Parser;

use xenonc::cli::Cli;
use xenonc::driver;

fn main() {
    let cli = Cli::parse();
    let exit_code = driver::run(cli);
    // 0 for success, 1 for compilation error, 2 for internal error
    std::process::exit(exit_code);
}
