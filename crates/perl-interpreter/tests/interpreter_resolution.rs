//! Interpreter resolution and cache regressions moved with their implementation.

use perl_interpreter::{
    PerlInterpreterResult, find_perl_interpreter_cached, resolve_perl_path,
    resolve_perl_path_with_toolchain,
};
#[cfg(not(windows))]
use perl_interpreter::{detect_perlbrew_perl, detect_plenv_perl};
#[cfg(not(windows))]
use perl_test_must::{must, must_some};
use serial_test::serial;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[serial]
fn resolve_perl_path_succeeds_when_perl_available() -> TestResult {
    // Environment-dependent: only assert properties when perl is found
    if let Ok(path) = resolve_perl_path() {
        assert!(path.exists(), "resolved perl path must exist");
        assert!(path.is_file(), "resolved perl path must be a regular file");

        // Should end with "perl" or "perl.exe"
        let filename = path.file_name().ok_or("perl path has no filename")?.to_string_lossy();
        assert!(
            filename == "perl" || filename == "perl.exe",
            "perl binary should be named 'perl' or 'perl.exe', got: {filename}"
        );
    }
    Ok(())
}

#[test]
#[serial]
fn resolve_perl_path_returns_absolute_path() -> TestResult {
    if let Ok(path) = resolve_perl_path() {
        assert!(path.is_absolute(), "resolved perl path should be absolute, got: {path:?}");
    }
    Ok(())
}

#[test]
#[serial]
fn find_perl_interpreter_cached_respects_configured_path_changes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let fake_perl = tmp.path().join("perl");
    std::fs::write(&fake_perl, "#!/usr/bin/env perl\n")?;

    let configured = fake_perl.to_string_lossy().to_string();
    let found = find_perl_interpreter_cached(Some(&configured));
    assert!(
        matches!(found, PerlInterpreterResult::ConfiguredPath(ref path) if *path == fake_perl),
        "configured path should be returned as-is, got: {found:?}"
    );

    let missing = tmp.path().join("missing-perl");
    let missing_cfg = missing.to_string_lossy().to_string();
    let not_found = find_perl_interpreter_cached(Some(&missing_cfg));
    assert!(
        matches!(not_found, PerlInterpreterResult::NotFound { .. }),
        "missing configured path should not be replaced by cached previous result, got: {not_found:?}"
    );

    Ok(())
}

#[test]
#[serial]
fn find_perl_interpreter_cached_revalidates_configured_file_changes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let fake_perl = tmp.path().join("cached-perl");
    let configured = fake_perl.to_string_lossy().to_string();

    let missing = find_perl_interpreter_cached(Some(&configured));
    assert!(
        matches!(missing, PerlInterpreterResult::NotFound { .. }),
        "missing configured path should be cached as NotFound, got: {missing:?}"
    );

    std::fs::write(&fake_perl, b"#!/usr/bin/env perl\n")?;
    let found = find_perl_interpreter_cached(Some(&configured));
    assert!(
        matches!(found, PerlInterpreterResult::ConfiguredPath(ref path) if *path == fake_perl),
        "creating a configured interpreter without changing its key must invalidate NotFound, got: {found:?}"
    );

    std::fs::remove_file(&fake_perl)?;
    let removed = find_perl_interpreter_cached(Some(&configured));
    assert!(
        matches!(removed, PerlInterpreterResult::NotFound { .. }),
        "deleting a configured interpreter without changing its key must invalidate ConfiguredPath, got: {removed:?}"
    );

    Ok(())
}
/// Create a fake perl binary that is executable (Unix only).
#[cfg(not(windows))]
fn make_fake_perl(dir: &std::path::Path, name: &str) -> TestResult {
    let bin = dir.join(name);
    std::fs::write(&bin, b"#!/bin/sh\necho perl")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms)?;
    }
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn detect_perlbrew_perl_env_var_points_to_valid_binary() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let bin_dir = tmp.path().join("perls").join("perl-5.38.0").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    make_fake_perl(&bin_dir, "perl")?;
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::set_var("PERLBREW_ROOT", tmp.path());
        std::env::set_var("PERLBREW_PERL", "perl-5.38.0");
    }
    let result = detect_perlbrew_perl();
    unsafe {
        std::env::remove_var("PERLBREW_PERL");
        std::env::remove_var("PERLBREW_ROOT");
    }
    let path = must_some(result);
    assert!(path.ends_with("perl"), "should point to perl binary, got: {path:?}");
    assert!(path.exists(), "detected perlbrew perl should exist on disk");
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn detect_perlbrew_perl_env_set_but_binary_missing_returns_none() -> TestResult {
    let tmp = tempfile::tempdir()?;
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::set_var("PERLBREW_ROOT", tmp.path());
        std::env::set_var("PERLBREW_PERL", "perl-nonexistent-9.99.0");
    }
    let result = detect_perlbrew_perl();
    unsafe {
        std::env::remove_var("PERLBREW_PERL");
        std::env::remove_var("PERLBREW_ROOT");
    }
    assert!(result.is_none(), "None when PERLBREW_PERL points to nonexistent binary");
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn detect_plenv_perl_env_var_points_to_valid_binary() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let bin_dir = tmp.path().join("versions").join("5.38.0").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    make_fake_perl(&bin_dir, "perl")?;
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::set_var("PLENV_ROOT", tmp.path());
        std::env::set_var("PLENV_VERSION", "5.38.0");
    }
    let result = detect_plenv_perl();
    unsafe {
        std::env::remove_var("PLENV_VERSION");
        std::env::remove_var("PLENV_ROOT");
    }
    let path = must_some(result);
    assert!(path.ends_with("perl"), "should point to perl binary, got: {path:?}");
    assert!(path.exists(), "detected plenv perl should exist on disk");
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn detect_plenv_perl_env_set_but_binary_missing_returns_none() -> TestResult {
    let tmp = tempfile::tempdir()?;
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::set_var("PLENV_ROOT", tmp.path());
        std::env::set_var("PLENV_VERSION", "9.99.0");
    }
    let result = detect_plenv_perl();
    unsafe {
        std::env::remove_var("PLENV_VERSION");
        std::env::remove_var("PLENV_ROOT");
    }
    assert!(result.is_none(), "None when PLENV_VERSION points to nonexistent binary");
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn resolve_perl_path_with_toolchain_prefers_perlbrew_over_path() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let bin_dir = tmp.path().join("perls").join("perl-5.38.0").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    make_fake_perl(&bin_dir, "perl")?;
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::set_var("PERLBREW_ROOT", tmp.path());
        std::env::set_var("PERLBREW_PERL", "perl-5.38.0");
    }
    let result = resolve_perl_path_with_toolchain();
    unsafe {
        std::env::remove_var("PERLBREW_PERL");
        std::env::remove_var("PERLBREW_ROOT");
    }
    let path = must(result);
    assert!(
        path.to_string_lossy().contains("perl-5.38.0"),
        "should use perlbrew perl, got: {path:?}"
    );
    Ok(())
}

#[cfg(not(windows))]
#[test]
#[serial]
fn resolve_perl_path_with_toolchain_prefers_plenv_over_path() -> TestResult {
    // Ensure perlbrew vars are absent so plenv is tried.
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::remove_var("PERLBREW_PERL");
        std::env::remove_var("PERLBREW_ROOT");
    }
    let tmp = tempfile::tempdir()?;
    let bin_dir = tmp.path().join("versions").join("5.36.0").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    make_fake_perl(&bin_dir, "perl")?;
    unsafe {
        std::env::set_var("PLENV_ROOT", tmp.path());
        std::env::set_var("PLENV_VERSION", "5.36.0");
    }
    let result = resolve_perl_path_with_toolchain();
    unsafe {
        std::env::remove_var("PLENV_VERSION");
        std::env::remove_var("PLENV_ROOT");
    }
    let path = must(result);
    assert!(path.to_string_lossy().contains("5.36.0"), "should use plenv perl, got: {path:?}");
    Ok(())
}

#[test]
#[serial]
fn resolve_perl_path_with_toolchain_falls_back_to_path() -> TestResult {
    // Clear toolchain env vars so PATH fallback is exercised.
    // Safety: env mutation is safe because #[serial] prevents
    // concurrent access to PERLBREW_ROOT/PERLBREW_PERL/PLENV_ROOT/PLENV_VERSION.
    unsafe {
        std::env::remove_var("PERLBREW_PERL");
        std::env::remove_var("PERLBREW_ROOT");
        std::env::remove_var("PLENV_VERSION");
        std::env::remove_var("PLENV_ROOT");
    }
    let result = resolve_perl_path_with_toolchain();
    match result {
        Ok(path) => {
            assert!(path.is_absolute(), "fallback PATH perl should be absolute");
        }
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("perl") || msg.contains("PATH"),
                "error should mention perl/PATH: {msg}"
            );
        }
    }
    Ok(())
}
