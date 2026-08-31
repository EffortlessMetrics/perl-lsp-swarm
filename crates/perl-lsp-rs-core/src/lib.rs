#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]

/// Crate-wide test support. Shared, process-global synchronization for tests
/// that mutate environment variables — `PATH` in particular is read by
/// production code (`platform::*`, `config::perl_oracle_env`) and mutated by
/// `set_var`/`remove_var` (which Rust 2024 made `unsafe` precisely because the
/// process environment is a shared global). Every test that mutates `PATH` MUST
/// hold [`test_support::PATH_ENV_LOCK`] for the duration of the mutation +
/// restore, so all such tests serialize against the same guard rather than each
/// relying on a function-local lock that only excludes itself.
#[cfg(test)]
#[allow(unsafe_code)]
pub(crate) mod test_support {
    use std::ffi::OsString;

    /// Process-global lock serializing every `PATH`-mutating test in this crate.
    pub(crate) static PATH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores a test's process environment when the guard leaves scope.
    ///
    /// Values are captured losslessly with [`std::env::var_os`], including
    /// non-UTF-8 values on Unix. Restoration runs in reverse capture order so
    /// callers can safely snapshot several related keys.
    pub(crate) struct EnvSnapshot {
        values: Vec<(OsString, Option<OsString>)>,
    }

    impl EnvSnapshot {
        /// Capture the current values of `keys` for restoration on drop.
        pub(crate) fn capture(keys: &[&str]) -> Self {
            Self {
                values: keys
                    .iter()
                    .map(|key| (OsString::from(key), std::env::var_os(key)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in self.values.iter().rev() {
                match value {
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::EnvSnapshot;
        use std::ffi::OsString;

        static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

        #[test]
        #[serial_test::serial]
        fn restores_present_and_absent_keys() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_PRESENT", "before") };
            unsafe { std::env::remove_var("PERL_LSP_SNAPSHOT_ABSENT") };
            {
                let _snapshot = EnvSnapshot::capture(&[
                    "PERL_LSP_SNAPSHOT_PRESENT",
                    "PERL_LSP_SNAPSHOT_ABSENT",
                ]);
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_PRESENT", "during") };
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_ABSENT", "temporary") };
            }
            assert_eq!(
                std::env::var_os("PERL_LSP_SNAPSHOT_PRESENT"),
                Some(OsString::from("before"))
            );
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_ABSENT"), None);
            unsafe { std::env::remove_var("PERL_LSP_SNAPSHOT_PRESENT") };
        }

        #[test]
        #[serial_test::serial]
        fn restores_after_early_error_and_panic() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_EARLY", "before") };
            let result: Result<(), &str> = {
                let _snapshot = EnvSnapshot::capture(&["PERL_LSP_SNAPSHOT_EARLY"]);
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_EARLY", "error") };
                Err("early")
            };
            assert!(result.is_err());
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_EARLY"), Some(OsString::from("before")));
            let panic_result = std::panic::catch_unwind(|| {
                let _snapshot = EnvSnapshot::capture(&["PERL_LSP_SNAPSHOT_EARLY"]);
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_EARLY", "panic") };
                std::panic::resume_unwind(Box::new("snapshot falsifier"));
            });
            assert!(panic_result.is_err());
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_EARLY"), Some(OsString::from("before")));
            unsafe { std::env::remove_var("PERL_LSP_SNAPSHOT_EARLY") };
        }

        #[test]
        #[serial_test::serial]
        fn restores_multiple_keys_in_reverse_order() {
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_ONE", "one") };
            unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_TWO", "two") };
            {
                let _snapshot =
                    EnvSnapshot::capture(&["PERL_LSP_SNAPSHOT_ONE", "PERL_LSP_SNAPSHOT_TWO"]);
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_ONE", "changed-one") };
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_TWO", "changed-two") };
            }
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_ONE"), Some(OsString::from("one")));
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_TWO"), Some(OsString::from("two")));
            unsafe {
                std::env::remove_var("PERL_LSP_SNAPSHOT_ONE");
                std::env::remove_var("PERL_LSP_SNAPSHOT_TWO");
            }
        }

        #[cfg(unix)]
        #[test]
        #[serial_test::serial]
        fn restores_non_utf8_value() {
            use std::os::unix::ffi::OsStringExt;
            let _lock = ENV_MUTEX.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let original = OsString::from_vec(vec![b'b', 0xff]);
            unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_NON_UTF8", &original) };
            {
                let _snapshot = EnvSnapshot::capture(&["PERL_LSP_SNAPSHOT_NON_UTF8"]);
                unsafe { std::env::set_var("PERL_LSP_SNAPSHOT_NON_UTF8", "changed") };
            }
            assert_eq!(std::env::var_os("PERL_LSP_SNAPSHOT_NON_UTF8"), Some(original));
            unsafe { std::env::remove_var("PERL_LSP_SNAPSHOT_NON_UTF8") };
        }
    }
}

/// Helpers for translating feature catalog entries into client capability checks.
pub mod capability_map;
/// Runtime configuration loading, validation, and compatibility adapters.
pub mod config;
/// Checked scope/precedence/validation authority consumed by configuration generations.
#[path = "configuration_authority/checked.rs"]
pub(crate) mod configuration_authority;
/// Crate-private, versioned configuration observation model (#10813); fixture
/// producers only until #10386 consumes it.
mod configuration_observation;
/// Parser for Perl::Critic output emitted by external lint runs.
pub mod critic_parser;
/// Registry-driven, native-first external-tool doctor projection.
pub mod external_tool_doctor;
/// Canonical policy roles and native replacements for external Perl tooling.
pub mod external_tools;
/// Feature catalog parsing and generation utilities shared by build/runtime code.
pub mod feature_catalog;
/// Feature model, identifiers, and registry plumbing for capability gating.
pub mod features;
/// Policy and governance APIs for feature profiles and rollout controls.
pub mod governance;
/// Hashing helpers shared by workspace tooling and verification pipelines.
pub mod hashing;
/// Performance-focused caches and allocation strategies for large workspaces.
pub mod performance;
/// Cross-platform interpreter and toolchain detection helpers.
pub mod platform;
/// Canonical runtime product, executable, build, and artifact identity packets.
pub mod product_identity;
/// JSON-RPC and LSP protocol types used across providers and transport layers.
pub mod protocol;
/// Language Server Protocol request/notification provider implementations.
pub mod providers;
/// Request lifecycle, scheduling, and runtime orchestration infrastructure.
pub mod runtime;
/// Ticket-owned fresh-full semantic construction cell (#12151).
pub mod semantic_construction;
/// Ticket-bound immutable file semantic snapshot envelope (#12150).
pub mod semantic_snapshot;
/// Integrations for external tools such as `perlcritic` and `perltidy`.
pub mod tooling;
/// Message framing and stream transport glue for stdio/socket communication.
pub mod transport;
/// URI parsing and conversion helpers used by protocol-facing components.
pub mod uri;
