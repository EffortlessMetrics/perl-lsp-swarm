//! Internal dependency-layer contract for the LSP runtime extraction.
//!
//! Issue #8398 owns dependency direction, not module ownership or behavior.
//! The layer map is exact for current runtime modules, while generic-candidate
//! source files fail on product/domain imports unless a narrow consumptive
//! exception names its migration owner and removal condition.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    ModelTypes,
    ApplicationServices,
    AdapterPolicy,
    RuntimeProtocol,
    ProductComposition,
    ObservabilityTest,
}

#[derive(Debug, Clone, Copy)]
struct LayerRow {
    module: &'static str,
    layer: Layer,
}

macro_rules! layer {
    ($module:literal, $layer:ident) => {
        LayerRow { module: $module, layer: Layer::$layer }
    };
}

const LAYERS: &[LayerRow] = &[
    layer!("active_document_readiness_tests", ObservabilityTest),
    layer!("client_requests", AdapterPolicy),
    layer!("constructors", ProductComposition),
    layer!("diagnostic_debounce", ApplicationServices),
    layer!("diagnostics", ApplicationServices),
    layer!("diagnostics_sink", ApplicationServices),
    layer!("diagnostics_sink_tests", ObservabilityTest),
    layer!("dispatch", AdapterPolicy),
    layer!("document_access", ApplicationServices),
    layer!("document_symbols_sink", ApplicationServices),
    layer!("document_symbols_sink_tests", ObservabilityTest),
    layer!("file_discovery", ApplicationServices),
    layer!("file_watcher_debounce", ApplicationServices),
    layer!("language", ApplicationServices),
    layer!("latency", ObservabilityTest),
    layer!("lifecycle", RuntimeProtocol),
    layer!("metadata_invalidation", ApplicationServices),
    layer!("metadata_invalidation_tests", ObservabilityTest),
    layer!("notebook", ApplicationServices),
    layer!("open_buffer_authority_tests", ObservabilityTest),
    layer!("outbound", RuntimeProtocol),
    layer!("parse_effect_contract", ApplicationServices),
    layer!("parse_worker", ApplicationServices),
    layer!("readiness", ApplicationServices),
    layer!("refresh", AdapterPolicy),
    layer!("resolve_session", RuntimeProtocol),
    layer!("routing", AdapterPolicy),
    layer!("runtime_services", ApplicationServices),
    layer!("runtime_services_tests", ObservabilityTest),
    layer!("scan_gate_observation", ObservabilityTest),
    layer!("scheduler", RuntimeProtocol),
    layer!("serving", RuntimeProtocol),
    layer!("session_warning_dedup", ApplicationServices),
    layer!("session_warning_dedup_tests", ObservabilityTest),
    layer!("stream_session", ApplicationServices),
    layer!("symbol_extraction", ApplicationServices),
    layer!("test_api", ObservabilityTest),
    layer!("test_runners", ApplicationServices),
    layer!("text_sync", ApplicationServices),
    layer!("timing", ObservabilityTest),
    layer!("types", ModelTypes),
    layer!("v0_18_text_sync_envelope", ApplicationServices),
    layer!("window", AdapterPolicy),
    layer!("workspace", ApplicationServices),
    layer!("workspace_folder", ApplicationServices),
    layer!("workspace_progress", ApplicationServices),
];

#[derive(Debug, Clone, Copy)]
struct GenericCandidate {
    path: &'static str,
    layer: Layer,
}

const GENERIC_CANDIDATES: &[GenericCandidate] = &[
    GenericCandidate {
        path: "crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs",
        layer: Layer::ModelTypes,
    },
    GenericCandidate {
        path: "crates/perl-lsp-rs-core/src/transport/framing.rs",
        layer: Layer::RuntimeProtocol,
    },
    GenericCandidate {
        path: "crates/perl-lsp-rs/src/runtime/outbound.rs",
        layer: Layer::RuntimeProtocol,
    },
    GenericCandidate {
        path: "crates/perl-lsp-rs/src/runtime/scheduler.rs",
        layer: Layer::RuntimeProtocol,
    },
    GenericCandidate {
        path: "crates/perl-lsp-rs/src/runtime/serving.rs",
        layer: Layer::RuntimeProtocol,
    },
];

const FORBIDDEN_GENERIC_TOKENS: &[&str] = &[
    "LspServer",
    "crate::features",
    "crate::providers",
    "crate::runtime::language",
    "crate::runtime::types",
    "crate::runtime::workspace",
    "perl_dap",
    "perl_module",
    "perl_parser::",
    "perl_parser_core",
    "perl_semantic",
    "perl_workspace",
];

#[derive(Debug, Clone, Copy)]
struct TemporaryException {
    path: &'static str,
    token: &'static str,
    owner_issue: &'static str,
    removal_condition: &'static str,
}

const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[
    TemporaryException {
        path: "crates/perl-lsp-rs-core/src/transport/framing.rs",
        token: "perl_parser_core",
        owner_issue: "#7599",
        removal_condition: "neutral FramingError no longer implements parser ErrorClass",
    },
    TemporaryException {
        path: "crates/perl-lsp-rs/src/runtime/outbound.rs",
        token: "crate::runtime::types",
        owner_issue: "#9506",
        removal_condition: "generic outbound envelope owns its request identity",
    },
    TemporaryException {
        path: "crates/perl-lsp-rs/src/runtime/scheduler.rs",
        token: "LspServer",
        owner_issue: "#7388",
        removal_condition: "scheduler consumes only the language-neutral application port",
    },
    TemporaryException {
        path: "crates/perl-lsp-rs/src/runtime/serving.rs",
        token: "LspServer",
        owner_issue: "#9509",
        removal_condition: "one async connection engine hosts an application adapter",
    },
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live beneath the repository root")
        .to_path_buf()
}

fn discover_runtime_modules(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line
                .strip_prefix("pub(crate) mod ")
                .or_else(|| line.strip_prefix("pub mod "))
                .or_else(|| line.strip_prefix("mod "))?;
            let module = line.strip_suffix(';')?.trim();
            module
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
                .then(|| module.to_string())
        })
        .collect()
}

fn permits_dependency(from: Layer, to: Layer) -> bool {
    match from {
        Layer::ModelTypes => to == Layer::ModelTypes,
        Layer::ApplicationServices => matches!(to, Layer::ModelTypes | Layer::ApplicationServices),
        Layer::AdapterPolicy => {
            matches!(to, Layer::ModelTypes | Layer::ApplicationServices | Layer::AdapterPolicy)
        }
        Layer::RuntimeProtocol => matches!(
            to,
            Layer::ModelTypes
                | Layer::ApplicationServices
                | Layer::AdapterPolicy
                | Layer::RuntimeProtocol
        ),
        Layer::ProductComposition | Layer::ObservabilityTest => true,
    }
}

fn registered_exception(path: &str, token: &str) -> bool {
    TEMPORARY_EXCEPTIONS.iter().any(|exception| exception.path == path && exception.token == token)
}

fn code_without_line_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unregistered_forbidden_tokens(path: &str, source: &str) -> BTreeSet<String> {
    let code = code_without_line_comments(source);
    FORBIDDEN_GENERIC_TOKENS
        .iter()
        .filter(|token| code.contains(**token) && !registered_exception(path, token))
        .map(|token| (*token).to_string())
        .collect()
}

#[test]
fn every_runtime_module_has_one_layer() {
    let source = fs::read_to_string(repo_root().join("crates/perl-lsp-rs/src/runtime/mod.rs"))
        .expect("read current runtime module root");
    let discovered = discover_runtime_modules(&source);
    let governed: BTreeSet<_> = LAYERS.iter().map(|row| row.module.to_string()).collect();

    assert_eq!(discovered, governed, "runtime modules and the #8398 layer map must move together");

    let mut unique = BTreeMap::new();
    for row in LAYERS {
        assert!(
            unique.insert(row.module, row.layer).is_none(),
            "duplicate layer row for {}",
            row.module
        );
    }
}

#[test]
fn open_buffer_authority_tests_keep_the_decided_layer() {
    assert_eq!(
        LAYERS.iter().find(|row| row.module == "open_buffer_authority_tests").map(|row| row.layer),
        Some(Layer::ObservabilityTest)
    );
}

#[test]
fn layer_direction_is_positive_authority() {
    assert!(permits_dependency(Layer::ApplicationServices, Layer::ModelTypes));
    assert!(permits_dependency(Layer::AdapterPolicy, Layer::ApplicationServices));
    assert!(permits_dependency(Layer::RuntimeProtocol, Layer::AdapterPolicy));
    assert!(permits_dependency(Layer::ProductComposition, Layer::RuntimeProtocol));

    assert!(!permits_dependency(Layer::ModelTypes, Layer::RuntimeProtocol));
    assert!(!permits_dependency(Layer::ApplicationServices, Layer::AdapterPolicy));
    assert!(!permits_dependency(Layer::AdapterPolicy, Layer::RuntimeProtocol));
}

#[test]
fn generic_candidate_imports_are_clean_or_consumptively_excepted() {
    let root = repo_root();
    for candidate in GENERIC_CANDIDATES {
        assert!(matches!(candidate.layer, Layer::ModelTypes | Layer::RuntimeProtocol));
        let source = fs::read_to_string(root.join(candidate.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", candidate.path));
        let unregistered = unregistered_forbidden_tokens(candidate.path, &source);
        assert!(
            unregistered.is_empty(),
            "{} has unregistered generic-layer imports: {unregistered:?}",
            candidate.path
        );
    }
}

#[test]
fn jsonrpc_model_has_no_perl_taxonomy_escape_hatch() {
    assert!(
        !TEMPORARY_EXCEPTIONS.iter().any(|exception| {
            exception.path == "crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs"
                && exception.token == "perl_parser_core"
        }),
        "generic JsonRpcError must not regain a temporary Perl taxonomy exception"
    );
}

#[test]
fn jsonrpc_taxonomy_guard_rejects_synthetic_restoration() {
    let restored_taxonomy = r#"
use perl_parser_core::ErrorClass;

impl ErrorClass for JsonRpcError {}
"#;

    assert_eq!(
        unregistered_forbidden_tokens(
            "crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs",
            restored_taxonomy
        ),
        BTreeSet::from(["perl_parser_core".to_string()])
    );
}

#[test]
fn temporary_exceptions_are_unique_owned_and_still_consumed() {
    let root = repo_root();
    let mut unique = BTreeSet::new();
    for exception in TEMPORARY_EXCEPTIONS {
        assert!(
            unique.insert((exception.path, exception.token)),
            "duplicate exception for {} / {}",
            exception.path,
            exception.token
        );
        assert!(exception.owner_issue.starts_with('#'));
        assert!(!exception.removal_condition.trim().is_empty());
        assert_ne!(exception.token, "crate::");

        let source = fs::read_to_string(root.join(exception.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", exception.path));
        assert!(
            code_without_line_comments(&source).contains(exception.token),
            "stale exception {} / {} must be removed",
            exception.path,
            exception.token
        );
    }
}

#[test]
fn a_new_back_edge_without_an_exception_is_rejected() {
    let source = "use crate::providers::completion::Provider;\nstruct Codec;\n";
    assert_eq!(
        unregistered_forbidden_tokens("synthetic/generic.rs", source),
        BTreeSet::from(["crate::providers".to_string()])
    );
}

const CORE_FACADE_TESTS: &[&str] = &[
    "test_lsp_initialization_facade_shape",
    "test_mixed_facade_and_core_consumption",
    "test_capability_map_dual_access_complete",
    "test_perl_lsp_facade_has_all_reexports",
    "test_facade_reexports_from_perl_lsp",
    "test_type_identity_facade_vs_core",
    "test_capability_map_reexport_from_perl_lsp",
    "test_facade_no_conflicting_reexports",
    "test_capability_map_dual_access_paths",
    "test_facade_reexports_preserve_type_identity",
];

fn unconditional_tests(source: &str) -> BTreeSet<String> {
    fn gated(attributes: &[syn::Attribute]) -> bool {
        attributes.iter().any(|attribute| {
            ["cfg", "cfg_attr", "ignore"].iter().any(|name| attribute.path().is_ident(name))
        })
    }
    let file = syn::parse_file(source).expect("parse facade contract tests");
    if gated(&file.attrs) {
        return BTreeSet::new();
    }
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function)
                if !gated(&function.attrs)
                    && function.attrs.iter().any(|attribute| attribute.path().is_ident("test")) =>
            {
                Some(function.sig.ident.to_string())
            }
            _ => None,
        })
        .collect()
}

fn facade_contract_uses_libtest(manifest: &str) -> bool {
    let manifest: toml::Value = toml::from_str(manifest).expect("parse facade manifest");
    manifest.get("test").and_then(toml::Value::as_array).is_none_or(|targets| {
        targets.iter().all(|target| {
            target.get("name").and_then(toml::Value::as_str) != Some("core_facade_contract")
                || target.get("harness").and_then(toml::Value::as_bool) != Some(false)
        })
    })
}

#[test]
fn moved_core_facade_tests_remain_unconditionally_selected() {
    let root = repo_root();
    let path = root.join("crates/perl-lsp-rs/tests/core_facade_contract.rs");
    let source = fs::read_to_string(&path).expect("read facade-owned contract");
    let expected: BTreeSet<_> = CORE_FACADE_TESTS.iter().map(|name| name.to_string()).collect();
    assert!(expected.is_subset(&unconditional_tests(&source)), "all ten migrated tests must run");
    // Cargo metadata's `test: true` does not mean libtest is enabled:
    // harness=false can run an empty main while ignoring every #[test].
    let manifest = fs::read_to_string(root.join("crates/perl-lsp-rs/Cargo.toml"))
        .expect("read facade manifest");
    assert!(facade_contract_uses_libtest(&manifest), "the moved tests require the libtest harness");

    // Cargo discovery is the selection authority used by the existing required
    // unit_routed_full `cargo test --tests` gate. No feature opt-in is allowed.
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--locked", "--format-version=1"])
        .current_dir(&root)
        .output()
        .expect("run Cargo target discovery");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Cargo metadata");
    let package = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|package| package["name"] == "perl-lsp-rs")
        .expect("facade package");
    let target = package["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| target["name"] == "core_facade_contract")
        .expect("automatically discovered facade integration target");
    assert_eq!(target["kind"], serde_json::json!(["test"]));
    assert_eq!(target["test"], true);
    assert_eq!(
        Path::new(target["src_path"].as_str().expect("test source")).canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    assert!(target["required-features"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn migrated_test_selection_guard_rejects_disabled_proof() {
    let test = "#[test]\nfn facade_contract() {}\n";
    assert_eq!(unconditional_tests(test), BTreeSet::from(["facade_contract".into()]));
    for attribute in ["#[ignore]", "#[cfg(feature = \"hidden\")]", "#[cfg_attr(test, ignore)]"] {
        assert!(unconditional_tests(&format!("{attribute}\n{test}")).is_empty());
    }
    assert!(unconditional_tests(&format!("#![cfg(feature = \"hidden\")]\n{test}")).is_empty());
    assert!(unconditional_tests("fn facade_contract() {}\n").is_empty());
}

#[test]
fn migrated_test_selection_guard_rejects_disabled_harness() {
    assert!(facade_contract_uses_libtest("[package]\nname = 'perl-lsp-rs'\n"));
    let target = "[[test]]\nname = 'core_facade_contract'\n";
    assert!(facade_contract_uses_libtest(target));
    assert!(facade_contract_uses_libtest(&format!("{target}harness = true\n")));
    assert!(!facade_contract_uses_libtest(&format!("{target}harness = false\n")));
    assert!(facade_contract_uses_libtest("[[test]]\nname = 'unrelated'\nharness = false\n"));
}
