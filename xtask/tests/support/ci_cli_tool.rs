//! Native dependency stand-in for the real bare-ci CLI integration proof.
use std::{env, fs, io::Write, path::Path, process};

fn main() {
    let executable = env::current_exe().expect("fixture executable");
    let tool = Path::new(&executable).file_stem().expect("fixture tool name").to_string_lossy();
    let args: Vec<String> = env::args().skip(1).collect();
    let log = env::var_os("XTASK_CI_FIXTURE_LOG").expect("fixture log");
    let mut file = fs::OpenOptions::new().append(true).open(log).expect("open fixture log");
    writeln!(file, "{tool} {args:?}").expect("record exact invocation");
    if tool == "rustfmt" {
        return;
    }
    if tool != "cargo" {
        process::exit(83);
    }
    match args.first().map(String::as_str) {
        Some("metadata") => {
            let metadata = env::var_os("XTASK_CI_FIXTURE_METADATA").expect("fixture metadata");
            print!("{}", fs::read_to_string(metadata).expect("read fixture metadata"));
        }
        Some("clippy") => {
            if env::var_os("XTASK_CI_FIXTURE_FAIL_CLIPPY").is_some() {
                eprintln!("fixture clippy failed with exit 19");
                process::exit(19);
            }
        }
        Some("test") => {}
        Some("doc") => println!("fixture-doc-completed"),
        _ => process::exit(83),
    }
}
