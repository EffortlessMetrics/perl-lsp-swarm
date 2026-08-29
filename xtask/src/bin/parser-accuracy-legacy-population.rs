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
    let output = match env::args().nth(1).as_deref() {
        None | Some("--cases") => population.canonical_ndjson()?,
        Some("--summary") => population.canonical_summary_json()?,
        Some(argument) => {
            return Err(io::Error::other(format!(
                "unknown argument {argument:?}; expected --cases or --summary"
            ))
            .into());
        }
    };

    io::stdout().lock().write_all(output.as_bytes())?;
    Ok(())
}
