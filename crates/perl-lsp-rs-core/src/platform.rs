//! Compatibility exports for canonical Perl interpreter resolution.
//!
//! The implementation is owned by `perl-interpreter`, below both LSP and DAP.
//! These re-exports preserve the existing public resolver paths and behavior.

pub use perl_interpreter::{
    detect_perlbrew_perl, detect_plenv_perl, resolve_perl_path, resolve_perl_path_with_toolchain,
};
