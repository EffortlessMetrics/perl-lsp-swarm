//! Emit the exact legacy parser-accuracy population retained by #13654.

use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;

use xtask::parser_accuracy_legacy_population::load_legacy_whitespace_population;

fn main() -> Result<(), Box<dyn Error>> {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !root.pop() {
        return Err(io::Error::other("xtask manifest directory has no repository parent").into());
    }

    let population = load_legacy_whitespace_population(&root)?;
    // `env::args` panics on non-UTF-8 arguments; collect `OsString`s and fail
    // closed instead, so a hostile argument is rejected, never a crash.
    let arguments = env::args_os()
        .skip(1)
        .map(|argument| {
            argument.into_string().map_err(|invalid| {
                io::Error::other(format!(
                    "non-UTF-8 argument {invalid:?}; expected --cases or --summary"
                ))
            })
        })
        .collect::<Result<Vec<String>, _>>()?;
    let output = match arguments.as_slice() {
        [] => population.canonical_ndjson()?,
        [argument] if argument == "--cases" => population.canonical_ndjson()?,
        [argument] if argument == "--summary" => population.canonical_summary_json()?,
        _ => {
            return Err(io::Error::other(format!(
                "unknown arguments {arguments:?}; expected --cases or --summary"
            ))
            .into());
        }
    };

    io::stdout().lock().write_all(output.as_bytes())?;
    Ok(())
}
