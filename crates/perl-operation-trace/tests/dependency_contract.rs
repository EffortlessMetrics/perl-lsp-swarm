//! Asserts the lower-crate dependency contract for `perl-operation-trace`.
//!
//! `perl-operation-trace` owns `operation_trace.v1` and must sit below the
//! whole product stack: no editor/LSP runtime, no DAP, no async runtime, no
//! parser or workspace crate, and — critically for this crate's ephemeral
//! identity contract — no hash-function crate (`sha2`), directly or
//! transitively. A hash dependency here would make it possible to fold an
//! `OperationId` into a digest, which is exactly what this crate's identity
//! model forbids (see `src/lib.rs`).
//!
//! This test shells out to `cargo tree --target all` and **fails closed**:
//! if the dependency graph cannot be established, the contract is unproven
//! and the test fails. A proof instrument that cannot run is not evidence of
//! compliance. `--target all` matters here: `cargo tree` defaults to the
//! host target only, so without it a target-specific normal dependency (a
//! Windows-only `sha2` edge, say) would evade both `FORBIDDEN` and the
//! "exact closure" claim on every platform except the one it was added for —
//! and the platform-specific CI job that might otherwise notice does not run
//! this integration test.

// Failing closed is the point of this file: an unavailable instrument must
// abort the test rather than return a passing verdict.
#![allow(clippy::panic)]

use std::process::Command;

/// Crates that must never appear in `perl-operation-trace`'s dependency
/// tree.
///
/// Matched against the package name parsed out of each `cargo tree` line, so
/// entries are exact names — `perl-workspace` does not match
/// `perl-workspace-core`.
const FORBIDDEN: &[&str] = &[
    // LSP/DAP/editor runtime
    "lsp-types",
    "tokio",
    "tower-lsp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    // Workspace/project model
    "perl-workspace",
    "perl-workspace-core",
    // Parser implementation
    "perl-parser",
    "perl-parser-core",
    "perl-lexer",
    "perl-ast",
    "perl-semantic-analyzer",
    "perl-semantic-facts",
    // Adjacent ephemeral-identity/subprocess crate — deliberately not a
    // dependency; this crate defines vocabulary, it does not consume any of
    // the five existing namespaces.
    "perl-subprocess-runtime",
    // Durable/content-addressed identity — a different concern (see
    // src/lib.rs's ephemeral-vs-durable contrast); depending on it would
    // blur the boundary this crate exists to keep explicit.
    "perl-source-identity",
    // This crate's own dependency closure must contain no hash function, so
    // that no API within this crate can fold an OperationId into a digest.
    // This proves this crate's closure, not that fingerprinting is
    // unreachable in any absolute sense: a caller can still hash
    // OperationId::as_wire's exposed string in safe Rust with no dependency
    // at all (see src/privacy.rs).
    "sha2",
];

/// The complete set of crates permitted in the normal dependency closure.
///
/// This is an exact allowlist, not a denylist: a new transitive dependency
/// fails the contract until it is reviewed and recorded here. A denylist
/// alone would silently admit anything nobody thought to forbid.
///
/// `serde` provides stable serialization; the remainder are its required
/// proc-macro support crates.
const PERMITTED: &[&str] = &[
    "perl-operation-trace",
    // serde
    "serde",
    "serde_core",
    "serde_derive",
    // proc-macro support for serde_derive
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

/// The exact `cargo tree` invocation this contract runs.
///
/// Named apart from `dependency_tree` so its exact contents can be pinned
/// directly by [`tree_command_args_include_edges_normal_and_target_all`]
/// without shelling out to `cargo` at all: a deleted `--target`/`all` pair,
/// or a `--edges` value other than `normal`, would otherwise only ever show
/// up as a change to what `cargo tree` happens to print on the current host
/// (which, for this crate's *current* dependency closure, `--target all`
/// does not actually change), not as a direct assertion failure.
const TREE_COMMAND_ARGS: &[&str] = &[
    "tree",
    "-p",
    "perl-operation-trace",
    "--edges",
    "normal",
    "--prefix",
    "none",
    "--target",
    "all",
];

/// Run `cargo tree` for this crate's normal (non-dev, non-build) edges.
///
/// Panics with a precise diagnostic when the instrument cannot run — the
/// contract is unproven in that case, which is a failure, not a pass.
fn dependency_tree() -> String {
    let output =
        Command::new(env!("CARGO")).args(TREE_COMMAND_ARGS).output().unwrap_or_else(|error| {
            panic!(
                "dependency contract is unproven: could not run `cargo tree` ({error}). \
                 The contract fails closed rather than skipping."
            )
        });

    assert!(
        output.status.success(),
        "dependency contract is unproven: `cargo tree` exited with {}. stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Parse the distinct package names out of `cargo tree --prefix none` output.
///
/// Each line is `<name> v<version>[ (<source>)]`; deduplicated entries are
/// rendered as `<name> v<version> (*)`. Taking the first whitespace-separated
/// token yields the package name and ignores paths, which may otherwise
/// contain crate-like substrings.
fn package_names(tree: &str) -> Vec<&str> {
    let mut names: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[test]
fn no_forbidden_dependencies() {
    let tree = dependency_tree();
    let names = package_names(&tree);

    for forbidden in FORBIDDEN {
        assert!(
            !names.contains(forbidden),
            "perl-operation-trace must not depend on `{forbidden}`, \
             but it appeared in `cargo tree`:\n{tree}"
        );
    }
}

/// The dependency closure must be exactly what has been reviewed.
///
/// This is the check that catches a genuinely new dependency — including one
/// nobody thought to add to `FORBIDDEN`.
#[test]
fn dependency_closure_is_exactly_permitted() {
    let tree = dependency_tree();
    let names = package_names(&tree);

    let unexpected: Vec<&str> =
        names.iter().copied().filter(|name| !PERMITTED.contains(name)).collect();

    assert!(
        unexpected.is_empty(),
        "perl-operation-trace gained unreviewed dependencies: {unexpected:?}\n\
         Every crate in the closure must be justified and added to `PERMITTED`.\n\
         Full tree:\n{tree}"
    );

    assert!(
        names.contains(&"perl-operation-trace"),
        "cargo tree output did not contain the crate itself; \
         the instrument is not measuring what it claims:\n{tree}"
    );
}

/// Guards the parser used by both contract assertions.
///
/// Without this, a `package_names` bug that returned nothing would make the
/// forbidden-dependency test pass vacuously.
#[test]
fn package_names_parses_cargo_tree_output() {
    let sample = "perl-operation-trace v0.1.0 (/repo/crates/perl-operation-trace)\n\
                  serde v1.0.0\n\
                  serde_derive v1.0.0 (*)\n\
                  \n";
    assert_eq!(package_names(sample), vec!["perl-operation-trace", "serde", "serde_derive"]);

    // A path containing a crate-like substring must not be read as a package.
    let tricky = "perl-operation-trace v0.1.0 (/home/dev/perl-workspace-core/checkout)\n";
    assert_eq!(package_names(tricky), vec!["perl-operation-trace"]);
}

/// `package_names` sorts then `.dedup()`s; the sample above never repeats a
/// package name, so it never actually exercises `.dedup()` collapsing a
/// true duplicate — a deleted `.dedup()` call would still pass every
/// existing test in this file (`cargo tree`'s real `--prefix none` output
/// deduplicates already-visited subtrees itself, so a live run may not
/// repeat a name either). This test supplies two literal duplicate lines
/// directly, independent of what a live `cargo tree` invocation happens to
/// produce.
#[test]
fn package_names_deduplicates_repeated_entries() {
    let sample = "perl-operation-trace v0.1.0\nserde v1.0.0\nserde v1.0.0 (*)\nserde v1.0.0 (*)\n";
    assert_eq!(package_names(sample), vec!["perl-operation-trace", "serde"]);
}

/// Pins the exact `cargo tree` invocation's arguments directly, without
/// shelling out to `cargo`: `--edges normal` (dev/build edges excluded) and
/// `--target all` (see `TREE_COMMAND_ARGS`'s doc comment for why dropping
/// `--target all` would not otherwise be caught by any test in this file).
#[test]
fn tree_command_args_include_edges_normal_and_target_all() {
    assert_eq!(
        TREE_COMMAND_ARGS,
        [
            "tree",
            "-p",
            "perl-operation-trace",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--target",
            "all"
        ]
    );
}

/// `FORBIDDEN` is this contract's core declared data; pin that a handful of
/// the specifically-motivated entries (LSP/DAP runtime, the durable-identity
/// sibling, the hash-function dependency) are actually present, rather than
/// only ever exercising the list indirectly through a live `cargo tree` run
/// that currently contains none of them (so their *absence* from the list
/// would not be caught by `no_forbidden_dependencies` at all).
#[test]
fn forbidden_list_names_the_specifically_reviewed_out_of_bounds_crates() {
    for name in
        ["tokio", "sha2", "perl-workspace", "perl-source-identity", "perl-subprocess-runtime"]
    {
        assert!(FORBIDDEN.contains(&name), "expected {name:?} in FORBIDDEN");
    }
}

/// `PERMITTED` is this contract's core declared data; pin it is *exactly*
/// the reviewed serde closure, not merely a superset that happens to contain
/// whatever a live `cargo tree` run currently prints.
#[test]
fn permitted_list_is_exactly_the_reviewed_serde_closure() {
    let mut expected = vec![
        "perl-operation-trace",
        "serde",
        "serde_core",
        "serde_derive",
        "proc-macro2",
        "quote",
        "syn",
        "unicode-ident",
    ];
    expected.sort_unstable();
    let mut actual = PERMITTED.to_vec();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}
