//! Development tasks. Run with `cargo run -p xtask -- <task>`.

use std::process::ExitCode;

const HELP: &str = "\
Usage: cargo run -p xtask -- <task>

Tasks:
  scrub <file>   Remove personal data from a captured API response (Phase 1)
  help           Show this message
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("scrub") => {
            eprintln!(
                "xtask scrub: not implemented yet (planned for Phase 1, see docs/TESTING.md)"
            );
            ExitCode::FAILURE
        }
        None | Some("help" | "-h" | "--help") => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown task `{other}`\n\n{HELP}");
            ExitCode::FAILURE
        }
    }
}
