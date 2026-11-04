//! Layer-check: enforce crate dependency direction constraints.
//!
//! Leaf crates (diagnostic codes, pure analysis) must not depend on
//! higher-level crates (LSP providers, wire format). This prevents
//! circular dependency introduction and keeps the crate dependency graph clean.
//!
//! # Current rules
//!
//! - `perl-diagnostics` must NOT depend on any `perl-lsp-*` crate.
//!   Reason: perl-diagnostics is a stable kernel of diagnostic codes and types.
//!   LSP-specific formatting belongs in the LSP provider layer, not here.
//!
//! - `perl-lsp-rs-core` must not reach the facade (`perl-lsp-rs`) or product
//!   (`perllsp`) through its normal, build, or test dependency closure (#8401).
//!
//! # Dependency scope
//!
//! The existing leaf rules check direct normal dependencies. Core also checks
//! every root dependency kind, then normal/build edges of workspace dependencies:
//! a dependency's own dev-dependencies are not part of core's build. Metadata
//! retains canonical package names (including renamed dependencies), optional
//! edges, and target-specific declarations without host/feature filtering.

use color_eyre::eyre::{Result, bail};
use std::{collections::BTreeSet, process::Command};

/// A layer constraint: `crate_name` must not depend on any crate matching `forbidden_prefix`.
struct LayerRule {
    /// The crate being constrained.
    crate_name: &'static str,
    /// Crate name prefix that is forbidden as a direct dependency.
    forbidden_prefix: &'static str,
    /// Human-readable explanation for the constraint.
    rationale: &'static str,
}

/// The complete set of layer constraints enforced by this check.
const LAYER_RULES: &[LayerRule] = &[
    LayerRule {
        crate_name: "perl-diagnostics",
        forbidden_prefix: "perl-lsp-",
        rationale: "perl-diagnostics is a stable leaf crate (diagnostic codes/types/catalog). \
                    It must not depend on LSP wire types or provider crates. \
                    LSP-specific logic belongs in the perl-lsp-* layer above it.",
    },
    LayerRule {
        crate_name: "perl-symbol",
        forbidden_prefix: "lsp-types",
        rationale: "the source taxonomy crate carries no transport/wire policy \
                    (#8794/#10794); LSP symbol-kind projection belongs to provider adapters.",
    },
    LayerRule {
        crate_name: "perl-symbol",
        forbidden_prefix: "perl-workspace",
        rationale: "workspace-symbol query policy lives above the taxonomy crate \
                    (#8794/#10794); this blocks query thresholds/matchers from \
                    re-entering perl-symbol.",
    },
    LayerRule {
        crate_name: "perl-workspace",
        forbidden_prefix: "perl-lsp-",
        rationale: "workspace_symbol_query is the provider-neutral owner of query \
                    profiles/match evidence (#10794) and stays below provider crates.",
    },
];

/// Walk the workspace declarations supplied by `cargo metadata --no-deps`.
/// This is a structural rule across all target/feature profiles, not a claim
/// about the resolved registry dependency graph.
/// A host-filtered `resolve.nodes` graph can omit a target-conditioned helper
/// back edge, so declarations remain the authority at every hop.
fn core_host_paths(packages: &[serde_json::Value]) -> Vec<String> {
    const CORE: &str = "perl-lsp-rs-core";
    const HOSTS: &[&str] = &["perl-lsp-rs", "perllsp"];
    let mut pending = vec![(CORE, CORE.to_string())];
    let mut visited = BTreeSet::new();
    let mut violations = Vec::new();

    while let Some((name, path)) = pending.pop() {
        if !visited.insert(name) {
            continue;
        }
        let Some(package) = packages.iter().find(|package| package["name"].as_str() == Some(name))
        else {
            continue;
        };
        let Some(dependencies) = package["dependencies"].as_array() else {
            continue;
        };
        for dependency in dependencies {
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            if name != CORE && kind == "dev" {
                continue;
            }
            // `name` is the package identity; `rename` is only its import alias.
            let Some(dependency_name) = dependency["name"].as_str() else {
                continue;
            };
            let next_path = format!("{path} -[{kind}]-> {dependency_name}");
            if HOSTS.contains(&dependency_name) {
                violations.push(next_path);
            } else {
                pending.push((dependency_name, next_path));
            }
        }
    }
    violations
}

/// Entry point used by `cargo xtask layer-check`.
pub fn run() -> Result<()> {
    run_with_metadata(None)
}

/// Run the layer check, optionally against synthetic metadata (for tests).
///
/// When `metadata` is `None`, shells out to `cargo metadata --no-deps` to
/// introspect the real workspace. When `Some(value)` is provided, that value
/// is used directly in place of the cargo invocation — this is how unit tests
/// exercise the rule engine without needing to mutate real `Cargo.toml` files.
pub fn run_with_metadata(metadata: Option<serde_json::Value>) -> Result<()> {
    println!("Checking crate layer constraints...");

    let metadata: serde_json::Value = match metadata {
        Some(m) => m,
        None => {
            let output = Command::new("cargo")
                .args(["metadata", "--no-deps", "--format-version=1"])
                .output()?;

            if !output.status.success() {
                bail!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
            }

            serde_json::from_slice(&output.stdout)?
        }
    };

    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("cargo metadata: expected packages array"))?;
    let mut violations: Vec<_> = core_host_paths(packages)
        .into_iter()
        .map(|path| {
            format!(
                "VIOLATION: {path}\n  Core must stay below facade/product composition, \
                 including test dependencies (#8401). Move cross-layer proof to the host."
            )
        })
        .collect();

    for rule in LAYER_RULES {
        // Find the package for this crate in metadata
        for pkg in packages {
            let name = pkg["name"].as_str().unwrap_or("");
            if name != rule.crate_name {
                continue;
            }

            // Check each dependency for forbidden prefix
            if let Some(deps) = pkg["dependencies"].as_array() {
                for dep in deps {
                    // Only enforce on normal (runtime) deps.
                    // cargo_metadata records `kind` as null for normal deps,
                    // "dev" for dev-dependencies, and "build" for build-dependencies.
                    let kind = dep["kind"].as_str();
                    if kind.is_some() {
                        continue;
                    }

                    let dep_name = dep["name"].as_str().unwrap_or("");
                    if dep_name.starts_with(rule.forbidden_prefix) {
                        violations.push(format!(
                            "VIOLATION: `{crate}` depends on `{dep}` (prefix `{prefix}` is forbidden)\n  \
                             Rationale: {rationale}",
                            crate = rule.crate_name,
                            dep = dep_name,
                            prefix = rule.forbidden_prefix,
                            rationale = rule.rationale,
                        ));
                    }
                }
            }
        }
    }

    if violations.is_empty() {
        println!("Layer check passed: all {} rule(s) satisfied.", LAYER_RULES.len() + 1);
        Ok(())
    } else {
        for v in &violations {
            eprintln!("{v}");
        }
        bail!("Layer check failed: {} violation(s) found.", violations.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn core_rejects_host_edges_for_every_kind_alias_and_target() {
        for host in ["perl-lsp-rs", "perllsp"] {
            for kind in [None, Some("dev"), Some("build")] {
                for target in [None, Some("cfg(target_os = \"non_host_platform\")")] {
                    for rename in [None, Some("fixture_alias")] {
                        let metadata = json!({"packages": [{
                            "name": "perl-lsp-rs-core",
                            "dependencies": [{
                                "name": host, "kind": kind, "target": target,
                                "rename": rename, "optional": kind != Some("dev")
                            }]
                        }]});
                        assert!(
                            run_with_metadata(Some(metadata)).is_err(),
                            "core must reject {host}, kind={kind:?}, target={target:?}, alias={rename:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn core_rejects_indirect_host_paths_through_test_helpers() {
        for kind in [None, Some("build")] {
            let packages = json!([
                {"name": "perl-lsp-rs-core", "dependencies": [
                    {"name": "test-helper", "kind": "dev", "rename": "fixture"}
                ]},
                {"name": "test-helper", "dependencies": [
                    {"name": "bridge", "kind": kind, "target": "cfg(windows)", "optional": true}
                ]},
                {"name": "bridge", "dependencies": [
                    {"name": "perl-lsp-rs", "kind": null, "rename": "host"}
                ]}
            ]);
            let paths = core_host_paths(packages.as_array().unwrap());
            assert_eq!(
                paths,
                [format!(
                    "perl-lsp-rs-core -[dev]-> test-helper -[{}]-> bridge -[normal]-> perl-lsp-rs",
                    kind.unwrap_or("normal")
                )]
            );
            assert!(run_with_metadata(Some(json!({"packages": packages}))).is_err());
        }
    }

    #[test]
    fn core_rejects_declared_transitive_back_edges_absent_from_host_resolution() {
        for target in ["cfg(unix)", "cfg(windows)"] {
            for kind in [None, Some("build")] {
                let mut metadata = json!({
                    "packages": [
                        {"name": "perl-lsp-rs-core", "dependencies": [
                            {"name": "fixture", "kind": "dev"}
                        ]},
                        {"name": "fixture", "dependencies": [
                            {"name": "perl-lsp-rs", "rename": "host_alias",
                             "kind": kind, "target": target}
                        ]},
                        {"name": "unrelated", "dependencies": [
                            {"name": "perl-lsp-rs", "rename": "other_host_alias",
                             "kind": kind, "target": target}
                        ]}
                    ],
                    // A host-filtered query can retain core -> fixture while
                    // hiding fixture -> host on this platform. The declaration
                    // still violates the cross-platform core boundary.
                    "resolve": {"nodes": [
                        {"id": "perl-lsp-rs-core", "deps": [{"pkg": "fixture"}]},
                        {"id": "fixture", "deps": []}
                    ]}
                });
                assert_eq!(
                    core_host_paths(metadata["packages"].as_array().unwrap()),
                    [format!(
                        "perl-lsp-rs-core -[dev]-> fixture -[{}]-> perl-lsp-rs",
                        kind.unwrap_or("normal")
                    )]
                );
                assert!(run_with_metadata(Some(metadata.clone())).is_err());

                // Removing only the reachable declaration must restore a pass.
                // The unrelated workspace member keeps the same target-specific
                // host edge; scanning all packages without reachability is wrong.
                metadata["packages"][1]["dependencies"] = json!([]);
                assert!(core_host_paths(metadata["packages"].as_array().unwrap()).is_empty());
                assert!(run_with_metadata(Some(metadata)).is_ok());
            }
        }
    }

    #[test]
    fn core_allows_lower_dependencies_and_unconsumed_transitive_dev_edges() {
        let metadata = json!({"packages": [
            {"name": "perl-lsp-rs-core", "dependencies": [
                {"name": "helper", "kind": "dev"},
                // The import alias must not be mistaken for package identity.
                {"name": "lower", "kind": null, "rename": "perl-lsp-rs"}
            ]},
            {"name": "helper", "dependencies": [
                {"name": "perl-lsp-rs", "kind": "dev"},
                {"name": "lower", "kind": null}
            ]},
            {"name": "lower", "dependencies": [
                {"name": "helper", "kind": null}
            ]},
            {"name": "perl-lsp-rs", "dependencies": [
                {"name": "perl-lsp-rs-core", "kind": null},
                {"name": "perl-lsp-rs-core", "kind": "dev"}
            ]},
            {"name": "unrelated", "dependencies": [
                {"name": "perl-lsp-rs", "kind": null}
            ]}
        ]});
        assert!(run_with_metadata(Some(metadata)).is_ok());
    }

    /// Sanity check: running against the real workspace must succeed.
    ///
    /// This guards against regressions — if someone reintroduces a
    /// forbidden dep inside `perl-diagnostics`, this test will fail
    /// alongside the CI gate.
    #[test]
    fn run_passes_on_clean_workspace() {
        let result = run_with_metadata(None);
        assert!(result.is_ok(), "layer-check should pass on the real workspace; got: {result:?}");
    }

    /// Synthetic metadata: `perl-diagnostics` has a dev-dependency on a
    /// `perl-lsp-*` crate. The dev-dep filter must skip it and the check
    /// must pass.
    #[test]
    fn kind_filter_skips_dev_deps() {
        let metadata = json!({
            "packages": [
                {
                    "name": "perl-diagnostics",
                    "dependencies": [
                        { "name": "perl-lsp-core", "kind": "dev" },
                        { "name": "perl-lsp-types", "kind": "dev" }
                    ]
                }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(
            result.is_ok(),
            "dev-deps crossing layer boundaries should be allowed; got: {result:?}"
        );
    }

    /// Synthetic metadata: `perl-diagnostics` has a build-dependency on a
    /// `perl-lsp-*` crate. Build-deps (used by build scripts) may cross layers
    /// freely, so the check must pass.
    #[test]
    fn kind_filter_skips_build_deps() {
        let metadata = json!({
            "packages": [
                {
                    "name": "perl-diagnostics",
                    "dependencies": [
                        { "name": "perl-lsp-core", "kind": "build" }
                    ]
                }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(
            result.is_ok(),
            "build-deps crossing layer boundaries should be allowed; got: {result:?}"
        );
    }

    /// Synthetic metadata: `perl-diagnostics` has a **normal** dependency on
    /// a `perl-lsp-*` crate. The check must fail.
    #[test]
    fn kind_filter_enforces_normal_deps() {
        let metadata = json!({
            "packages": [
                {
                    "name": "perl-diagnostics",
                    "dependencies": [
                        // kind: null => normal runtime dep
                        { "name": "perl-lsp-core", "kind": null }
                    ]
                }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(
            result.is_err(),
            "normal deps crossing layer boundaries must be rejected; got: {result:?}"
        );
    }

    /// #10794 recurrence rule: `perl-symbol` must not gain LSP wire types.
    #[test]
    fn perl_symbol_cannot_import_lsp_wire_types() {
        let metadata = json!({
            "packages": [
                { "name": "perl-symbol", "dependencies": [{ "name": "lsp-types", "kind": null }] }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(result.is_err(), "lsp-types in perl-symbol must be rejected; got: {result:?}");
    }

    /// #10794 recurrence rule: query policy cannot move back into
    /// `perl-symbol` via a dependency on the workspace layer.
    #[test]
    fn perl_symbol_cannot_depend_on_workspace_query_owner() {
        let metadata = json!({
            "packages": [
                { "name": "perl-symbol", "dependencies": [{ "name": "perl-workspace", "kind": null }] }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(
            result.is_err(),
            "perl-workspace dep in perl-symbol must be rejected; got: {result:?}"
        );
    }

    /// #10794 recurrence rule: the provider-neutral query-profile owner stays
    /// below provider crates.
    #[test]
    fn perl_workspace_query_profile_owner_stays_below_providers() {
        let metadata = json!({
            "packages": [
                { "name": "perl-workspace", "dependencies": [{ "name": "perl-lsp-rs-core", "kind": null }] }
            ]
        });

        let result = run_with_metadata(Some(metadata));
        assert!(
            result.is_err(),
            "perl-lsp-* dep in perl-workspace must be rejected; got: {result:?}"
        );
    }
}
