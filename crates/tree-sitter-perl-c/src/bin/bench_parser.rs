use std::env;
use std::fs;
use std::time::Instant;
use tree_sitter_perl_c::{
    parse_perl_bytes, parse_perl_bytes_with_parser, parse_perl_code, parse_perl_code_with_parser,
    try_create_parser,
};

mod cli;
mod config;

use cli::parse_config;
use config::{BenchSummary, Config, InputKind, Mode};

fn parse_cold(input: InputKind, code: &[u8]) -> Result<bool, String> {
    let tree = match input {
        InputKind::Str => {
            let as_str = std::str::from_utf8(code)
                .map_err(|err| format!("Input is not UTF-8 for --input str: {err}"))?;
            parse_perl_code(as_str).map_err(|err| err.to_string())?
        }
        InputKind::Bytes => parse_perl_bytes(code).map_err(|err| err.to_string())?,
    };
    Ok(tree.root_node().has_error())
}

fn run_benchmark(config: &Config, code: &[u8]) -> Result<BenchSummary, String> {
    let mut saw_error = false;
    let start = Instant::now();

    match config.mode {
        Mode::Cold => {
            for _ in 0..config.iterations {
                if parse_cold(config.input, code)? {
                    saw_error = true;
                }
            }
        }
        Mode::Warm => {
            let mut parser = try_create_parser().map_err(|err| err.to_string())?;
            for _ in 0..config.iterations {
                let tree = match config.input {
                    InputKind::Str => {
                        let as_str = std::str::from_utf8(code)
                            .map_err(|err| format!("Input is not UTF-8 for --input str: {err}"))?;
                        parse_perl_code_with_parser(&mut parser, as_str)
                            .map_err(|err| err.to_string())?
                    }
                    InputKind::Bytes => parse_perl_bytes_with_parser(&mut parser, code)
                        .map_err(|err| err.to_string())?,
                };
                if tree.root_node().has_error() {
                    saw_error = true;
                }
            }
        }
    }

    let total_us = start.elapsed().as_micros();
    let avg_us = total_us / u128::from(config.iterations);

    Ok(BenchSummary {
        mode: config.mode,
        input: config.input,
        iterations: config.iterations,
        total_us,
        avg_us,
        has_error: saw_error,
    })
}

fn print_summary(summary: &BenchSummary) {
    println!("mode={}", summary.mode.as_str());
    println!("input={}", summary.input.as_str());
    println!("iterations={}", summary.iterations);
    println!("total_us={}", summary.total_us);
    println!("avg_us={}", summary.avg_us);
    println!("has_error={}", summary.has_error);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let config = match parse_config(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let code = match fs::read(&config.file_path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Failed to read file: {err}");
            std::process::exit(1);
        }
    };

    match run_benchmark(&config, &code) {
        Ok(summary) => {
            print_summary(&summary);
        }
        Err(err) => {
            eprintln!("Parse error: {err}");
            std::process::exit(1);
        }
    }
}
