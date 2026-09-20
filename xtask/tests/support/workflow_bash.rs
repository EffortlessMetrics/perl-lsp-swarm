//! Locating a POSIX shell for tests that execute a workflow's real `run:` block.
//!
//! Several contract suites prove a workflow guard by extracting its `run:`
//! block and executing it under Actions bash semantics, rather than asserting
//! on YAML text — a text assertion still passes when a comparison is inverted.
//! They all need the same thing: the shell Actions would have used.
//!
//! This lived in three copies that had already drifted apart. Two searched both
//! Git-for-Windows install roots; the third searched only the 64-bit one, so on
//! a 64-bit host carrying 32-bit Git it fell through to a bare `bash` that is
//! not on `PATH` there. Consolidating removes that gap rather than merely
//! deduplicating the text.

// `mod support;` is compiled fresh per integration-test binary and not every
// binary uses every item — unused ones here are false-positive dead code, not
// unreachable production code.
#![allow(dead_code)]

use std::{env, path::PathBuf};

/// The shell to run a workflow `run:` block with.
///
/// `BASH` wins when set, so a host with a non-standard shell location can point
/// the suites at it without patching them.
pub fn bash_executable() -> PathBuf {
    if let Some(path) = env::var_os("BASH") {
        return path.into();
    }
    #[cfg(windows)]
    {
        // 32-bit Git for Windows installs under Program Files (x86) on 64-bit
        // hosts, so both roots have to be searched.
        for candidate in
            [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"]
        {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from("bash")
}
