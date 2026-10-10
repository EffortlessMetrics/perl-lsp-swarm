//! One admitted-mode seam shared by both process-test resolver families.
//! Ordinary developer resolution remains with each existing caller.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

pub fn resolve(profile: &str) -> Option<Result<PathBuf, String>> {
    resolve_with(
        profile,
        |name| std::env::var_os(name),
        |python, profile| {
            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/ci/perllsp_workspace_prepare.py");
            Command::new(python)
                .arg("-I")
                .arg(script)
                .args(["--resolve", profile])
                .output()
                .map_err(|error| format!("admitted perllsp validator could not start: {error}"))
                .and_then(|output| {
                    if !output.status.success() {
                        return Err(format!(
                            "admitted perllsp refused: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    }
                    String::from_utf8(output.stdout)
                        .map_err(|error| format!("invalid admitted executable path: {error}"))
                })
        },
    )
}

fn resolve_with(
    profile: &str,
    mut variable: impl FnMut(&str) -> Option<OsString>,
    invoke: impl FnOnce(OsString, &str) -> Result<String, String>,
) -> Option<Result<PathBuf, String>> {
    let admitted =
        ["CARGO_ADMITTED_RESOURCES", "CARGO_ADMITTED_PERLLSP_HANDOFF", "CARGO_ADMITTED_PYTHON"]
            .iter()
            .any(|name| variable(name).is_some());
    if !admitted {
        return None;
    }
    Some((|| {
        for name in ["CARGO_ADMITTED_RESOURCES", "CARGO_ADMITTED_PERLLSP_HANDOFF", "PERL_LSP_BIN"] {
            if variable(name).is_none_or(|value| value.is_empty()) {
                return Err(format!("missing admitted perllsp binding: {name}"));
            }
        }
        let python = variable("CARGO_ADMITTED_PYTHON")
            .filter(|value| !value.is_empty())
            .ok_or("missing admitted perllsp validator interpreter")?;
        let path = invoke(python, profile)?;
        let path = path.trim_end_matches(['\r', '\n']);
        if path.is_empty() || path.contains(['\r', '\n']) || !PathBuf::from(path).is_absolute() {
            return Err("invalid admitted perllsp executable path".into());
        }
        if variable("PERL_LSP_BIN").as_deref() != Some(std::ffi::OsStr::new(path)) {
            return Err("admitted perllsp executable differs from child binding".into());
        }
        Ok(PathBuf::from(path))
    })())
}
