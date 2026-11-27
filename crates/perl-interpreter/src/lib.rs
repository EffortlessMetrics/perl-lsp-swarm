#![warn(missing_docs)]
//! Perl interpreter discovery shared by language-server and debugger consumers.
//!
//! [`resolve_perl_path_with_toolchain`] preserves the existing perlbrew/plenv/
//! absolute-PATH/Termux resolver. [`find_perl_interpreter`] separately preserves
//! explicit-path validation, toolchain precedence, PATH ranking and OS fallbacks.
//! [`find_perl_interpreter_cached`] adds environment and filesystem invalidation.
//!
//! Discovery observes candidates. It does not execute Perl, grant workspace trust,
//! authorize a launch, or build a project-environment snapshot.

mod discovery;
mod resolution;

pub use discovery::{PerlInterpreterResult, find_perl_interpreter, find_perl_interpreter_cached};
pub use resolution::{
    detect_perlbrew_perl, detect_plenv_perl, resolve_perl_path, resolve_perl_path_with_toolchain,
};
