//! DAP path/environment helpers and compatibility interpreter-discovery exports.
//!
//! Interpreter discovery is owned by `perl-interpreter`. These exports retain
//! the existing DAP API and share the same result types and process-local cache.

pub use crate::command_args::format_command_args;
pub use perl_interpreter::{
    PerlInterpreterResult, detect_perlbrew_perl, detect_plenv_perl, find_perl_interpreter,
    find_perl_interpreter_cached, resolve_perl_path, resolve_perl_path_with_toolchain,
};

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(windows)]
const PATH_SEPARATOR: char = ';';
#[cfg(not(windows))]
const PATH_SEPARATOR: char = ':';

/// Normalize a file path for cross-platform compatibility.
pub fn normalize_path(path: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        // WSL drive mounts have the shape `/mnt/<letter>/...` with a single ASCII
        // drive letter. Index the drive by char, never by byte, so a non-ASCII first
        // segment (e.g. `/mnt/é/...`) is left untranslated instead of panicking on a
        // mid-codepoint byte slice, and a non-letter segment is not treated as a drive
        // (#3927).
        if let Some(path_str) = path.to_str()
            && let Some(after_mnt) = path_str.strip_prefix("/mnt/")
            && let Some(drive) = after_mnt.chars().next()
            && drive.is_ascii_alphabetic()
        {
            let rest = &after_mnt[drive.len_utf8()..];
            // Require a separator after the single drive letter. Bare `/mnt/c`
            // and adjacent mount names such as `/mnt/cx` must not become the
            // drive-relative Windows paths `C:` and `C:x` (#13028).
            if rest.starts_with('/') {
                let windows_path =
                    format!("{}:{}", drive.to_ascii_uppercase(), rest.replace('/', "\\"));
                return PathBuf::from(windows_path);
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(path_str) = path.to_str() {
            if path_str.len() >= 2
                && path_str.chars().nth(1) == Some(':')
                && let Some(first_char) = path_str.chars().next()
            {
                let drive_letter = first_char.to_uppercase();
                let rest = &path_str[1..];
                return PathBuf::from(format!("{}{}", drive_letter, rest));
            }

            if path_str.starts_with("\\\\") {
                return path.to_path_buf();
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(canonical) = path.canonicalize() {
            return canonical;
        }
    }

    path.to_path_buf()
}

/// Setup environment variables for Perl execution.
pub fn setup_environment(include_paths: &[PathBuf]) -> HashMap<String, String> {
    let mut env = HashMap::new();

    if !include_paths.is_empty() {
        let perl5lib = include_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(&PATH_SEPARATOR.to_string());

        env.insert("PERL5LIB".to_string(), perl5lib);
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_basic() {
        let normalized = normalize_path(&PathBuf::from("script.pl"));
        assert!(!normalized.as_os_str().is_empty());
    }

    #[test]
    fn test_setup_environment_empty() {
        let env = setup_environment(&[]);
        assert!(!env.contains_key("PERL5LIB"));
    }

    #[test]
    fn test_setup_environment_with_paths() {
        let env =
            setup_environment(&[PathBuf::from("/workspace/lib"), PathBuf::from("/custom/lib")]);
        assert!(env.contains_key("PERL5LIB"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn normalize_path_wsl_mnt_translated_to_windows_style() {
        let wsl_path = std::path::Path::new("/mnt/c/Users/user/script.pl");
        let normalized = normalize_path(wsl_path);
        let s = normalized.to_string_lossy();
        assert!(
            s.starts_with("C:\\") || s.starts_with("C:/"),
            "expected Windows-style path, got: {s}"
        );
        assert!(s.contains("Users"), "path content preserved: {s}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn normalize_path_wsl_mnt_multibyte_drive_does_not_panic() {
        // #3927: a non-ASCII first segment under /mnt/ must not be byte-sliced at a
        // mid-codepoint boundary. `é` is two bytes (0xC3 0xA9), so `[5..6]` panicked.
        let wsl_path = std::path::Path::new("/mnt/é/file.pl");
        let normalized = normalize_path(wsl_path);
        let s = normalized.to_string_lossy();
        // A non-ASCII "drive" is not a valid WSL mount letter: leave it untranslated
        // rather than producing a bogus Windows path.
        assert!(s.contains('é'), "multi-byte segment preserved: {s}");
        assert!(!s.contains(':'), "non-ASCII drive must not be translated: {s}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn normalize_path_wsl_mnt_non_alpha_drive_not_translated() {
        // A digit in the drive position is not a valid mount letter — leave as-is.
        let normalized = normalize_path(std::path::Path::new("/mnt/1/file.pl"));
        let s = normalized.to_string_lossy();
        assert!(!s.contains('\\') && !s.contains(':'), "non-alpha drive not translated: {s}");
    }

    #[test]
    fn normalize_path_non_wsl_unix_path_unchanged_on_linux() {
        let path = std::path::Path::new("/usr/local/bin/perl");
        let normalized = normalize_path(path);
        assert!(
            !normalized.to_string_lossy().contains('\\'),
            "non-WSL path should not be Windows-escaped"
        );
    }
}
