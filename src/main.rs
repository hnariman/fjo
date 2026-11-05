use clap::Parser;

mod cli;
mod git;
mod key;
mod status;

use cli::{Cli, Outcome};

fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    match cli.run() {
        Ok(outcome) => {
            emit(json, &outcome);
        }
        Err(err) => {
            if json {
                // agents parse stdout; errors keep the same single-object shape
                println!(
                    "{}",
                    serde_json::json!({ "ok": false, "error": err.to_string() })
                );
            } else {
                eprintln!("fjo: {err}");
            }
            std::process::exit(1);
        }
    }
}

/// One-line human text, or one machine-parseable JSON object.
fn emit(json: bool, outcome: &Outcome) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "message": outcome.message,
                "data": outcome.extra,
            })
        );
    } else {
        println!("{}", outcome.message);
    }
}
