//! Executable ownership ledger for the state-coherent LSP runtime extraction.
//!
//! This is the inventory/freeze PR for issue #7385. It changes no product
//! behavior. Instead, it makes current runtime modules and direct LSP-crate
//! dependencies fail closed when they appear without an ownership decision.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Responsibility {
    GenericProtocol,
    GenericConnection,
    GenericCoherenceRuntime,
    ApplicationPolicy,
    PerlApplication,
    ProductComposition,
    TemporaryCoupling,
    Retire,
}

#[derive(Debug, Clone, Copy)]
struct ModuleRow {
    module: &'static str,
    responsibility: Responsibility,
    target_owner: &'static str,
    disposition: &'static str,
    migration_issue: &'static str,
}

macro_rules! module_row {
    (
        $module:literal,
        $responsibility:ident,
        $owner:literal,
        $disposition:literal,
        $issue:literal
    ) => {
        ModuleRow {
            module: $module,
            responsibility: Responsibility::$responsibility,
            target_owner: $owner,
            disposition: $disposition,
            migration_issue: $issue,
        }
    };
}

const MODULES: &[ModuleRow] = &[
    module_row!(
        "client_requests",
        TemporaryCoupling,
        "effortless-lsp + perl-lsp-rs",
        "split generic client mechanics from Perl request policy",
        "#7392"
    ),
    module_row!(
        "constructors",
        ProductComposition,
        "perl-lsp-rs",
        "retain adapter construction; move generic runtime construction",
        "#9511"
    ),
    module_row!(
        "diagnostic_debounce",
        PerlApplication,
        "perl-lsp-rs RuntimeServices",
        "retain as application worker",
        "#9508"
    ),
    module_row!(
        "diagnostics",
        PerlApplication,
        "perl-code-intelligence",
        "move semantic operation below transport",
        "#6957"
    ),
    module_row!(
        "dispatch",
        TemporaryCoupling,
        "effortless-lsp + PerlLspAdapter",
        "split generic dispatch from Perl method handling",
        "#7388"
    ),
    module_row!(
        "document_access",
        PerlApplication,
        "perl-lsp-rs DocumentStore",
        "move document ownership out of LspServer",
        "#8384"
    ),
    module_row!(
        "file_discovery",
        PerlApplication,
        "perl-lsp-rs WorkspaceServices",
        "retain workspace policy above runtime",
        "#8385"
    ),
    module_row!(
        "file_watcher_debounce",
        PerlApplication,
        "perl-lsp-rs RuntimeServices",
        "retain as application worker",
        "#9508"
    ),
    module_row!(
        "language",
        PerlApplication,
        "perl-code-intelligence + PerlLspAdapter",
        "split semantic operation from LSP projection",
        "#6957"
    ),
    module_row!(
        "latency",
        TemporaryCoupling,
        "effortless-lsp observer + product metrics",
        "split generic runtime events from product measurement",
        "#9510"
    ),
    module_row!(
        "lifecycle",
        TemporaryCoupling,
        "effortless-lsp + ClientSession",
        "split protocol lifecycle from client/product state",
        "#7390"
    ),
    module_row!(
        "notebook",
        PerlApplication,
        "perl-lsp-rs DocumentStore",
        "retain notebook document authority above runtime",
        "#8384"
    ),
    module_row!(
        "outbound",
        GenericConnection,
        "effortless-lsp",
        "extract generic admission, writer, and delivery fate",
        "#9506"
    ),
    module_row!(
        "parse_worker",
        PerlApplication,
        "perl-lsp-rs RuntimeServices",
        "retain parser worker as application service",
        "#9508"
    ),
    module_row!(
        "readiness",
        PerlApplication,
        "perl-code-intelligence + WorkspaceServices",
        "retain semantic readiness outside generic runtime",
        "#6957"
    ),
    module_row!(
        "refresh",
        TemporaryCoupling,
        "effortless-lsp client + PerlLspAdapter",
        "split generic reverse request from feature policy",
        "#7392"
    ),
    module_row!(
        "routing",
        ApplicationPolicy,
        "PerlLspAdapter",
        "provide explicit route descriptors to generic runtime",
        "#9503"
    ),
    module_row!(
        "scheduler",
        GenericCoherenceRuntime,
        "effortless-lsp",
        "extract ordered mutation/read coherence mechanics",
        "#7389"
    ),
    module_row!(
        "serving",
        GenericConnection,
        "effortless-lsp",
        "replace duplicate serving loops with one connection engine",
        "#9509"
    ),
    module_row!(
        "stream_session",
        PerlApplication,
        "perl-lsp-rs RuntimeServices",
        "retain inline-completion session policy above runtime",
        "#4418"
    ),
    module_row!(
        "symbol_extraction",
        Retire,
        "canonical semantic fact producers",
        "retire direct runtime extraction after service cutover",
        "#6957"
    ),
    module_row!(
        "test_api",
        TemporaryCoupling,
        "effortless-lsp testkit + product tests",
        "split generic gates/observations from Perl fixtures",
        "#7394"
    ),
    module_row!(
        "test_runners",
        PerlApplication,
        "Perl test service",
        "retain process/test semantics outside runtime",
        "#4776"
    ),
    module_row!(
        "text_sync",
        PerlApplication,
        "perl-lsp-rs DocumentStore + adapter",
        "retain source mutation and position policy above runtime",
        "#8384"
    ),
    module_row!(
        "timing",
        TemporaryCoupling,
        "effortless-lsp observer + product metrics",
        "split generic operation events from product timing",
        "#9510"
    ),
    module_row!(
        "types",
        GenericProtocol,
        "effortless-lsp",
        "extract generic protocol types; adapter keeps Perl policy",
        "#7386"
    ),
    module_row!(
        "window",
        ApplicationPolicy,
        "PerlLspAdapter over generic client",
        "retain method policy; consume generic client handle",
        "#7392"
    ),
    module_row!(
        "workspace",
        PerlApplication,
        "perl-lsp-rs WorkspaceServices",
        "retain workspace state and policy above runtime",
        "#8385"
    ),
    module_row!(
        "workspace_folder",
        PerlApplication,
        "perl-lsp-rs WorkspaceServices",
        "retain root lifecycle above runtime",
        "#8385"
    ),
    module_row!(
        "workspace_progress",
        PerlApplication,
        "perl-lsp-rs WorkspaceServices",
        "retain workspace progress semantics above runtime",
        "#8385"
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyDisposition {
    RetainGeneric,
    RetainGenericTest,
    CandidateSubstrate,
    MoveToPerlAdapter,
    ProductOnly,
    PerlTestOnly,
    RetireCoupling,
}

#[derive(Debug, Clone, Copy)]
struct DependencyRow {
    package: &'static str,
    disposition: DependencyDisposition,
    owner_issue: &'static str,
}

macro_rules! dependency {
    ($package:literal, $disposition:ident, $issue:literal) => {
        DependencyRow {
            package: $package,
            disposition: DependencyDisposition::$disposition,
            owner_issue: $issue,
        }
    };
}

const DEPENDENCIES: &[DependencyRow] = &[
    dependency!("anyhow", RetainGeneric, "#9291"),
    dependency!("assert_cmd", RetainGenericTest, "#9298"),
    dependency!("clap", ProductOnly, "#7216"),
    dependency!("criterion", RetainGenericTest, "#1373"),
    dependency!("insta", RetainGenericTest, "#9298"),
    dependency!("lsp-types", CandidateSubstrate, "#9360"),
    dependency!("md5", MoveToPerlAdapter, "#9511"),
    dependency!("moka", MoveToPerlAdapter, "#6957"),
    dependency!("parking_lot", RetainGeneric, "#9291"),
    dependency!("perl-ast", MoveToPerlAdapter, "#6957"),
    dependency!("perl-corpus", PerlTestOnly, "#7394"),
    dependency!("perl-dap", MoveToPerlAdapter, "#8400"),
    dependency!("perl-diagnostics", MoveToPerlAdapter, "#6957"),
    dependency!("perl-lexer", MoveToPerlAdapter, "#6957"),
    dependency!("perl-lsp-perltidy", MoveToPerlAdapter, "#6957"),
    dependency!("perl-lsp-rs", RetireCoupling, "#8401"),
    dependency!("perl-lsp-rs-core", RetireCoupling, "#7412"),
    dependency!("perl-module", MoveToPerlAdapter, "#6957"),
    dependency!("perl-parser", MoveToPerlAdapter, "#6957"),
    dependency!("perl-parser-core", MoveToPerlAdapter, "#7599"),
    dependency!("perl-pod", MoveToPerlAdapter, "#6957"),
    dependency!("perl-position-tracking", MoveToPerlAdapter, "#8617"),
    dependency!("perl-pragma", MoveToPerlAdapter, "#6957"),
    dependency!("perl-ripr-facts", MoveToPerlAdapter, "#6957"),
    dependency!("perl-semantic-analyzer", MoveToPerlAdapter, "#6957"),
    dependency!("perl-semantic-facts", MoveToPerlAdapter, "#6957"),
    dependency!("perl-subprocess-runtime", ProductOnly, "#4836"),
    dependency!("perl-symbol", MoveToPerlAdapter, "#6957"),
    dependency!("perl-tdd-support", PerlTestOnly, "#7394"),
    dependency!("perl-uri", MoveToPerlAdapter, "#8617"),
    dependency!("perl-workspace", MoveToPerlAdapter, "#6957"),
    dependency!("predicates", RetainGenericTest, "#9298"),
    dependency!("proptest", RetainGenericTest, "#9298"),
    dependency!("regex", MoveToPerlAdapter, "#9511"),
    dependency!("ropey", MoveToPerlAdapter, "#8617"),
    dependency!("rustc-hash", RetainGeneric, "#9291"),
    dependency!("serde", RetainGeneric, "#9291"),
    dependency!("serde_json", RetainGeneric, "#9291"),
    dependency!("serial_test", RetainGenericTest, "#9298"),
    dependency!("static_assertions", RetainGenericTest, "#9298"),
    dependency!("tempfile", RetainGenericTest, "#9298"),
    dependency!("thiserror", RetainGeneric, "#9291"),
    dependency!("tokio", CandidateSubstrate, "#9360"),
    dependency!("toml", ProductOnly, "#6736"),
    dependency!("tracing", RetainGeneric, "#9291"),
    dependency!("tracing-appender", ProductOnly, "#9510"),
    dependency!("tracing-subscriber", ProductOnly, "#9510"),
    dependency!("ureq", ProductOnly, "#8400"),
    dependency!("url", MoveToPerlAdapter, "#8617"),
    dependency!("uuid", RetainGeneric, "#9291"),
    dependency!("walkdir", MoveToPerlAdapter, "#8385"),
    dependency!("which", ProductOnly, "#4836"),
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

fn discover_direct_dependencies(source: &str) -> BTreeSet<String> {
    let mut in_dependency_section = false;
    let mut dependencies = BTreeSet::new();

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            in_dependency_section = section == "dependencies"
                || section == "build-dependencies"
                || section == "dev-dependencies"
                || section.ends_with(".dependencies");
            continue;
        }
        if !in_dependency_section || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((left, _)) = line.split_once('=') else {
            continue;
        };
        let package = left.trim().split('.').next().unwrap_or_default().trim();
        if !package.is_empty() {
            dependencies.insert(package.to_string());
        }
    }

    dependencies
}

fn unclassified_modules(source: &str) -> BTreeSet<String> {
    let governed: BTreeSet<_> = MODULES.iter().map(|row| row.module.to_string()).collect();
    discover_runtime_modules(source).difference(&governed).cloned().collect()
}

fn unclassified_dependencies(sources: &[&str]) -> BTreeSet<String> {
    let governed: BTreeSet<_> = DEPENDENCIES.iter().map(|row| row.package.to_string()).collect();
    sources
        .iter()
        .flat_map(|source| discover_direct_dependencies(source))
        .collect::<BTreeSet<_>>()
        .difference(&governed)
        .cloned()
        .collect()
}

#[test]
fn every_current_runtime_module_has_one_ownership_row() {
    let source = fs::read_to_string(repo_root().join("crates/perl-lsp-rs/src/runtime/mod.rs"))
        .expect("read current runtime module root");
    let discovered = discover_runtime_modules(&source);
    let governed: BTreeSet<_> = MODULES.iter().map(|row| row.module.to_string()).collect();

    assert_eq!(
        discovered, governed,
        "runtime module declarations and the #7385 ledger must move together"
    );
}

#[test]
fn every_direct_lsp_crate_dependency_has_one_disposition() {
    let root = repo_root();
    let adapter = fs::read_to_string(root.join("crates/perl-lsp-rs/Cargo.toml"))
        .expect("read perl-lsp-rs manifest");
    let core = fs::read_to_string(root.join("crates/perl-lsp-rs-core/Cargo.toml"))
        .expect("read perl-lsp-rs-core manifest");
    let discovered = [adapter.as_str(), core.as_str()]
        .iter()
        .flat_map(|source| discover_direct_dependencies(source))
        .collect::<BTreeSet<_>>();
    let governed: BTreeSet<_> = DEPENDENCIES.iter().map(|row| row.package.to_string()).collect();

    assert_eq!(
        discovered, governed,
        "direct dependency changes require an explicit extraction disposition"
    );
}

#[test]
fn ownership_rows_are_unique_complete_and_directional() {
    let mut modules = BTreeMap::new();
    for row in MODULES {
        assert!(
            modules.insert(row.module, row).is_none(),
            "duplicate module row for {}",
            row.module
        );
        assert!(!row.target_owner.trim().is_empty());
        assert!(!row.disposition.trim().is_empty());
        assert!(row.migration_issue.starts_with('#'));

        if matches!(
            row.responsibility,
            Responsibility::GenericProtocol
                | Responsibility::GenericConnection
                | Responsibility::GenericCoherenceRuntime
        ) {
            assert_eq!(row.target_owner, "effortless-lsp");
        }
        if row.responsibility == Responsibility::TemporaryCoupling {
            assert_ne!(row.disposition, "retain");
        }
    }

    let mut dependencies = BTreeMap::new();
    for row in DEPENDENCIES {
        assert!(
            dependencies.insert(row.package, row).is_none(),
            "duplicate dependency row for {}",
            row.package
        );
        assert!(row.owner_issue.starts_with('#'));
        if matches!(
            row.disposition,
            DependencyDisposition::RetainGeneric
                | DependencyDisposition::RetainGenericTest
                | DependencyDisposition::CandidateSubstrate
        ) {
            assert!(
                !row.package.starts_with("perl-"),
                "zero-Perl target graph cannot retain {}",
                row.package
            );
        }
    }
}

#[test]
fn unclassified_runtime_or_dependency_additions_are_rejected() {
    let modules = "mod serving;\nmod newly_added_runtime;\n";
    assert_eq!(unclassified_modules(modules), BTreeSet::from(["newly_added_runtime".to_string()]));

    let manifest = "[dependencies]\nserde.workspace = true\nnew-runtime-dep = \"1\"\n";
    assert_eq!(
        unclassified_dependencies(&[manifest]),
        BTreeSet::from(["new-runtime-dep".to_string()])
    );
}
