//! Proof for the bare-name availability admission policy.
//!
//! Every row drives the pure `command_exists_in` seam with explicit `PATH` and
//! current-directory inputs, so the policy is observable without mutating
//! process-global state.

#![cfg(not(windows))]

use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::command_exists_in;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn require(condition: bool, message: &str) -> TestResult {
    if condition { Ok(()) } else { Err(message.into()) }
}

/// A disposable directory tree; removed when the guard drops.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(tag: &str) -> TestResult<Self> {
        let root = std::env::temp_dir().join(format!(
            "perl-subprocess-availability-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).map_err(|e| format!("create temp tree: {e}"))?;
        Ok(Self { root })
    }

    fn dir(&self, name: &str) -> TestResult<PathBuf> {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("create dir: {e}"))?;
        Ok(dir)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Write `name` into `dir` and mark it executable.
fn plant_tool(dir: &Path, name: &str) -> TestResult<PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").map_err(|e| format!("write tool: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod tool: {e}"))?;
    }
    Ok(path)
}

fn path_of(dirs: &[&Path]) -> TestResult<OsString> {
    Ok(std::env::join_paths(dirs.iter().map(|d| d.to_path_buf()))?)
}

// --- The defect this policy closes -------------------------------------------
//
// `which::which` resolves an empty or relative PATH component against the
// process current directory, so before this policy a planted file in the
// server's working directory satisfied an availability gate.

#[test]
fn empty_path_component_cannot_admit_a_current_directory_candidate() -> TestResult {
    let tree = TempTree::new("empty-component")?;
    let cwd = tree.dir("workspace")?;
    plant_tool(&cwd, "perlcritic")?;

    // A single empty component: `PATH=""`. POSIX reads it as the current
    // directory, and that is exactly what must not be searchable.
    require(
        !command_exists_in("perlcritic", Some(OsStr::new("")), &cwd),
        "an empty PATH component must not admit a current-directory candidate",
    )?;
    Ok(())
}

#[test]
fn dot_path_component_cannot_admit_a_current_directory_candidate() -> TestResult {
    let tree = TempTree::new("dot-component")?;
    let cwd = tree.dir("workspace")?;
    plant_tool(&cwd, "perlcritic")?;

    require(
        !command_exists_in("perlcritic", Some(OsStr::new(".")), &cwd),
        "a `.` PATH component must not admit a current-directory candidate",
    )?;
    Ok(())
}

#[test]
fn trailing_separator_cannot_admit_a_current_directory_candidate() -> TestResult {
    let tree = TempTree::new("trailing-sep")?;
    let cwd = tree.dir("workspace")?;
    let real = tree.dir("usr-bin")?;
    plant_tool(&cwd, "perlcritic")?;

    // `"/abs:"` — a legitimate absolute entry plus the empty component a
    // trailing separator produces. The absolute entry holds no tool, so the
    // only way to answer `true` is to have searched the current directory.
    let mut value = real.clone().into_os_string();
    value.push(":");

    require(
        !command_exists_in("perlcritic", Some(&value), &cwd),
        "a trailing PATH separator must not make the current directory searchable",
    )?;
    Ok(())
}

#[test]
fn relative_path_component_is_never_admitted() -> TestResult {
    let tree = TempTree::new("relative-component")?;
    let cwd = tree.dir("workspace")?;
    let nested = tree.dir("workspace/tools")?;
    plant_tool(&nested, "perlcritic")?;

    // `tools` resolves against the current directory, so it is the planted
    // directory under another spelling.
    require(
        !command_exists_in("perlcritic", Some(OsStr::new("tools")), &cwd),
        "a relative PATH component must not be admitted",
    )?;
    Ok(())
}

#[test]
fn candidate_in_the_current_directory_is_excluded_even_via_an_absolute_component() -> TestResult {
    let tree = TempTree::new("cwd-absolute")?;
    let cwd = tree.dir("workspace")?;
    plant_tool(&cwd, "perlcritic")?;

    // The component is absolute, so layer 1 admits it; only the CWD-exclusion
    // layer can reject this. Naming the same directory absolutely must not be a
    // way around the policy.
    require(
        !command_exists_in("perlcritic", Some(&path_of(&[&cwd])?), &cwd),
        "an absolute component naming the current directory must still be excluded",
    )?;
    Ok(())
}

// --- Opposite direction: the policy must not report installed tools absent ---

#[test]
fn tool_in_an_absolute_path_directory_is_admitted() -> TestResult {
    let tree = TempTree::new("absolute-hit")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    plant_tool(&bin, "perlcritic")?;

    require(
        command_exists_in("perlcritic", Some(&path_of(&[&bin])?), &cwd),
        "a tool in a legitimate absolute PATH directory must remain available",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_absolute_path_component_preserves_admission_policy() -> TestResult {
    let tree = TempTree::new("non-utf8-path")?;
    let name = OsString::from_vec(b"bin-\xff".to_vec());
    let bin = tree.root.join(&name);
    std::fs::create_dir_all(&bin).map_err(|e| format!("create non-utf8 dir: {e}"))?;
    plant_tool(&bin, "perlcritic")?;

    let path = path_of(&[&bin])?;
    require(
        command_exists_in("perlcritic", Some(&path), &tree.root),
        "an executable under an absolute non-UTF-8 PATH component must remain available",
    )?;
    require(
        !command_exists_in("perlcritic", Some(&path), &bin),
        "a non-UTF-8 directory must still be excluded when it is the current directory",
    )
}

#[test]
fn absolute_component_is_searched_even_when_a_planted_candidate_also_exists() -> TestResult {
    let tree = TempTree::new("both-present")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    plant_tool(&cwd, "perlcritic")?;
    plant_tool(&bin, "perlcritic")?;

    // Tightening admission must not blind the probe to the real installation
    // merely because a planted lookalike is also present.
    require(
        command_exists_in("perlcritic", Some(&path_of(&[&cwd, &bin])?), &cwd),
        "a real absolute-PATH tool must still be found alongside a planted one",
    )?;
    Ok(())
}

#[test]
fn later_absolute_component_is_still_searched() -> TestResult {
    let tree = TempTree::new("second-component")?;
    let cwd = tree.dir("workspace")?;
    let empty = tree.dir("empty-bin")?;
    let bin = tree.dir("usr-bin")?;
    plant_tool(&bin, "perlcritic")?;

    require(
        command_exists_in("perlcritic", Some(&path_of(&[&empty, &bin])?), &cwd),
        "the search must continue past an absolute component that holds no match",
    )?;
    Ok(())
}

// --- Absence, shape, and executability ---------------------------------------

#[test]
fn absent_tool_is_reported_absent() -> TestResult {
    let tree = TempTree::new("absent")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    plant_tool(&bin, "perlcritic")?;

    require(
        !command_exists_in("perltidy", Some(&path_of(&[&bin])?), &cwd),
        "a tool that is not installed must be reported absent",
    )?;
    Ok(())
}

#[test]
fn non_executable_file_is_not_available() -> TestResult {
    let tree = TempTree::new("non-executable")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    std::fs::write(bin.join("perlcritic"), b"not executable")
        .map_err(|e| format!("write file: {e}"))?;

    require(
        !command_exists_in("perlcritic", Some(&path_of(&[&bin])?), &cwd),
        "a non-executable file of the right name is not a usable tool",
    )?;
    Ok(())
}

#[test]
fn directory_of_the_right_name_is_not_available() -> TestResult {
    let tree = TempTree::new("directory")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    std::fs::create_dir_all(bin.join("perlcritic")).map_err(|e| format!("create dir: {e}"))?;

    require(
        !command_exists_in("perlcritic", Some(&path_of(&[&bin])?), &cwd),
        "a directory must never satisfy an availability probe",
    )?;
    Ok(())
}

#[test]
fn absent_path_variable_admits_nothing() -> TestResult {
    let tree = TempTree::new("no-path")?;
    let cwd = tree.dir("workspace")?;
    plant_tool(&cwd, "perlcritic")?;

    require(
        !command_exists_in("perlcritic", None, &cwd),
        "an absent PATH must fail closed rather than falling back to the cwd",
    )?;
    Ok(())
}

#[test]
fn path_bearing_input_is_refused() -> TestResult {
    let tree = TempTree::new("path-bearing")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;
    let nested = tree.dir("usr-bin/tools")?;

    // The tool is planted *in the searched directory*, and in a subdirectory of
    // it, so each spelling below would resolve to a real executable if the
    // path-bearing refusal were removed. Planting it anywhere else would make
    // these rows pass for the wrong reason.
    let installed = plant_tool(&bin, "perlcritic")?;
    plant_tool(&nested, "perlcritic")?;

    let search = path_of(&[&bin])?;

    // Sanity: the bare name genuinely resolves against this PATH, so a refusal
    // below is the path-bearing rule and not an empty fixture.
    require(
        command_exists_in("perlcritic", Some(&search), &cwd),
        "precondition: the bare name resolves in this fixture",
    )?;

    require(
        !command_exists_in("./perlcritic", Some(&search), &cwd),
        "a `./`-prefixed name is not a PATH lookup and must be refused",
    )?;
    require(
        !command_exists_in("tools/perlcritic", Some(&search), &cwd),
        "a nested relative path-bearing name must be refused",
    )?;
    require(
        !command_exists_in(
            installed.to_str().ok_or("fixture path is not UTF-8")?,
            Some(&search),
            &cwd,
        ),
        "an absolute path is not a bare name and carries its own trust policy",
    )?;
    Ok(())
}

#[test]
fn empty_command_is_refused() -> TestResult {
    let tree = TempTree::new("empty-command")?;
    let cwd = tree.dir("workspace")?;
    let bin = tree.dir("usr-bin")?;

    require(
        !command_exists_in("", Some(&path_of(&[&bin])?), &cwd),
        "empty command must be refused",
    )?;
    Ok(())
}
