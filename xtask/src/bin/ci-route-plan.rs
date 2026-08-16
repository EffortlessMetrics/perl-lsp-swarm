use std::env;
use std::fs;
use std::path::Path;

use xtask::ci_route_plan::{CiRoutePlanV1, CompileRoutePlanInput};

fn main() {
    if let Err(error) = run() {
        eprintln!("ci-route-plan: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("compile") => {
            let input = required_path(&mut args, "compile input")?;
            let output = required_path(&mut args, "compile output")?;
            reject_extra(args)?;
            let payload: CompileRoutePlanInput = read_json(&input)?;
            let plan = CiRoutePlanV1::compile(payload)?;
            write_atomic(&output, &plan.canonical_json()?)
        }
        Some("validate") => {
            let input = required_path(&mut args, "plan")?;
            reject_extra(args)?;
            let plan: CiRoutePlanV1 = read_json(&input)?;
            plan.validate()?;
            println!("ci-route-plan: valid {}", plan.semantic_fingerprint);
            Ok(())
        }
        Some("explain") => {
            let input = required_path(&mut args, "plan")?;
            let gate = args.next();
            reject_extra(args)?;
            let plan: CiRoutePlanV1 = read_json(&input)?;
            println!("{}", plan.explain(gate.as_deref())?);
            Ok(())
        }
        _ => Err(
            "usage: ci-route-plan compile <input.json> <output.json> | validate <plan.json> | explain <plan.json> [gate_id]"
                .to_string(),
        ),
    }
}

fn required_path(args: &mut impl Iterator<Item = String>, subject: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("missing {subject} path"))
}

fn reject_extra(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument {extra:?}"));
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {path:?}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {path:?}: {error}"))
}

fn write_atomic(path: &str, bytes: &[u8]) -> Result<(), String> {
    let path = Path::new(path);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| format!("create {parent:?}: {error}"))?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|error| format!("write {temp:?}: {error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("rename {temp:?} to {path:?}: {error}"))
}
