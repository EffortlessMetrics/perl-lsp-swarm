//! Shared-helper self-tests, registered only by `dap_smoke_e2e`.

#[cfg(unix)]
use crate::common::decode_evaluate_path;
use crate::common::{
    DebuggeePerl, ProbeFailure, assert_pinned_identity, launch_arguments,
    normalize_explicit_debuggee_pin, resolve_debuggee_candidate,
    resolve_debuggee_perl_uncached_with,
};
#[cfg(windows)]
use crate::common::{bare_name_lookup_variants, normalize_windows_path_prefix};

#[cfg(test)]
mod launch_argument_tests {
    use super::launch_arguments;
    use serde_json::Value;
    use std::path::Path;

    #[test]
    fn launch_arguments_preserve_exact_pinned_interpreter() -> Result<(), String> {
        let pinned = Path::new("C:/controls/pinned-perl.exe");
        let args = launch_arguments("fixture.pl", Some("C:/work"), true, Some(pinned));

        if args.get("perlPath") != Some(&Value::String(pinned.to_string_lossy().into_owned())) {
            return Err("launch arguments must preserve the pinned interpreter".to_string());
        }
        if args.get("cwd") != Some(&Value::String("C:/work".to_string())) {
            return Err("launch arguments must preserve cwd".to_string());
        }
        if args.get("stopOnEntry") != Some(&Value::Bool(true)) {
            return Err("launch arguments must preserve stopOnEntry".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod explicit_pin_tests {
    use super::{
        assert_pinned_identity, launch_arguments, normalize_explicit_debuggee_pin,
        resolve_debuggee_candidate,
    };
    use perl_dap::variables::{PerlValue, PerlVariableRenderer, VariableRenderer};
    use std::fs;
    use std::path::Path;

    #[test]
    fn pinned_identity_decodes_rendered_dap_string() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let pinned = controls.path().join("perl with spaces");
        let ambient = controls.path().join("ambient-perl");
        fs::write(&pinned, b"pinned").map_err(|error| error.to_string())?;
        fs::write(&ambient, b"ambient").map_err(|error| error.to_string())?;

        let rendered = PerlVariableRenderer::new()
            .render("$^X", &PerlValue::Scalar(pinned.to_string_lossy().to_string()))
            .value;
        assert_pinned_identity(&rendered, &pinned, &ambient, "rendered")?;
        let plain =
            pinned.to_str().ok_or_else(|| "the test pin should be valid UTF-8".to_string())?;
        assert_pinned_identity(plain, &pinned, &ambient, "plain")
    }

    #[test]
    fn pinned_identity_decodes_perl_debugger_ordinal() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let pinned = controls.path().join("perl5");
        let ambient = controls.path().join("perl");
        fs::write(&pinned, b"pinned").map_err(|error| error.to_string())?;
        fs::write(&ambient, b"ambient").map_err(|error| error.to_string())?;

        let perl_quote =
            |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('\'', "\\'");
        let rendered = serde_json::to_string(&format!("0  '{}'", perl_quote(&pinned)))
            .map_err(|error| error.to_string())?;
        assert_pinned_identity(&rendered, &pinned, &ambient, "ordinal")?;

        let ambient_rendered = format!("0  '{}'", perl_quote(&ambient));
        let error = assert_pinned_identity(&ambient_rendered, &pinned, &ambient, "ambient ordinal")
            .err()
            .ok_or_else(|| "the ordinal ambient path was accepted".to_string())?;
        if !error.contains("expected pinned") {
            return Err(format!("ordinal ambient rejection lost its reason: {error}"));
        }
        Ok(())
    }

    #[test]
    fn pinned_identity_decodes_raw_perl_debugger_escapes() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        #[cfg(unix)]
        let pinned = controls.path().join("nested").join("pinned\\'perl");
        #[cfg(not(unix))]
        let pinned = controls.path().join("nested").join("pinned'perl");
        let ambient = controls.path().join("ambient").join("perl'with spaces");
        fs::create_dir_all(
            pinned.parent().ok_or_else(|| "the pinned path should have a parent".to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::create_dir_all(
            ambient.parent().ok_or_else(|| "the ambient path should have a parent".to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::write(&pinned, b"pinned").map_err(|error| error.to_string())?;
        fs::write(&ambient, b"ambient").map_err(|error| error.to_string())?;

        let perl_quote =
            |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('\'', "\\'");
        let raw_perl5db = format!("0  '{}'", perl_quote(&pinned));
        assert_pinned_identity(&raw_perl5db, &pinned, &ambient, "raw perl5db")?;
        let rendered = serde_json::to_string(&raw_perl5db).map_err(|error| error.to_string())?;
        assert_pinned_identity(&rendered, &pinned, &ambient, "JSON-wrapped perl5db")?;

        let ambient_rendered = format!("0  '{}'", perl_quote(&ambient));
        let error = assert_pinned_identity(&ambient_rendered, &pinned, &ambient, "raw ambient")
            .err()
            .ok_or_else(|| "the escaped ambient path was accepted".to_string())?;
        if !error.contains("expected pinned") {
            return Err(format!("escaped ambient rejection lost its reason: {error}"));
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn plain_path_ending_in_double_quote_is_not_json_decoded() -> Result<(), String> {
        let path = "/tmp/perl-lsp-15217-plain-quote\"";
        if super::decode_evaluate_path(path)? != path {
            return Err("plain path ending in a double quote was changed".to_string());
        }
        Ok(())
    }

    #[test]
    fn pinned_identity_rejects_rendered_ambient_path() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let pinned = controls.path().join("pinned-perl");
        let ambient = controls.path().join("ambient-perl");
        fs::write(&pinned, b"pinned").map_err(|error| error.to_string())?;
        fs::write(&ambient, b"ambient").map_err(|error| error.to_string())?;

        let rendered_ambient = PerlVariableRenderer::new()
            .render("$^X", &PerlValue::Scalar(ambient.to_string_lossy().to_string()))
            .value;
        let error = assert_pinned_identity(&rendered_ambient, &pinned, &ambient, "ambient")
            .err()
            .ok_or_else(|| "the rendered ambient path was accepted".to_string())?;
        if !error.contains("expected pinned") {
            return Err(format!("ambient rejection lost its reason: {error}"));
        }
        Ok(())
    }

    #[test]
    fn pinned_identity_rejects_ambiguous_unclosed_rendered_string() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let pinned = controls.path().join("pinned-perl");
        let ambient = controls.path().join("ambient-perl");
        fs::write(&pinned, b"pinned").map_err(|error| error.to_string())?;
        fs::write(&ambient, b"ambient").map_err(|error| error.to_string())?;

        let error = assert_pinned_identity("\"unclosed", &pinned, &ambient, "malformed")
            .err()
            .ok_or_else(|| "malformed rendered output was accepted".to_string())?;
        if !error.contains("raw DAP result") || !error.contains("invalid quoted DAP string") {
            return Err(format!("malformed rendered output lost diagnostic context: {error}"));
        }

        let ordinal_error = assert_pinned_identity("0  '", &pinned, &ambient, "ordinal")
            .err()
            .ok_or_else(|| "an empty ordinal payload was accepted".to_string())?;
        if !ordinal_error.contains("raw DAP result") || !ordinal_error.contains("unclosed") {
            return Err(format!("malformed ordinal lost diagnostic context: {ordinal_error}"));
        }

        let empty_ordinal_error = assert_pinned_identity("0  ''", &pinned, &ambient, "ordinal")
            .err()
            .ok_or_else(|| "an empty quoted ordinal payload was accepted".to_string())?;
        if !empty_ordinal_error.contains("raw DAP result") || !empty_ordinal_error.contains("empty")
        {
            return Err(format!("empty ordinal lost diagnostic context: {empty_ordinal_error}"));
        }

        let unsupported_escape = assert_pinned_identity("0  'C:\\q'", &pinned, &ambient, "ordinal")
            .err()
            .ok_or_else(|| "an unsupported ordinal escape was accepted".to_string())?;
        if !unsupported_escape.contains("unsupported perl5db ordinal escape") {
            return Err(format!(
                "unsupported ordinal escape lost its reason: {unsupported_escape}"
            ));
        }

        let escaped_quote_without_close =
            assert_pinned_identity("0  'C:\\'", &pinned, &ambient, "ordinal")
                .err()
                .ok_or_else(|| "an ordinal with no closing quote was accepted".to_string())?;
        if !escaped_quote_without_close.contains("unclosed") {
            return Err(format!(
                "unclosed escaped ordinal lost its reason: {escaped_quote_without_close}"
            ));
        }
        Ok(())
    }

    #[test]
    fn nested_relative_pin_is_frozen_before_different_launch_cwd() -> Result<(), String> {
        let process_cwd = std::env::current_dir()
            .map_err(|error| format!("the current directory should resolve: {error}"))?;
        let controls = tempfile::Builder::new()
            .prefix("perl-lsp-dap-relative-pin-")
            .tempdir_in(&process_cwd)
            .map_err(|error| format!("relative-pin controls should be created: {error}"))?;
        let nested_dir = controls.path().join("nested").join("bin");
        fs::create_dir_all(&nested_dir)
            .map_err(|error| format!("nested relative-pin directory should be created: {error}"))?;
        let binary = nested_dir.join(if cfg!(windows) { "pinned-perl.exe" } else { "pinned-perl" });
        fs::write(&binary, b"pin identity control")
            .map_err(|error| format!("relative-pin executable should be created: {error}"))?;
        let relative = binary
            .strip_prefix(&process_cwd)
            .map_err(|error| format!("control path should be relative to the test cwd: {error}"))?;
        let expected = fs::canonicalize(&binary).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        let expected = super::normalize_windows_path_prefix(expected);
        let resolved = normalize_explicit_debuggee_pin(relative)
            .map_err(|error| format!("the nested relative pin should canonicalize: {error}"))?;
        if resolved != expected {
            return Err(format!(
                "resolved pin differs from canonical path: {resolved:?} != {expected:?}"
            ));
        }

        let different_cwd = controls.path().join("different-launch-cwd");
        fs::create_dir(&different_cwd)
            .map_err(|error| format!("different launch cwd should be created: {error}"))?;
        let different_cwd =
            different_cwd.to_str().ok_or("different launch cwd should be valid UTF-8")?;
        let launch = launch_arguments("fixture.pl", Some(different_cwd), false, Some(&resolved));
        if launch.get("perlPath").and_then(|value| value.as_str())
            != Some(resolved.to_string_lossy().as_ref())
        {
            return Err(
                "the launch must carry the canonical pin, not reinterpret it under cwd".to_string()
            );
        }
        if launch.get("cwd").and_then(|value| value.as_str()) != Some(different_cwd) {
            return Err("the launch must retain its independent working directory".to_string());
        }
        Ok(())
    }

    /// A bare program name reachable only through the search path must resolve
    /// to the absolute path the probe will execute, so the launch boundary's
    /// pin canonicalization sees the same value (#13553).
    #[test]
    fn path_only_candidate_is_frozen_to_absolute_path() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let binary = controls.path().join(if cfg!(windows) { "perl.exe" } else { "perl" });
        fs::write(&binary, b"path candidate").map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions =
                fs::metadata(&binary).map_err(|error| error.to_string())?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&binary, permissions).map_err(|error| error.to_string())?;
        }
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let resolved =
            resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str()))?;
        let expected = fs::canonicalize(binary).map_err(|error| error.to_string())?;
        if resolved != expected {
            return Err(format!(
                "PATH candidate was not frozen absolutely: {resolved:?} != {expected:?}"
            ));
        }
        Ok(())
    }

    /// PATH lookup must continue past a regular file that Unix cannot execute.
    /// This negative control distinguishes executable eligibility from mere
    /// canonicalizability and protects the ordering contract for later PATH
    /// entries.
    #[cfg(unix)]
    #[test]
    fn path_lookup_skips_non_executable_candidate() -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;

        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let first_dir = controls.path().join("first");
        let second_dir = controls.path().join("second");
        fs::create_dir_all(&first_dir).map_err(|error| error.to_string())?;
        fs::create_dir_all(&second_dir).map_err(|error| error.to_string())?;
        let first = first_dir.join("perl");
        let second = second_dir.join("perl");
        fs::write(&first, b"non-executable path candidate").map_err(|error| error.to_string())?;
        fs::write(&second, b"executable path candidate").map_err(|error| error.to_string())?;

        let mut first_permissions =
            fs::metadata(&first).map_err(|error| error.to_string())?.permissions();
        first_permissions.set_mode(0o644);
        fs::set_permissions(&first, first_permissions).map_err(|error| error.to_string())?;
        let mut second_permissions =
            fs::metadata(&second).map_err(|error| error.to_string())?.permissions();
        second_permissions.set_mode(0o755);
        fs::set_permissions(&second, second_permissions).map_err(|error| error.to_string())?;

        let search_path =
            std::env::join_paths([&first_dir, &second_dir]).map_err(|error| error.to_string())?;
        let resolved =
            resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str()))?;
        let expected = fs::canonicalize(&second).map_err(|error| error.to_string())?;
        if resolved != expected {
            return Err(format!(
                "PATH lookup did not skip the non-executable candidate: {resolved:?} != {expected:?}"
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn path_only_candidate_honors_pathext_for_perl_exe() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let binary = controls.path().join("perl.exe");
        fs::write(&binary, b"path candidate").map_err(|error| error.to_string())?;
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let resolved =
            resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str()))?;
        let expected = fs::canonicalize(binary).map_err(|error| error.to_string())?;
        if resolved != expected {
            return Err(format!(
                "PATHEXT candidate was not resolved: {resolved:?} != {expected:?}"
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn path_candidate_with_extension_does_not_gain_pathext_suffix() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        // A faulty extension replacement would reinterpret `perl.cmd` as
        // `perl.EXE`; keep that decoy present so the negative control fails.
        let misleading = controls.path().join("perl.EXE");
        fs::write(&misleading, b"path candidate").map_err(|error| error.to_string())?;
        // A faulty PATHEXT append would instead search for `perl.cmd.EXE`;
        // keep that distinct decoy present so both regressions are caught.
        let pathext_suffix = controls.path().join("perl.cmd.EXE");
        fs::write(&pathext_suffix, b"path candidate").map_err(|error| error.to_string())?;
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let error = match resolve_debuggee_candidate(
            Path::new("perl.cmd"),
            Some(search_path.as_os_str()),
        ) {
            Ok(resolved) => {
                return Err(format!("extended candidate unexpectedly resolved to {resolved:?}"));
            }
            Err(error) => error,
        };
        if !error.contains("was not found") {
            return Err(format!("unexpected failure reason: {error}"));
        }
        Ok(())
    }

    #[test]
    fn uncached_resolver_continues_after_a_failed_path_probe() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let first_dir = controls.path().join("first");
        let second_dir = controls.path().join("second");
        fs::create_dir_all(&first_dir).map_err(|error| error.to_string())?;
        fs::create_dir_all(&second_dir).map_err(|error| error.to_string())?;
        let first = first_dir.join(if cfg!(windows) { "perl.exe" } else { "perl" });
        let second = second_dir.join(if cfg!(windows) { "perl.exe" } else { "perl" });
        fs::write(&first, b"first candidate").map_err(|error| error.to_string())?;
        fs::write(&second, b"second candidate").map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            for candidate in [&first, &second] {
                let mut permissions =
                    fs::metadata(candidate).map_err(|error| error.to_string())?.permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(candidate, permissions).map_err(|error| error.to_string())?;
            }
        }

        let mut attempted = Vec::new();
        let search_path =
            std::env::join_paths([&first_dir, &second_dir]).map_err(|error| error.to_string())?;
        let first = fs::canonicalize(first).map_err(|error| error.to_string())?;
        let second = fs::canonicalize(second).map_err(|error| error.to_string())?;
        let resolution = super::resolve_debuggee_perl_uncached_with(
            [Path::new("perl").to_path_buf()],
            false,
            Some(search_path.as_os_str()),
            |candidate| {
                attempted.push(candidate.to_path_buf());
                if candidate == first {
                    return Err(super::ProbeFailure {
                        reason: "simulated spawn failure".to_string(),
                        transient: false,
                    });
                }
                Ok(super::DebuggeePerl {
                    binary: candidate.to_path_buf(),
                    identity: "fixture candidate".to_string(),
                })
            },
        );

        if attempted != [first.clone(), second.clone()] {
            return Err(format!("probe sequence was {attempted:?}"));
        }
        let resolved = resolution.resolved.ok_or("the later candidate should be selected")?;
        if resolved.binary != second {
            return Err(format!("unexpected selected candidate: {:?}", resolved.binary));
        }
        if resolution.diagnostics != [format!("{}: simulated spawn failure", first.display())] {
            return Err(format!("unexpected probe diagnostics: {:?}", resolution.diagnostics));
        }
        if resolution.transient_failure {
            return Err("deterministic fixture failure was marked transient".to_string());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn bare_name_lookup_uses_only_exe_as_the_implicit_extension() -> Result<(), String> {
        let bare = super::bare_name_lookup_variants(std::ffi::OsStr::new("perl"));
        if bare != [std::ffi::OsString::from("perl"), std::ffi::OsString::from("perl.exe")] {
            return Err(format!("bare-name variants were {bare:?}"));
        }

        let extended = super::bare_name_lookup_variants(std::ffi::OsStr::new("perl.cmd"));
        if extended != [std::ffi::OsString::from("perl.cmd")] {
            return Err(format!("extended-name variants were {extended:?}"));
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_pin_is_rejected_before_launch() -> Result<(), String> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::path::Path;

        let path_value = OsString::from_vec(vec![b'/', b't', b'm', b'p', 0xff]);
        let path = Path::new(&path_value);
        let error = match normalize_explicit_debuggee_pin(path) {
            Ok(_) => return Err("non-UTF-8 pin unexpectedly resolved".to_string()),
            Err(error) => error,
        };
        if !error.contains("not valid UTF-8") {
            return Err(format!("unexpected error: {error}"));
        }
        Ok(())
    }

    /// A bare name absent from the search path is a resolution failure, never
    /// an accepted candidate: the resolver must not hand the probe a value the
    /// launch boundary would later reject.
    #[test]
    fn bare_candidate_absent_from_search_path_fails_closed() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let error =
            match resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str())) {
                Ok(resolved) => {
                    return Err(format!("absent candidate unexpectedly resolved to {resolved:?}"));
                }
                Err(error) => error,
            };
        if !error.contains("was not found") {
            return Err(format!("unexpected failure reason: {error}"));
        }
        Ok(())
    }

    /// A bare candidate without a captured PATH is a resolution failure, not a
    /// relative path that can leak through to the probe or launch boundary.
    #[test]
    fn bare_candidate_without_search_path_fails_closed() -> Result<(), String> {
        let error = match resolve_debuggee_candidate(Path::new("perl"), None) {
            Ok(resolved) => {
                return Err(format!("candidate unexpectedly resolved to {resolved:?}"));
            }
            Err(error) => error,
        };
        if !error.contains("no search path is available") {
            return Err(format!("unexpected failure reason: {error}"));
        }
        Ok(())
    }

    /// A non-bare candidate that does not exist on disk must fail closed at
    /// resolution instead of reaching the probe or the launch boundary.
    #[test]
    fn absolute_candidate_missing_on_disk_fails_closed() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let absent = controls.path().join("absent-perl");
        let error = match resolve_debuggee_candidate(&absent, None) {
            Ok(resolved) => {
                return Err(format!(
                    "missing absolute candidate unexpectedly resolved to {resolved:?}"
                ));
            }
            Err(error) => error,
        };
        if !error.contains("cannot canonicalize") {
            return Err(format!("unexpected failure reason: {error}"));
        }
        Ok(())
    }
}
