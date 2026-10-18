//! Development tasks. Run with `cargo run -p xtask -- <task>`.

mod scrub;

use std::process::ExitCode;

const HELP: &str = "\
Usage: cargo run -p xtask -- <task>

Tasks:
  scrub <file>   Print <file> (a captured API response) with personal data removed:
                 ids, email, names, notes, URLs and tokens are replaced, GPS is
                 moved to a fake location, long sample lists are thinned.
                 Review the output before committing it.
  help           Show this message
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("scrub") => {
            let Some(path) = args.next() else {
                eprintln!("usage: cargo run -p xtask -- scrub <file>");
                return ExitCode::FAILURE;
            };
            match scrub::scrub_file(&path) {
                Ok(output) => {
                    println!("{output}");
                    eprintln!(
                        "scrubbed {path}. Check activity and workout names before committing."
                    );
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("xtask scrub: {err}");
                    ExitCode::FAILURE
                }
            }
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
