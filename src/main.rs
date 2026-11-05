use clap::Parser;

mod cli;
mod git;
mod key;

use cli::Cli;

fn main() {
    if let Err(err) = Cli::parse().run() {
        eprintln!("fjo: {err}");
        std::process::exit(1);
    }
}
