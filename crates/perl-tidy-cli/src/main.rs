#![deny(unsafe_code)]

mod app;
mod args;
mod input;

use clap::Parser;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match args::Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = if error.use_stderr() { app::ExitClass::Usage.code() } else { 0 };
            if error.print().is_err() {
                return ExitCode::from(app::ExitClass::OutputError.code());
            }
            return ExitCode::from(code);
        }
    };
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut diagnostics = io::stderr().lock();
    let result = app::run(&args, &mut input, &mut output, &mut diagnostics);
    match result {
        Ok(class) => ExitCode::from(class.code()),
        Err(error) => {
            let _ = writeln!(diagnostics, "perl-tidy: {}", error.message);
            ExitCode::from(error.class.code())
        }
    }
}
