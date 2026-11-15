//! Verification that perl-dap platform re-exports are consistent with
//! the shared perl-interpreter implementation (#19439; originally #4545).

use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn resolve_perl_path_with_toolchain_returns_same_result_from_both_crates() -> TestResult {
    let dap_result = perl_dap::platform::resolve_perl_path_with_toolchain();
    let core_result = perl_lsp_rs_core::platform::resolve_perl_path_with_toolchain();

    match (&dap_result, &core_result) {
        (Ok(dap_path), Ok(core_path)) => {
            assert_eq!(
                dap_path, core_path,
                "perl-dap and perl-lsp-rs-core should resolve the same Perl path"
            );
        }
        (Err(_), Err(_)) => {}
        _ => {
            return Err(format!(
                "perl-dap and perl-lsp-rs-core disagree: dap={dap_result:?}, core={core_result:?}"
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn detect_perlbrew_perl_returns_same_result_from_both_crates() {
    let dap_result = perl_dap::platform::detect_perlbrew_perl();
    let core_result = perl_lsp_rs_core::platform::detect_perlbrew_perl();
    assert_eq!(
        dap_result, core_result,
        "perl-dap and perl-lsp-rs-core perlbrew detection must agree"
    );
}

#[test]
fn detect_plenv_perl_returns_same_result_from_both_crates() {
    let dap_result = perl_dap::platform::detect_plenv_perl();
    let core_result = perl_lsp_rs_core::platform::detect_plenv_perl();
    assert_eq!(dap_result, core_result, "perl-dap and perl-lsp-rs-core plenv detection must agree");
}

#[test]
fn resolve_perl_path_returns_same_result_from_both_crates() -> TestResult {
    let dap_result = perl_dap::platform::resolve_perl_path();
    let core_result = perl_lsp_rs_core::platform::resolve_perl_path();

    match (&dap_result, &core_result) {
        (Ok(dap_path), Ok(core_path)) => {
            assert_eq!(
                dap_path, core_path,
                "perl-dap and perl-lsp-rs-core PATH resolution must agree"
            );
        }
        (Err(_), Err(_)) => {}
        _ => {
            return Err(format!(
                "perl-dap and perl-lsp-rs-core disagree: dap={dap_result:?}, core={core_result:?}"
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn dap_platform_re_exports_are_function_identical_not_just_name_identical() {
    let dap_fn: fn() -> anyhow::Result<PathBuf> =
        perl_dap::platform::resolve_perl_path_with_toolchain;
    let core_fn: fn() -> anyhow::Result<PathBuf> =
        perl_lsp_rs_core::platform::resolve_perl_path_with_toolchain;
    let canonical_fn: fn() -> anyhow::Result<PathBuf> =
        perl_interpreter::resolve_perl_path_with_toolchain;
    assert_eq!(
        dap_fn as usize, core_fn as usize,
        "re-export should point to the same function, not a wrapper"
    );
    assert_eq!(core_fn as usize, canonical_fn as usize);
}

#[test]
fn discovery_reexports_keep_canonical_result_type_and_cached_function() {
    let canonical: perl_interpreter::PerlInterpreterResult =
        perl_dap::platform::PerlInterpreterResult::ConfiguredPath(PathBuf::from("perl"));
    let through_dap: perl_dap::platform::PerlInterpreterResult = canonical;
    assert_eq!(
        through_dap,
        perl_interpreter::PerlInterpreterResult::ConfiguredPath(PathBuf::from("perl")),
    );
    let dap_fn: fn(Option<&str>) -> perl_interpreter::PerlInterpreterResult =
        perl_dap::platform::find_perl_interpreter_cached;
    let canonical_fn: fn(Option<&str>) -> perl_interpreter::PerlInterpreterResult =
        perl_interpreter::find_perl_interpreter_cached;
    assert_eq!(dap_fn as usize, canonical_fn as usize, "both paths must share the same cache");
}
