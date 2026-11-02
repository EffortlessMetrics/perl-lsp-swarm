//! Fail-closed proof that classified test-only protocol routes stay out of
//! ordinary product dispatch/capability surfaces (#9207).
//!
//! Class inventory lives in `policy/test-api-protocol-routes.v1.json`. The
//! activation class checker (#9205) validates harness/profile rows; this
//! module proves ordinary product binaries do not advertise or accept those
//! wire methods.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::ActivationError;

pub const CONTROLLING_ISSUE: &str = "#9207";
pub const INVENTORY_PATH: &str = "policy/test-api-protocol-routes.v1.json";

const ROUTING_PATH: &str = "crates/perl-lsp-rs/src/runtime/dispatch/routing.rs";
const PRODUCTION_CAPABILITIES: &str =
    "crates/perl-lsp-rs/tests/snapshots/production_capabilities.json";
const GA_LOCK_CAPABILITIES: &str = "crates/perl-lsp-rs/tests/snapshots/ga_lock_capabilities.json";

/// Expected cfg gate immediately preceding a test-only routing arm.
pub const TEST_API_CFG_GATE: &str = "#[cfg(any(test, feature = \"expose_lsp_test_api\"))]";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TestApiProtocolInventory {
    pub schema_version: u64,
    pub controlling_issue: String,
    pub routes: Vec<TestApiProtocolRoute>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TestApiProtocolRoute {
    pub surface_id: String,
    pub method: String,
    pub harness_profiles: Vec<String>,
    pub harness_consumers: Vec<String>,
    pub routing_authority: String,
    pub method_constant: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub passed: usize,
    pub failed: usize,
    pub violations: Vec<String>,
}

impl CheckReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failed == 0
    }
}

pub fn load_inventory(root: &Path) -> Result<TestApiProtocolInventory, ActivationError> {
    let bytes = fs::read(root.join(INVENTORY_PATH))
        .map_err(|error| ActivationError::new(format!("{INVENTORY_PATH}: cannot read: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ActivationError::new(format!("{INVENTORY_PATH}: invalid JSON: {error}")))
}

pub fn check(root: &Path) -> Result<CheckReport, ActivationError> {
    let inventory = load_inventory(root)?;
    Ok(evaluate(root, &inventory))
}

#[must_use]
pub fn evaluate(root: &Path, inventory: &TestApiProtocolInventory) -> CheckReport {
    let mut violations = Vec::new();

    for route in &inventory.routes {
        violations.extend(check_route_row(root, route));
    }

    let routing = read_repo_file(root, ROUTING_PATH);
    violations.extend(check_routing_completeness(&routing, inventory));
    violations.extend(check_product_capability_snapshots(root, inventory));

    let failed = violations.len();
    let passed = if failed == 0 { inventory.routes.len() + 2 } else { 0 };
    CheckReport { passed, failed, violations }
}

fn check_route_row(root: &Path, route: &TestApiProtocolRoute) -> Vec<String> {
    let mut violations = Vec::new();

    if route.harness_profiles.is_empty() {
        violations.push(format!(
            "{}: test-only protocol route requires at least one harness profile",
            route.surface_id
        ));
    }
    if route.harness_consumers.iter().all(|consumer| consumer.trim().is_empty()) {
        violations.push(format!(
            "{}: test-only protocol route requires a named harness consumer",
            route.surface_id
        ));
    }

    for consumer in &route.harness_consumers {
        let path = root.join(consumer);
        if !path.is_file() {
            violations
                .push(format!("{}: harness consumer `{}` is missing", route.surface_id, consumer));
        }
    }

    let routing_path = route.routing_authority.as_str();
    let routing = read_repo_file(root, routing_path);
    violations.extend(check_routing_for_method(&routing, &route.method, routing_path));

    let method_registry =
        read_repo_file(root, "crates/perl-lsp-rs/src/protocol/method_direction.rs");
    if !method_registry.contains(&format!("\"{}\"", route.method)) {
        violations.push(format!(
            "{}: method `{}` must stay classified in method_direction.rs",
            route.surface_id, route.method
        ));
    }

    let constant_path = route.method_constant.split('#').next().unwrap_or("");
    if !constant_path.is_empty() {
        let constants = read_repo_file(root, constant_path);
        if !constants.contains(&format!("\"{}\"", route.method)) {
            violations.push(format!(
                "{}: method constant authority `{}` must name `{}`",
                route.surface_id, route.method_constant, route.method
            ));
        }
    }

    violations
}

fn check_routing_for_method(routing: &str, method: &str, path_label: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let gated_prefix = format!("{TEST_API_CFG_GATE}\n            \"{method}\"");
    if !routing.contains(&gated_prefix) {
        violations.push(format!(
            "{path_label}: `{method}` routing arm must be immediately preceded by `{TEST_API_CFG_GATE}`"
        ));
    }

    if production_routing_contains_method(routing, method) {
        violations.push(format!(
            "{path_label}: `{method}` appears reachable from ordinary product routing \
             (missing or insufficient cfg gate)"
        ));
    }

    violations
}

pub fn check_routing_completeness(
    routing: &str,
    inventory: &TestApiProtocolInventory,
) -> Vec<String> {
    let mut violations = Vec::new();
    let mut discovered = BTreeSet::new();
    for method in discover_test_protocol_methods(routing) {
        discovered.insert(method);
    }

    let inventoried: BTreeSet<_> = inventory.routes.iter().map(|row| row.method.clone()).collect();
    for method in discovered.difference(&inventoried) {
        violations.push(format!(
            "routing table exposes unlisted test-only protocol route `{method}`; \
             add an activation inventory row in `{INVENTORY_PATH}`"
        ));
    }
    for method in inventoried.difference(&discovered) {
        violations
            .push(format!("inventory lists `{method}` but routing table no longer exposes it"));
    }
    violations
}

pub fn check_product_capability_snapshots(
    root: &Path,
    inventory: &TestApiProtocolInventory,
) -> Vec<String> {
    let mut violations = Vec::new();
    for snapshot in [PRODUCTION_CAPABILITIES, GA_LOCK_CAPABILITIES] {
        let text = read_repo_file(root, snapshot);
        for route in &inventory.routes {
            if text.contains(&route.method) {
                violations.push(format!(
                    "{snapshot}: ordinary product capability snapshot advertises `{}`",
                    route.method
                ));
            }
        }
    }
    violations
}

fn read_repo_file(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_default()
}

/// Discover `$/test/...` method literals declared on routing match arms.
fn discover_test_protocol_methods(routing: &str) -> BTreeSet<String> {
    let mut methods = BTreeSet::new();
    for line in routing.lines() {
        let code = line.split_once("//").map_or(line, |(prefix, _)| prefix);
        let trimmed = code.trim_start();
        if !trimmed.starts_with('"') && !trimmed.starts_with("| \"") {
            continue;
        }
        for literal in quoted_literals(code) {
            if literal.starts_with("$/test/") {
                methods.insert(literal.to_string());
            }
        }
    }
    methods
}

/// True when `method` appears on a routing arm that is not cfg-gated for test API.
pub fn production_routing_contains_method(routing: &str, method: &str) -> bool {
    let literal = format!("\"{method}\"");
    let mut previous_meaningful = "";
    for line in routing.lines() {
        let code = line.split_once("//").map_or(line, |(prefix, _)| prefix);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains(&literal) && is_routing_arm_head(trimmed, &literal) {
            let gated = previous_meaningful == TEST_API_CFG_GATE
                || previous_meaningful.contains("expose_lsp_test_api");
            if !gated {
                return true;
            }
        }
        previous_meaningful = trimmed;
    }
    false
}

fn is_routing_arm_head(line: &str, literal: &str) -> bool {
    let trimmed = line.trim_start();
    (trimmed.starts_with('"') || trimmed.starts_with("| \"")) && line.contains(literal)
}

fn quoted_literals(line: &str) -> Vec<&str> {
    line.split('"').enumerate().filter(|(index, _)| index % 2 == 1).map(|(_, part)| part).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_scan_ignores_cfg_gated_test_route() {
        let routing =
            format!("{TEST_API_CFG_GATE}\n            \"$/test/slowOperation\" => self.handle(),");
        assert!(!production_routing_contains_method(&routing, "$/test/slowOperation"));
    }

    #[test]
    fn production_scan_flags_ungated_test_route() {
        let routing = "\"$/test/slowOperation\" => self.handle(),";
        assert!(production_routing_contains_method(&routing, "$/test/slowOperation"));
    }
}
