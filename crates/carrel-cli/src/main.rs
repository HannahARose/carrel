//! `carrel`: the command-line tool for Carrel archives.
//!
//! Commands arrive with #18; for now it reports its version.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--version" | "-V" | "version") => {
            println!(
                "carrel {} (archive format {})",
                carrel_core::CARREL_VERSION,
                carrel_core::FORMAT_VERSION
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("carrel: unknown command '{other}'. Commands arrive in a later version.");
            ExitCode::from(2)
        }
    }
}
