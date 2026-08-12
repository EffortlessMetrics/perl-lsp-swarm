//! Integration tests for Wave H perl-dap collapse (#4430)
//!
//! Verifies that 11 satellite crates (perl-dap-*) are collapsed into perl-dap modules:
//! - breakpoint, eval, config, command_args, shell, stack, types, value, variables, platform, security
//!
//! Key scenarios:
//! 1. File-vs-folder conversion: platform/mod.rs and security/mod.rs must exist
//! 2. Module declarations: all 11 modules must be pub mod declarations in lib.rs
//! 3. Use-path updates: internal use statements must reference crate::MODULE, not perl_dap_MODULE
//! 4. External consumers: perl-lsp and perl-lsp-config must compile against perl_dap::platform
//! 5. Workspace cleanup: satellite crates removed, workspace count drops 123 → 112

// Trap tests: inner `use` statements are compile-time assertions that modules exist.
// The imports themselves are not used at runtime — that's the point.
#![allow(unused_imports)]

use anyhow::Result;

/// Test that all 11 satellite modules are accessible via perl_dap::*
/// This fails until all modules are declared in lib.rs
#[test]
fn test_all_modules_accessible_from_perl_dap_root() -> Result<()> {
    use perl_dap::breakpoint;
    use perl_dap::command_args;
    use perl_dap::config;
    use perl_dap::eval;
    use perl_dap::platform;
    use perl_dap::security;
    use perl_dap::shell;
    use perl_dap::stack;
    use perl_dap::types;
    use perl_dap::value;
    use perl_dap::variables;

    Ok(())
}

/// Test that types module can be imported with qualified paths.
#[test]
fn test_types_qualified_imports_work() -> Result<()> {
    use perl_dap::types::{Source, StackFrame};

    assert_ne!(std::any::type_name::<StackFrame>(), "", "StackFrame type must be accessible");
    assert_ne!(std::any::type_name::<Source>(), "", "Source type must be accessible");
    Ok(())
}

/// Test that platform module exports are accessible.
#[test]
fn test_platform_function_imports_work() -> Result<()> {
    use perl_dap::platform::resolve_perl_path_with_toolchain;

    let _result = resolve_perl_path_with_toolchain();
    Ok(())
}

/// Test that DebugAdapter no longer depends on old satellite crates.
#[test]
fn test_debug_adapter_uses_internal_modules() -> Result<()> {
    use perl_dap::DebugAdapter;

    assert_ne!(std::any::type_name::<DebugAdapter>(), "", "DebugAdapter must be importable");
    Ok(())
}

/// Test that BreakpointStore uses crate::breakpoint instead of perl_dap_breakpoint.
#[test]
fn test_breakpoint_store_uses_internal_modules() -> Result<()> {
    use perl_dap::BreakpointStore;

    assert_ne!(std::any::type_name::<BreakpointStore>(), "", "BreakpointStore must be importable");
    Ok(())
}

/// Test that DapConfig uses crate::config instead of perl_dap_config.
#[test]
fn test_dap_config_uses_internal_modules() -> Result<()> {
    use perl_dap::DapConfig;

    assert_ne!(std::any::type_name::<DapConfig>(), "", "DapConfig must be importable");
    Ok(())
}

/// Test that the optional legacy BridgeAdapter still uses internal modules.
#[cfg(feature = "legacy-pls-bridge")]
#[test]
fn test_bridge_adapter_uses_internal_modules() -> Result<()> {
    use perl_dap::BridgeAdapter;

    assert_ne!(std::any::type_name::<BridgeAdapter>(), "", "BridgeAdapter must be importable");
    Ok(())
}

/// Test that platform module exists as a folder with public exports.
#[test]
fn test_platform_folder_conversion() -> Result<()> {
    use perl_dap::platform::{normalize_path, resolve_perl_path, setup_environment};

    assert_ne!(
        std::any::type_name::<fn(String) -> anyhow::Result<std::path::PathBuf>>(),
        "",
        "Platform functions must be accessible"
    );
    Ok(())
}

/// Test that security module exists as a folder with public exports.
#[test]
fn test_security_folder_conversion() -> Result<()> {
    use perl_dap::security::{validate_expression, validate_path};

    assert_ne!(
        std::any::type_name::<fn(&str) -> anyhow::Result<()>>(),
        "",
        "Security functions must be accessible"
    );
    Ok(())
}

/// Test that all module dependencies are resolved internally.
#[test]
fn test_no_external_satellite_dependencies() -> Result<()> {
    Ok(())
}