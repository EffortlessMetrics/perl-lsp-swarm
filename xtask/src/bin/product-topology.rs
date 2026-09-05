//! Validate the staged product package and executable topology.
//!
//! The checker consumes structured Cargo metadata and the reviewed
//! `policy/product-topology.toml` contract. It intentionally distinguishes
//! package absence, library admission, and product composition so an absent
//! MCP package cannot masquerade as supported MCP.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

const POLICY_PATH: &str = "policy/product-topology.toml";
const ROOT_MANIFEST_PATH: &str = "Cargo.toml";
const EXPECTED_CONTRACT: &str = "product_topology.v1";
const EXPECTED_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum McpStage {
    Absent,
    Admitted,
    Required,
}

impl fmt::Display for McpStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("absent"),
            Self::Admitted => formatter.write_str("admitted"),
            Self::Required => formatter.write_str("required"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductTopologyPolicy {
    schema_version: u32,
    contract: String,
    mcp_stage: McpStage,
    packages: PackagePolicy,
    targets: TargetPolicy,
    dependencies: DependencyPolicy,
    executables: ExecutablePolicy,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackagePolicy {
    lsp_library: String,
    transport_service: String,
    mcp_adapter: String,
    product: String,
    dap: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetPolicy {
    lsp_library: String,
    transport_service: String,
    mcp_adapter: String,
    product_library: String,
    product_binary: String,
    dap_library: String,
    dap_binary: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyPolicy {
    product_requires: Vec<String>,
    mcp_requires: Vec<String>,
    lsp_forbids: Vec<String>,
    dap_forbids: Vec<String>,
    mcp_forbids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutablePolicy {
    forbidden_names: Vec<String>,
    forbidden_prefixes: Vec<String>,
    forbidden_tokens: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    workspace_root: String,
    resolve: CargoResolve,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoResolveNode>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoResolveNode {
    id: String,
    deps: Vec<CargoResolvedDependency>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoResolvedDependency {
    pkg: String,
    dep_kinds: Vec<CargoResolvedDependencyKind>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoResolvedDependencyKind {
    kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoPackage {
    name: String,
    id: String,
    targets: Vec<CargoTarget>,
    dependencies: Vec<CargoDependency>,
    #[serde(default)]
    publish: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoDependency {
    name: String,
    kind: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    rename: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RootManifest {
    workspace: WorkspaceManifest,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkspaceManifest {
    metadata: WorkspaceMetadata,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkspaceMetadata {
    publish: PublishMetadata,
}

#[derive(Clone, Debug, Deserialize)]
struct PublishMetadata {
    allow: Vec<String>,
}

#[derive(Debug)]
struct ValidationReport {
    stage: McpStage,
    findings: Vec<String>,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("product-topology instrument failure: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run_cli() -> Result<ExitCode> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [command] if command == "check" => check_current_tree(),
        [command] if command == "--help" || command == "-h" => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            print_help();
            bail!("expected exactly one command: `check`")
        }
    }
}

fn print_help() {
    println!(
        "Validate staged product package and executable ownership.\n\n\
Usage: cargo run -p xtask --bin product-topology -- check\n\n\
Exit status:\n\
  0  topology accepted\n\
  1  topology rejected\n\
  2  policy or evidence instrument failure"
    );
}

fn check_current_tree() -> Result<ExitCode> {
    let metadata = load_cargo_metadata()?;
    let workspace_root = Path::new(&metadata.workspace_root);
    let policy_path = workspace_root.join(POLICY_PATH);
    let manifest_path = workspace_root.join(ROOT_MANIFEST_PATH);

    let policy_source = read_text(&policy_path)?;
    let policy = toml::from_str::<ProductTopologyPolicy>(&policy_source)
        .with_context(|| format!("parse {}", policy_path.display()))?;
    let manifest_source = read_text(&manifest_path)?;
    let manifest = toml::from_str::<RootManifest>(&manifest_source)
        .with_context(|| format!("parse {}", manifest_path.display()))?;

    let report = validate(&policy, &metadata, &manifest);
    if report.findings.is_empty() {
        println!(
            "product-topology: accepted stage={} product={} dap={} mcp_package={}",
            report.stage,
            policy.targets.product_binary,
            policy.targets.dap_binary,
            if report.stage == McpStage::Absent { "absent" } else { "library-only" }
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "product-topology: rejected stage={} finding_count={}",
            report.stage,
            report.findings.len()
        );
        for finding in &report.findings {
            eprintln!("{finding}");
        }
        Ok(ExitCode::FAILURE)
    }
}

fn load_cargo_metadata() -> Result<CargoMetadata> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .context("run `cargo metadata --format-version 1 --locked`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo metadata failed with status {}: {}", output.status, stderr.trim());
    }

    serde_json::from_slice(&output.stdout).context("parse Cargo metadata JSON")
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn validate(
    policy: &ProductTopologyPolicy,
    metadata: &CargoMetadata,
    manifest: &RootManifest,
) -> ValidationReport {
    let mut findings = BTreeSet::new();
    validate_policy_identity(policy, &mut findings);

    let packages = workspace_package_map(metadata, &mut findings);
    let publish_order =
        publish_order_map(&manifest.workspace.metadata.publish.allow, &mut findings);

    validate_publish_denominator(&packages, &publish_order, &mut findings);
    validate_required_packages(policy, &packages, &publish_order, &mut findings);
    validate_optional_service(policy, &packages, &publish_order, &mut findings);
    validate_mcp_stage(policy, &packages, &publish_order, &mut findings);
    validate_dependencies(policy, &packages, &metadata.resolve, &mut findings);
    validate_publish_order(policy, &packages, &publish_order, &mut findings);
    validate_reserved_executables(policy, &packages, &mut findings);

    ValidationReport { stage: policy.mcp_stage, findings: findings.into_iter().collect() }
}

fn validate_policy_identity(policy: &ProductTopologyPolicy, findings: &mut BTreeSet<String>) {
    if policy.schema_version != EXPECTED_SCHEMA_VERSION {
        findings.insert(format!(
            "policy: schema_version={} expected={EXPECTED_SCHEMA_VERSION}",
            policy.schema_version
        ));
    }
    if policy.contract != EXPECTED_CONTRACT {
        findings
            .insert(format!("policy: contract={} expected={EXPECTED_CONTRACT}", policy.contract));
    }

    let package_names = [
        policy.packages.lsp_library.as_str(),
        policy.packages.transport_service.as_str(),
        policy.packages.mcp_adapter.as_str(),
        policy.packages.product.as_str(),
        policy.packages.dap.as_str(),
    ];
    let unique_names = package_names.into_iter().collect::<BTreeSet<_>>();
    if unique_names.len() != package_names.len() {
        findings.insert("policy: governed package identities must be unique".to_owned());
    }

    if policy.targets.product_binary == policy.targets.dap_binary {
        findings.insert("policy: product and DAP binary identities must differ".to_owned());
    }
}

fn workspace_package_map<'a>(
    metadata: &'a CargoMetadata,
    findings: &mut BTreeSet<String>,
) -> BTreeMap<String, &'a CargoPackage> {
    let member_ids = metadata.workspace_members.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut packages = BTreeMap::new();
    let mut observed_member_ids = BTreeSet::new();

    for package in &metadata.packages {
        if !member_ids.contains(package.id.as_str()) {
            continue;
        }
        observed_member_ids.insert(package.id.as_str());
        if packages.insert(package.name.clone(), package).is_some() {
            findings.insert(format!(
                "cargo-metadata: duplicate workspace package name={}",
                package.name
            ));
        }
    }

    for member_id in &metadata.workspace_members {
        if !observed_member_ids.contains(member_id.as_str()) {
            findings.insert(format!(
                "cargo-metadata: workspace member id has no package row id={member_id}"
            ));
        }
    }

    packages
}

fn publish_order_map(allow: &[String], findings: &mut BTreeSet<String>) -> BTreeMap<String, usize> {
    let mut order = BTreeMap::new();
    for (index, package) in allow.iter().enumerate() {
        if order.insert(package.clone(), index).is_some() {
            findings.insert(format!("publish-order: duplicate package={package}"));
        }
    }
    order
}

fn validate_publish_denominator(
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    findings: &mut BTreeSet<String>,
) {
    for package_name in publish_order.keys() {
        let Some(package) = packages.get(package_name) else {
            findings.insert(format!(
                "publish-order: allowlist contains unknown workspace package={package_name}"
            ));
            continue;
        };
        if !is_publishable(package) {
            findings.insert(format!(
                "publish-order: allowlist contains non-publishable workspace package={package_name}"
            ));
        }
    }

    for package in packages.values().filter(|package| is_publishable(package)) {
        if !publish_order.contains_key(&package.name) {
            findings.insert(format!(
                "publish-order: publishable workspace package missing from allowlist={}",
                package.name
            ));
        }
    }
}

fn validate_required_packages(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    findings: &mut BTreeSet<String>,
) {
    validate_package_shape(
        packages,
        publish_order,
        &policy.packages.lsp_library,
        &policy.targets.lsp_library,
        &[],
        findings,
    );
    validate_package_shape(
        packages,
        publish_order,
        &policy.packages.product,
        &policy.targets.product_library,
        &[policy.targets.product_binary.as_str()],
        findings,
    );
    validate_package_shape(
        packages,
        publish_order,
        &policy.packages.dap,
        &policy.targets.dap_library,
        &[policy.targets.dap_binary.as_str()],
        findings,
    );
}

fn validate_optional_service(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    findings: &mut BTreeSet<String>,
) {
    if packages.contains_key(&policy.packages.transport_service) {
        validate_package_shape(
            packages,
            publish_order,
            &policy.packages.transport_service,
            &policy.targets.transport_service,
            &[],
            findings,
        );
    }
}

fn validate_mcp_stage(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    findings: &mut BTreeSet<String>,
) {
    let mcp_package = &policy.packages.mcp_adapter;
    match policy.mcp_stage {
        McpStage::Absent => {
            if packages.contains_key(mcp_package) {
                findings
                    .insert(format!("stage=absent: package must not exist package={mcp_package}"));
            }
            if publish_order.contains_key(mcp_package) {
                findings.insert(format!(
                    "stage=absent: publish allowlist must not contain package={mcp_package}"
                ));
            }
        }
        McpStage::Admitted | McpStage::Required => {
            if !packages.contains_key(&policy.packages.transport_service) {
                findings.insert(format!(
                    "stage={}: governed workspace package missing={}",
                    policy.mcp_stage, policy.packages.transport_service
                ));
            }
            validate_package_shape(
                packages,
                publish_order,
                mcp_package,
                &policy.targets.mcp_adapter,
                &[],
                findings,
            );
        }
    }
}

fn validate_package_shape(
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    package_name: &str,
    expected_library: &str,
    expected_binaries: &[&str],
    findings: &mut BTreeSet<String>,
) {
    let Some(package) = packages.get(package_name).copied() else {
        findings.insert(format!("package: required workspace package missing={package_name}"));
        return;
    };

    if !is_publishable(package) {
        findings.insert(format!("package: expected publishable package={package_name}"));
    }
    if !publish_order.contains_key(package_name) {
        findings.insert(format!(
            "publish-order: governed package missing from allowlist={package_name}"
        ));
    }

    let actual_libraries = target_names(package, "lib");
    let expected_libraries = [expected_library].into_iter().collect::<BTreeSet<_>>();
    if actual_libraries != expected_libraries {
        findings.insert(format!(
            "target-shape: package={package_name} library_targets={actual_libraries:?} expected={expected_libraries:?}"
        ));
    }

    let actual_binaries = target_names(package, "bin");
    let expected_binaries = expected_binaries.iter().copied().collect::<BTreeSet<_>>();
    if actual_binaries != expected_binaries {
        findings.insert(format!(
            "target-shape: package={package_name} binary_targets={actual_binaries:?} expected={expected_binaries:?}"
        ));
    }
}

fn is_publishable(package: &CargoPackage) -> bool {
    package.publish.as_ref().is_none_or(|registries| !registries.is_empty())
}

fn target_names<'a>(package: &'a CargoPackage, kind: &str) -> BTreeSet<&'a str> {
    package
        .targets
        .iter()
        .filter(|target| target.kind.iter().any(|candidate| candidate == kind))
        .map(|target| target.name.as_str())
        .collect()
}

fn validate_dependencies(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    resolve: &CargoResolve,
    findings: &mut BTreeSet<String>,
) {
    if let Some(product) = packages.get(&policy.packages.product).copied() {
        require_normal_dependencies(
            product,
            &policy.dependencies.product_requires,
            "product",
            packages,
            resolve,
            findings,
        );
        if policy.mcp_stage == McpStage::Required {
            require_normal_dependencies(
                product,
                std::slice::from_ref(&policy.packages.mcp_adapter),
                "stage=required product",
                packages,
                resolve,
                findings,
            );
        }
        if policy.mcp_stage == McpStage::Admitted {
            forbid_dependencies(
                product,
                std::slice::from_ref(&policy.packages.mcp_adapter),
                "stage=admitted product",
                findings,
            );
        }
        if policy.mcp_stage == McpStage::Absent {
            forbid_dependencies(
                product,
                std::slice::from_ref(&policy.packages.mcp_adapter),
                "stage=absent product",
                findings,
            );
        }
    }

    if let Some(lsp_library) = packages.get(&policy.packages.lsp_library).copied() {
        forbid_dependencies(lsp_library, &policy.dependencies.lsp_forbids, "LSP library", findings);
    }

    if let Some(dap) = packages.get(&policy.packages.dap).copied() {
        forbid_dependencies(dap, &policy.dependencies.dap_forbids, "DAP package", findings);
    }

    if matches!(policy.mcp_stage, McpStage::Admitted | McpStage::Required)
        && let Some(mcp_adapter) = packages.get(&policy.packages.mcp_adapter).copied()
    {
        require_normal_dependencies(
            mcp_adapter,
            &policy.dependencies.mcp_requires,
            "MCP adapter",
            packages,
            resolve,
            findings,
        );
        forbid_dependencies(mcp_adapter, &policy.dependencies.mcp_forbids, "MCP adapter", findings);
    }
}

fn require_normal_dependencies(
    package: &CargoPackage,
    required: &[String],
    owner: &str,
    workspace_packages: &BTreeMap<String, &CargoPackage>,
    resolve: &CargoResolve,
    findings: &mut BTreeSet<String>,
) {
    let resolved_node = resolve.nodes.iter().find(|node| node.id == package.id);

    for dependency in required {
        let manifest_has_normal_dependency = package.dependencies.iter().any(|candidate| {
            candidate.name == *dependency
                && candidate.kind.is_none()
                && !candidate.optional
                && candidate.rename.is_none()
                && candidate.source.is_none()
        });
        let resolves_to_workspace_package =
            workspace_packages.get(dependency).is_some_and(|workspace_package| {
                resolved_node.is_some_and(|node| {
                    node.deps.iter().any(|resolved_dependency| {
                        resolved_dependency.pkg == workspace_package.id
                            && resolved_dependency
                                .dep_kinds
                                .iter()
                                .any(|dependency_kind| dependency_kind.kind.is_none())
                    })
                })
            });

        if !manifest_has_normal_dependency || !resolves_to_workspace_package {
            findings.insert(format!(
                "dependency: {owner} package={} requires normal dependency={dependency} (non-optional, unrenamed, resolved to the governed workspace package)",
                package.name
            ));
        }
    }
}

fn forbid_dependencies(
    package: &CargoPackage,
    forbidden: &[String],
    owner: &str,
    findings: &mut BTreeSet<String>,
) {
    let dependencies = package
        .dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect::<BTreeSet<_>>();

    for dependency in forbidden {
        if dependencies.contains(dependency.as_str()) {
            findings.insert(format!(
                "dependency: {owner} package={} forbids dependency={dependency}",
                package.name
            ));
        }
    }
}

fn validate_publish_order(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    publish_order: &BTreeMap<String, usize>,
    findings: &mut BTreeSet<String>,
) {
    require_precedes(
        publish_order,
        &policy.packages.lsp_library,
        &policy.packages.product,
        findings,
    );

    if packages.contains_key(&policy.packages.transport_service) {
        require_precedes(
            publish_order,
            &policy.packages.transport_service,
            &policy.packages.product,
            findings,
        );
    }

    if matches!(policy.mcp_stage, McpStage::Admitted | McpStage::Required) {
        require_precedes(
            publish_order,
            &policy.packages.transport_service,
            &policy.packages.mcp_adapter,
            findings,
        );
        require_precedes(
            publish_order,
            &policy.packages.mcp_adapter,
            &policy.packages.product,
            findings,
        );
    }
}

fn require_precedes(
    publish_order: &BTreeMap<String, usize>,
    dependency: &str,
    consumer: &str,
    findings: &mut BTreeSet<String>,
) {
    let Some(dependency_index) = publish_order.get(dependency) else {
        findings.insert(format!("publish-order: dependency missing from allowlist={dependency}"));
        return;
    };
    let Some(consumer_index) = publish_order.get(consumer) else {
        findings.insert(format!("publish-order: consumer missing from allowlist={consumer}"));
        return;
    };
    if dependency_index >= consumer_index {
        findings.insert(format!(
            "publish-order: dependency={dependency} index={dependency_index} must precede consumer={consumer} index={consumer_index}"
        ));
    }
}

fn validate_reserved_executables(
    policy: &ProductTopologyPolicy,
    packages: &BTreeMap<String, &CargoPackage>,
    findings: &mut BTreeSet<String>,
) {
    for package in packages.values() {
        for binary in target_names(package, "bin") {
            let forbidden_name =
                policy.executables.forbidden_names.iter().any(|name| name == binary);
            let forbidden_prefix = policy
                .executables
                .forbidden_prefixes
                .iter()
                .any(|prefix| binary.starts_with(prefix));
            let forbidden_token = binary.split(['-', '_']).any(|token| {
                policy.executables.forbidden_tokens.iter().any(|candidate| candidate == token)
            });
            if forbidden_name || forbidden_prefix || forbidden_token {
                findings.insert(format!(
                    "executable: package={} reserved code-intelligence binary={binary}",
                    package.name
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str, kind: &str) -> CargoTarget {
        CargoTarget { name: name.to_owned(), kind: vec![kind.to_owned()] }
    }

    fn dependency(name: &str) -> CargoDependency {
        CargoDependency {
            name: name.to_owned(),
            kind: None,
            optional: false,
            rename: None,
            source: None,
        }
    }

    fn package(
        name: &str,
        id: &str,
        library: &str,
        binaries: &[&str],
        dependencies: &[&str],
    ) -> CargoPackage {
        let mut targets = vec![target(library, "lib")];
        targets.extend(binaries.iter().map(|binary| target(binary, "bin")));
        CargoPackage {
            name: name.to_owned(),
            id: id.to_owned(),
            targets,
            dependencies: dependencies.iter().map(|name| dependency(name)).collect(),
            publish: None,
        }
    }

    fn policy(stage: McpStage) -> ProductTopologyPolicy {
        ProductTopologyPolicy {
            schema_version: EXPECTED_SCHEMA_VERSION,
            contract: EXPECTED_CONTRACT.to_owned(),
            mcp_stage: stage,
            packages: PackagePolicy {
                lsp_library: "perl-lsp-rs".to_owned(),
                transport_service: "perl-code-intelligence".to_owned(),
                mcp_adapter: "perl-mcp".to_owned(),
                product: "perllsp".to_owned(),
                dap: "perl-dap".to_owned(),
            },
            targets: TargetPolicy {
                lsp_library: "perl_lsp".to_owned(),
                transport_service: "perl_code_intelligence".to_owned(),
                mcp_adapter: "perl_mcp".to_owned(),
                product_library: "perllsp".to_owned(),
                product_binary: "perllsp".to_owned(),
                dap_library: "perl_dap".to_owned(),
                dap_binary: "perl-dap".to_owned(),
            },
            dependencies: DependencyPolicy {
                product_requires: vec!["perl-lsp-rs".to_owned()],
                mcp_requires: vec!["perl-code-intelligence".to_owned()],
                lsp_forbids: vec!["perl-mcp".to_owned()],
                dap_forbids: vec![
                    "perllsp".to_owned(),
                    "perl-lsp-rs".to_owned(),
                    "perl-mcp".to_owned(),
                    "perl-mcp-rs".to_owned(),
                ],
                mcp_forbids: vec![
                    "perllsp".to_owned(),
                    "perl-lsp-rs".to_owned(),
                    "perl-lsp-rs-core".to_owned(),
                ],
            },
            executables: ExecutablePolicy {
                forbidden_names: vec!["perl-lsp".to_owned(), "perl-mcp".to_owned()],
                forbidden_prefixes: vec!["perl-lsp-".to_owned(), "perl-mcp-".to_owned()],
                forbidden_tokens: vec!["adapter".to_owned(), "server".to_owned()],
            },
        }
    }

    fn fixture(stage: McpStage) -> (ProductTopologyPolicy, CargoMetadata, RootManifest) {
        let mut packages = vec![
            package("perl-lsp-rs", "lsp", "perl_lsp", &[], &[]),
            package("perllsp", "product", "perllsp", &["perllsp"], &["perl-lsp-rs"]),
            package("perl-dap", "dap", "perl_dap", &["perl-dap"], &["perl-lsp-rs-core"]),
        ];
        let mut workspace_members = vec!["lsp".to_owned(), "product".to_owned(), "dap".to_owned()];
        let mut allow = vec!["perl-lsp-rs".to_owned(), "perl-dap".to_owned(), "perllsp".to_owned()];

        if matches!(stage, McpStage::Admitted | McpStage::Required) {
            packages.push(package(
                "perl-code-intelligence",
                "service",
                "perl_code_intelligence",
                &[],
                &[],
            ));
            packages.push(package("perl-mcp", "mcp", "perl_mcp", &[], &["perl-code-intelligence"]));
            workspace_members.push("service".to_owned());
            workspace_members.push("mcp".to_owned());
            allow = vec![
                "perl-lsp-rs".to_owned(),
                "perl-dap".to_owned(),
                "perl-code-intelligence".to_owned(),
                "perl-mcp".to_owned(),
                "perllsp".to_owned(),
            ];
        }

        if stage == McpStage::Required
            && let Some(product) = packages.iter_mut().find(|package| package.name == "perllsp")
        {
            product.dependencies.push(dependency("perl-mcp"));
        }

        let workspace_packages = packages
            .iter()
            .filter(|package| workspace_members.contains(&package.id))
            .map(|package| (package.name.as_str(), package.id.as_str()))
            .collect::<BTreeMap<_, _>>();
        let nodes = packages
            .iter()
            .filter(|package| workspace_members.contains(&package.id))
            .map(|package| CargoResolveNode {
                id: package.id.clone(),
                deps: package
                    .dependencies
                    .iter()
                    .filter_map(|dependency| {
                        workspace_packages.get(dependency.name.as_str()).map(|package_id| {
                            CargoResolvedDependency {
                                pkg: (*package_id).to_owned(),
                                dep_kinds: vec![CargoResolvedDependencyKind {
                                    kind: dependency.kind.clone(),
                                }],
                            }
                        })
                    })
                    .collect(),
            })
            .collect();

        (
            policy(stage),
            CargoMetadata {
                packages,
                workspace_members,
                workspace_root: ".".to_owned(),
                resolve: CargoResolve { nodes },
            },
            RootManifest {
                workspace: WorkspaceManifest {
                    metadata: WorkspaceMetadata { publish: PublishMetadata { allow } },
                },
            },
        )
    }

    fn has_finding(report: &ValidationReport, needle: &str) -> bool {
        report.findings.iter().any(|finding| finding.contains(needle))
    }

    fn require_no_findings(report: &ValidationReport) -> Result<()> {
        if report.findings.is_empty() {
            Ok(())
        } else {
            bail!("unexpected validation findings: {:?}", report.findings)
        }
    }

    fn require_finding(report: &ValidationReport, needle: &str) -> Result<()> {
        if has_finding(report, needle) {
            Ok(())
        } else {
            bail!("expected finding containing {needle:?}; got {:?}", report.findings)
        }
    }

    #[test]
    fn absent_stage_accepts_current_product_shape() -> Result<()> {
        let (policy, metadata, manifest) = fixture(McpStage::Absent);
        let report = validate(&policy, &metadata, &manifest);
        require_no_findings(&report)
    }

    #[test]
    fn retired_lsp_binary_is_rejected() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Absent);
        if let Some(package) =
            metadata.packages.iter_mut().find(|package| package.name == "perl-lsp-rs")
        {
            package.targets.push(target("perl-lsp", "bin"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "reserved code-intelligence binary=perl-lsp")
    }

    #[test]
    fn prefixed_adapter_binary_is_rejected() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Absent);
        if let Some(package) =
            metadata.packages.iter_mut().find(|package| package.name == "perllsp")
        {
            package.targets.push(target("perl-mcp-helper", "bin"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "reserved code-intelligence binary=perl-mcp-helper")
    }

    #[test]
    fn neutral_adapter_or_server_binary_in_another_package_is_rejected() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Absent);
        let mut unrelated = package(
            "unrelated-tooling",
            "unrelated",
            "unrelated_tooling",
            &["code_intelligence_server"],
            &[],
        );
        unrelated.publish = Some(Vec::new());
        metadata.packages.push(unrelated);
        metadata.workspace_members.push("unrelated".to_owned());

        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "reserved code-intelligence binary=code_intelligence_server")
    }

    #[test]
    fn absent_stage_rejects_mcp_package_presence() -> Result<()> {
        let (policy, mut metadata, mut manifest) = fixture(McpStage::Absent);
        metadata.packages.push(package(
            "perl-mcp",
            "mcp",
            "perl_mcp",
            &[],
            &["perl-code-intelligence"],
        ));
        metadata.workspace_members.push("mcp".to_owned());
        manifest.workspace.metadata.publish.allow.insert(1, "perl-mcp".to_owned());

        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "stage=absent: package must not exist package=perl-mcp")
    }

    #[test]
    fn admitted_stage_requires_mcp_package() -> Result<()> {
        let (policy, mut metadata, mut manifest) = fixture(McpStage::Admitted);
        metadata.packages.retain(|package| package.name != "perl-mcp");
        metadata.workspace_members.retain(|member| member != "mcp");
        manifest.workspace.metadata.publish.allow.retain(|package| package != "perl-mcp");

        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "required workspace package missing=perl-mcp")
    }

    #[test]
    fn admitted_stage_accepts_resolved_workspace_dependency() -> Result<()> {
        let (policy, metadata, manifest) = fixture(McpStage::Admitted);
        let report = validate(&policy, &metadata, &manifest);
        require_no_findings(&report)
    }

    #[test]
    fn admitted_or_required_stage_requires_governed_transport_workspace_package() -> Result<()> {
        for stage in [McpStage::Admitted, McpStage::Required] {
            let (policy, mut metadata, mut manifest) = fixture(stage);
            metadata.packages.retain(|package| package.name != "perl-code-intelligence");
            metadata.workspace_members.retain(|member| member != "service");
            manifest
                .workspace
                .metadata
                .publish
                .allow
                .retain(|package| package != "perl-code-intelligence");

            let report = validate(&policy, &metadata, &manifest);
            require_finding(
                &report,
                &format!(
                    "stage={stage}: governed workspace package missing=perl-code-intelligence"
                ),
            )?;
        }
        Ok(())
    }

    #[test]
    fn admitted_stage_rejects_executable_mcp_adapter() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(package) =
            metadata.packages.iter_mut().find(|package| package.name == "perl-mcp")
        {
            package.targets.push(target("perl-mcp", "bin"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "target-shape: package=perl-mcp binary_targets")?;
        require_finding(&report, "reserved code-intelligence binary=perl-mcp")
    }

    #[test]
    fn admitted_stage_rejects_wrong_publish_order() -> Result<()> {
        let (policy, metadata, mut manifest) = fixture(McpStage::Admitted);
        manifest.workspace.metadata.publish.allow = vec![
            "perl-lsp-rs".to_owned(),
            "perl-dap".to_owned(),
            "perl-mcp".to_owned(),
            "perl-code-intelligence".to_owned(),
            "perllsp".to_owned(),
        ];
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "dependency=perl-code-intelligence")
    }

    #[test]
    fn required_stage_requires_product_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Required);
        if let Some(product) =
            metadata.packages.iter_mut().find(|package| package.name == "perllsp")
        {
            product.dependencies.retain(|dependency| dependency.name != "perl-mcp");
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "stage=required product package=perllsp requires normal dependency=perl-mcp",
        )
    }

    #[test]
    fn admitted_stage_rejects_product_mcp_adapter_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(product) =
            metadata.packages.iter_mut().find(|package| package.name == "perllsp")
        {
            product.dependencies.push(dependency("perl-mcp"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "stage=admitted product package=perllsp forbids dependency=perl-mcp",
        )
    }

    #[test]
    fn admitted_stage_rejects_optional_required_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(mcp) = metadata.packages.iter_mut().find(|package| package.name == "perl-mcp") {
            if let Some(dependency) = mcp.dependencies.first_mut() {
                dependency.optional = true;
            }
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "MCP adapter package=perl-mcp requires normal dependency=perl-code-intelligence",
        )
    }

    #[test]
    fn admitted_stage_rejects_renamed_required_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(mcp) = metadata.packages.iter_mut().find(|package| package.name == "perl-mcp") {
            if let Some(dependency) = mcp.dependencies.first_mut() {
                dependency.rename = Some("transport".to_owned());
            }
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "MCP adapter package=perl-mcp requires normal dependency=perl-code-intelligence",
        )
    }

    #[test]
    fn dap_rejects_code_intelligence_dependencies() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Absent);
        if let Some(dap) = metadata.packages.iter_mut().find(|package| package.name == "perl-dap") {
            dap.dependencies.push(dependency("perllsp"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "DAP package package=perl-dap forbids dependency=perllsp")
    }

    #[test]
    fn missing_publish_order_edge_fails_closed() -> Result<()> {
        let (policy, metadata, mut manifest) = fixture(McpStage::Absent);
        manifest.workspace.metadata.publish.allow.retain(|package| package != "perl-lsp-rs");
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "dependency missing from allowlist=perl-lsp-rs")
    }

    #[test]
    fn admitted_stage_rejects_external_required_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(mcp) = metadata.packages.iter_mut().find(|package| package.name == "perl-mcp") {
            if let Some(dependency) = mcp.dependencies.first_mut() {
                dependency.source =
                    Some("registry+https://github.com/rust-lang/crates.io-index".to_owned());
            }
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "MCP adapter package=perl-mcp requires normal dependency=perl-code-intelligence",
        )
    }

    #[test]
    fn admitted_stage_rejects_same_name_non_member_path_dependency() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        let external_id = "path+file:///outside/perl-code-intelligence#0.1.0";
        metadata.packages.push(package(
            "perl-code-intelligence",
            external_id,
            "perl_code_intelligence",
            &[],
            &[],
        ));

        let Some(mcp_node) = metadata.resolve.nodes.iter_mut().find(|node| node.id == "mcp") else {
            bail!("fixture is missing the resolved perl-mcp node");
        };
        let Some(service_edge) = mcp_node.deps.iter_mut().find(|dependency| {
            dependency.pkg == "service"
                && dependency.dep_kinds.iter().any(|kind| kind.kind.is_none())
        }) else {
            bail!("fixture is missing the normal perl-mcp service dependency");
        };
        service_edge.pkg = external_id.to_owned();

        let report = validate(&policy, &metadata, &manifest);
        require_finding(
            &report,
            "MCP adapter package=perl-mcp requires normal dependency=perl-code-intelligence",
        )
    }

    #[test]
    fn lsp_library_cannot_depend_upward_on_mcp() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(lsp) =
            metadata.packages.iter_mut().find(|package| package.name == "perl-lsp-rs")
        {
            lsp.dependencies.push(dependency("perl-mcp"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "LSP library package=perl-lsp-rs forbids dependency=perl-mcp")
    }

    #[test]
    fn mcp_adapter_cannot_depend_on_lsp_runtime() -> Result<()> {
        let (policy, mut metadata, manifest) = fixture(McpStage::Admitted);
        if let Some(mcp) = metadata.packages.iter_mut().find(|package| package.name == "perl-mcp") {
            mcp.dependencies.push(dependency("perl-lsp-rs-core"));
        }
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "MCP adapter package=perl-mcp forbids dependency=perl-lsp-rs-core")
    }

    #[test]
    fn missing_publish_evidence_is_rejected() -> Result<()> {
        let (policy, metadata, mut manifest) = fixture(McpStage::Absent);
        manifest.workspace.metadata.publish.allow.retain(|package| package != "perllsp");
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "governed package missing from allowlist=perllsp")
    }

    #[test]
    fn unknown_publish_allowlist_entry_is_rejected() -> Result<()> {
        let (policy, metadata, mut manifest) = fixture(McpStage::Absent);
        manifest.workspace.metadata.publish.allow.push("unknown-package".to_owned());
        let report = validate(&policy, &metadata, &manifest);
        require_finding(&report, "allowlist contains unknown workspace package=unknown-package")
    }
}
