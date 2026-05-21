use super::config::{Config, InputKind, Mode};

pub(crate) fn usage() -> &'static str {
    "Usage: bench_parser_c <file> [--mode cold|warm] [--iterations N] [--input str|bytes]\n\
     Defaults: --mode cold --iterations 1 --input str"
}

pub(crate) fn parse_config(args: &[String]) -> Result<Config, String> {
    if args.len() < 2 {
        return Err(usage().to_string());
    }

    let file_path = args[1].clone();
    let mut mode = Mode::Cold;
    let mut input = InputKind::Str;
    let mut iterations = 1_u64;

    let mut index = 2_usize;
    while index < args.len() {
        match args[index].as_str() {
            "--mode" => {
                let value = args.get(index + 1).ok_or_else(|| "Missing value for --mode".to_string())?;
                mode = parse_mode(value)?;
                index += 2;
            }
            "--iterations" | "-n" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "Missing value for --iterations".to_string())?;
                iterations = parse_iterations(value)?;
                index += 2;
            }
            "--input" => {
                let value = args.get(index + 1).ok_or_else(|| "Missing value for --input".to_string())?;
                input = parse_input(value)?;
                index += 2;
            }
            "--cold" => {
                mode = Mode::Cold;
                index += 1;
            }
            "--warm" => {
                mode = Mode::Warm;
                index += 1;
            }
            "--help" | "-h" => {
                return Err(usage().to_string());
            }
            unknown => {
                return Err(format!("Unknown argument: {unknown}"));
            }
        }
    }

    Ok(Config { file_path, mode, input, iterations })
}

fn parse_mode(value: &str) -> Result<Mode, String> {
    match value {
        "cold" => Ok(Mode::Cold),
        "warm" => Ok(Mode::Warm),
        _ => Err(format!("Invalid mode: {value}")),
    }
}

fn parse_iterations(value: &str) -> Result<u64, String> {
    let iterations = value
        .parse::<u64>()
        .map_err(|_| format!("Invalid iteration count: {value}"))?;

    if iterations == 0 {
        return Err("--iterations must be greater than 0".to_string());
    }

    Ok(iterations)
}

fn parse_input(value: &str) -> Result<InputKind, String> {
    match value {
        "str" => Ok(InputKind::Str),
        "bytes" => Ok(InputKind::Bytes),
        _ => Err(format!("Invalid input mode: {value}")),
    }
}
