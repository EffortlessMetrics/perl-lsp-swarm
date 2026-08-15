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
    layer!("client_requests", AdapterPolicy),
    layer!("constructors", ProductComposition),
    layer!("diagnostic_debounce", ApplicationServices),
    layer!("diagnostics", ApplicationServices),
    layer!("dispatch", AdapterPolicy),
    layer!("document_access", ApplicationServices),
    layer!("file_discovery", ApplicationServices),
    layer!("file_watcher_debounce", ApplicationServices),
    layer!("language", ApplicationServices),
    layer!("latency", ObservabilityTest),
    layer!("lifecycle", RuntimeProtocol),
    layer!("notebook", ApplicationServices),
    layer!("outbound", RuntimeProtocol),
    layer!("parse_worker", ApplicationServices),
    layer!("readiness", ApplicationServices),
    layer!("refresh", AdapterPolicy),
    layer!("routing", AdapterPolicy),
    layer!("scheduler", RuntimeProtocol),
    layer!("serving", RuntimeProtocol),
    layer!("stream_session", ApplicationServices),
    layer!("symbol_extraction", ApplicationServices),
    layer!("test_api", ObservabilityTest),
    layer!("test_runners", ApplicationServices),
    layer!("text_sync", ApplicationServices),
    layer!("timing", ObservabilityTest),
    layer!("types", ModelTypes),
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
        path: "crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs",
        token: "perl_parser_core",
        owner_issue: "#7599",
        removal_condition: "neutral JsonRpcError no longer implements parser ErrorClass",
    },
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
        Layer::AdapterPolicy => matches!(
            to,
            Layer::ModelTypes | Layer::ApplicationServices | Layer::AdapterPolicy
        ),
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
    TEMPORARY_EXCEPTIONS
        .iter()
        .any(|exception| exception.path == path && exception.token == token)
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

    assert_eq!(
        discovered, governed,
        "runtime modules and the #8398 layer map must move together"
    );

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
