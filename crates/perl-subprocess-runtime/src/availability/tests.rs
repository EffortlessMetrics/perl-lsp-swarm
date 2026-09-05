//! Proof for the bare-name availability admission policy.
//!
//! Every row drives the pure `command_exists_in` seam with explicit `PATH` and
//! current-directory inputs, so the policy is observable without mutating
//! process-global state.

#![cfg(not(windows))]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::command_exists_in;

/// A disposable directory tree; removed when the guard drops.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "perl-subprocess-availability-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp tree");
        Self { root }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).expect("create dir");
        dir
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Write `name` into `dir` and mark it executable.
fn plant_tool(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod tool");
    }
    path
}

fn path_of(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs.iter().map(|d| d.to_path_buf())).expect("join paths")
}

// --- The defect this policy closes -------------------------------------------
//
// `which::which` resolves an empty or relative PATH component against the
// process current directory, so before this policy a planted file in the
// server's working directory satisfied an availability gate.

#[test]
fn empty_path_component_cannot_admit_a_current_directory_candidate() {
    let tree = TempTree::new("empty-component");
    let cwd = tree.dir("workspace");
    plant_tool(&cwd, "perlcritic");

    // A single empty component: `PATH=""`. POSIX reads it as the current
    // directory, and that is exactly what must not be searchable.
    assert!(
        !command_exists_in("perlcritic", Some(&OsString::from("")), &cwd),
        "an empty PATH component must not admit a current-directory candidate"
    );
}

#[test]
fn dot_path_component_cannot_admit_a_current_directory_candidate() {
    let tree = TempTree::new("dot-component");
    let cwd = tree.dir("workspace");
    plant_tool(&cwd, "perlcritic");

    assert!(
        !command_exists_in("perlcritic", Some(&OsString::from(".")), &cwd),
        "a `.` PATH component must not admit a current-directory candidate"
    );
}

#[test]
fn trailing_separator_cannot_admit_a_current_directory_candidate() {
    let tree = TempTree::new("trailing-sep");
    let cwd = tree.dir("workspace");
    let real = tree.dir("usr-bin");
    plant_tool(&cwd, "perlcritic");

    // `"/abs:"` — a legitimate absolute entry plus the empty component a
    // trailing separator produces. The absolute entry holds no tool, so the
    // only way to answer `true` is to have searched the current directory.
    let mut value = real.clone().into_os_string();
    value.push(":");

    assert!(
        !command_exists_in("perlcritic", Some(&value), &cwd),
        "a trailing PATH separator must not make the current directory searchable"
    );
}

#[test]
fn relative_path_component_is_never_admitted() {
    let tree = TempTree::new("relative-component");
    let cwd = tree.dir("workspace");
    let nested = tree.dir("workspace/tools");
    plant_tool(&nested, "perlcritic");

    // `tools` resolves against the current directory, so it is the planted
    // directory under another spelling.
    assert!(
        !command_exists_in("perlcritic", Some(&OsString::from("tools")), &cwd),
        "a relative PATH component must not be admitted"
    );
}

#[test]
fn candidate_in_the_current_directory_is_excluded_even_via_an_absolute_component() {
    let tree = TempTree::new("cwd-absolute");
    let cwd = tree.dir("workspace");
    plant_tool(&cwd, "perlcritic");

    // The component is absolute, so layer 1 admits it; only the CWD-exclusion
    // layer can reject this. Naming the same directory absolutely must not be a
    // way around the policy.
    assert!(
        !command_exists_in("perlcritic", Some(&path_of(&[&cwd])), &cwd),
        "an absolute component naming the current directory must still be excluded"
    );
}

// --- Opposite direction: the policy must not report installed tools absent ---

#[test]
fn tool_in_an_absolute_path_directory_is_admitted() {
    let tree = TempTree::new("absolute-hit");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    plant_tool(&bin, "perlcritic");

    assert!(
        command_exists_in("perlcritic", Some(&path_of(&[&bin])), &cwd),
        "a tool in a legitimate absolute PATH directory must remain available"
    );
}

#[test]
fn absolute_component_is_searched_even_when_a_planted_candidate_also_exists() {
    let tree = TempTree::new("both-present");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    plant_tool(&cwd, "perlcritic");
    plant_tool(&bin, "perlcritic");

    // Tightening admission must not blind the probe to the real installation
    // merely because a planted lookalike is also present.
    assert!(
        command_exists_in("perlcritic", Some(&path_of(&[&cwd, &bin])), &cwd),
        "a real absolute-PATH tool must still be found alongside a planted one"
    );
}

#[test]
fn later_absolute_component_is_still_searched() {
    let tree = TempTree::new("second-component");
    let cwd = tree.dir("workspace");
    let empty = tree.dir("empty-bin");
    let bin = tree.dir("usr-bin");
    plant_tool(&bin, "perlcritic");

    assert!(
        command_exists_in("perlcritic", Some(&path_of(&[&empty, &bin])), &cwd),
        "the search must continue past an absolute component that holds no match"
    );
}

// --- Absence, shape, and executability ---------------------------------------

#[test]
fn absent_tool_is_reported_absent() {
    let tree = TempTree::new("absent");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    plant_tool(&bin, "perlcritic");

    assert!(
        !command_exists_in("perltidy", Some(&path_of(&[&bin])), &cwd),
        "a tool that is not installed must be reported absent"
    );
}

#[test]
fn non_executable_file_is_not_available() {
    let tree = TempTree::new("non-executable");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    std::fs::write(bin.join("perlcritic"), b"not executable").expect("write file");

    assert!(
        !command_exists_in("perlcritic", Some(&path_of(&[&bin])), &cwd),
        "a non-executable file of the right name is not a usable tool"
    );
}

#[test]
fn directory_of_the_right_name_is_not_available() {
    let tree = TempTree::new("directory");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    std::fs::create_dir_all(bin.join("perlcritic")).expect("create dir");

    assert!(
        !command_exists_in("perlcritic", Some(&path_of(&[&bin])), &cwd),
        "a directory must never satisfy an availability probe"
    );
}

#[test]
fn absent_path_variable_admits_nothing() {
    let tree = TempTree::new("no-path");
    let cwd = tree.dir("workspace");
    plant_tool(&cwd, "perlcritic");

    assert!(
        !command_exists_in("perlcritic", None, &cwd),
        "an absent PATH must fail closed rather than falling back to the cwd"
    );
}

#[test]
fn path_bearing_input_is_refused() {
    let tree = TempTree::new("path-bearing");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");
    let nested = tree.dir("usr-bin/tools");

    // The tool is planted *in the searched directory*, and in a subdirectory of
    // it, so each spelling below would resolve to a real executable if the
    // path-bearing refusal were removed. Planting it anywhere else would make
    // these rows pass for the wrong reason.
    let installed = plant_tool(&bin, "perlcritic");
    plant_tool(&nested, "perlcritic");

    let search = path_of(&[&bin]);

    // Sanity: the bare name genuinely resolves against this PATH, so a refusal
    // below is the path-bearing rule and not an empty fixture.
    assert!(
        command_exists_in("perlcritic", Some(&search), &cwd),
        "precondition: the bare name resolves in this fixture"
    );

    assert!(
        !command_exists_in("./perlcritic", Some(&search), &cwd),
        "a `./`-prefixed name is not a PATH lookup and must be refused"
    );
    assert!(
        !command_exists_in("tools/perlcritic", Some(&search), &cwd),
        "a nested relative path-bearing name must be refused"
    );
    assert!(
        !command_exists_in(installed.to_str().expect("utf-8 path"), Some(&search), &cwd),
        "an absolute path is not a bare name and carries its own trust policy"
    );
}

#[test]
fn empty_command_is_refused() {
    let tree = TempTree::new("empty-command");
    let cwd = tree.dir("workspace");
    let bin = tree.dir("usr-bin");

    assert!(!command_exists_in("", Some(&path_of(&[&bin])), &cwd));
}
