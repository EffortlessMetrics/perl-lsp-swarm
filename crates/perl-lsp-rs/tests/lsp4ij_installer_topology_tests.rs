use serde_json::Value;
use std::collections::BTreeSet;

const DOWNSTREAM: &str = include_str!("../../../docs/reference/downstream-dap-integrations.json");
const INSTALLER_POLICY: &str = include_str!("../../../integrations/lsp4ij/installer-policy.json");
const LSP_INSTALLER: &str = include_str!("../../../integrations/lsp4ij/perl-lsp/installer.json");
const DAP_INSTALLER: &str = include_str!("../../../integrations/lsp4ij/perl-dap/installer.json");

fn parse(input: &str) -> Value {
    serde_json::from_str(input).expect("checked JSON fixture must parse")
}

fn download<'a>(installer: &'a Value) -> &'a Value {
    installer.pointer("/run/download").expect("installer run.download")
}

fn collect_asset_patterns(value: &Value, output: &mut BTreeSet<String>) {
    match value {
        Value::String(pattern) => {
            output.insert(pattern.clone());
        }
        Value::Object(object) => {
            for child in object.values() {
                collect_asset_patterns(child, output);
            }
        }
        _ => panic!("installer asset selector must contain only nested objects and strings"),
    }
}

fn expected_pattern(triple: &str) -> String {
    let extension = if triple.contains("windows") { ".zip" } else { ".tar.gz" };
    format!("perllsp-*-{triple}{extension}")
}

#[test]
fn lsp_and_dap_installers_share_one_release_asset_selector() {
    let lsp = parse(LSP_INSTALLER);
    let dap = parse(DAP_INSTALLER);

    assert_eq!(
        download(&lsp).pointer("/github/asset"),
        download(&dap).pointer("/github/asset"),
        "LSP and DAP consume the same release archive family and must not drift independently"
    );
}

#[test]
fn managed_installer_targets_partition_the_release_contract_without_gaps() {
    let downstream = parse(DOWNSTREAM);
    let policy = parse(INSTALLER_POLICY);

    let release_targets: BTreeSet<_> = downstream
        .get("targets")
        .and_then(Value::as_array)
        .expect("downstream targets")
        .iter()
        .filter_map(|entry| entry.get("triple").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    let managed: BTreeSet<_> = policy
        .get("managed_targets")
        .and_then(Value::as_array)
        .expect("managed targets")
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();
    let external: BTreeSet<_> = policy
        .get("external_binary_targets")
        .and_then(Value::as_array)
        .expect("external targets")
        .iter()
        .filter_map(|entry| entry.get("triple").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    assert!(managed.is_disjoint(&external), "a target cannot be both managed and external-only");
    assert_eq!(
        managed.union(&external).cloned().collect::<BTreeSet<_>>(),
        release_targets,
        "every produced release target needs one explicit LSP4IJ installation disposition"
    );
    assert!(managed.contains("aarch64-pc-windows-msvc"), "native Windows ARM64 must be managed");
    assert!(external.contains("x86_64-unknown-linux-musl"));
    assert!(external.contains("aarch64-unknown-linux-musl"));
}

#[test]
fn installer_asset_patterns_match_exactly_the_managed_release_targets() {
    let policy = parse(INSTALLER_POLICY);
    let lsp = parse(LSP_INSTALLER);

    let managed: BTreeSet<_> = policy
        .get("managed_targets")
        .and_then(Value::as_array)
        .expect("managed targets")
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();

    let expected: BTreeSet<_> = managed.iter().map(|triple| expected_pattern(triple)).collect();
    let mut actual = BTreeSet::new();
    collect_asset_patterns(
        download(&lsp).pointer("/github/asset").expect("github asset selector"),
        &mut actual,
    );

    assert_eq!(actual, expected, "LSP4IJ asset patterns must be a projection of managed targets");
    assert!(
        actual.iter().all(|pattern| !pattern.contains("linux-musl")),
        "musl must remain external/manual until LSP4IJ has a proven libc discriminator"
    );
}

#[test]
fn installer_fails_closed_instead_of_falling_back_to_a_stale_release() {
    let policy = parse(INSTALLER_POLICY);
    let lsp = parse(LSP_INSTALLER);
    let dap = parse(DAP_INSTALLER);

    assert_eq!(policy.get("stale_url_fallback_allowed"), Some(&Value::Bool(false)));
    assert!(download(&lsp).get("url").is_none(), "LSP installer must not contain a static URL fallback");
    assert!(download(&dap).get("url").is_none(), "DAP installer must not contain a static URL fallback");

    let lsp_text = LSP_INSTALLER.to_ascii_lowercase();
    let dap_text = DAP_INSTALLER.to_ascii_lowercase();
    assert!(!lsp_text.contains("v0.15.0") && !dap_text.contains("v0.15.0"));
}

#[test]
fn checksum_claim_does_not_inherit_release_pipeline_verification() {
    let policy = parse(INSTALLER_POLICY);
    assert_eq!(
        policy.pointer("/checksum_manifest/file").and_then(Value::as_str),
        Some("SHA256SUMS")
    );
    assert_eq!(
        policy.pointer("/checksum_manifest/consumed_by_current_lsp4ij_installer"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        policy.pointer("/checksum_manifest/claim").and_then(Value::as_str),
        Some("not_checksum_verified_by_lsp4ij")
    );
}

#[test]
fn installed_binary_names_and_stdio_command_match_release_members() {
    let lsp = parse(LSP_INSTALLER);
    let dap = parse(DAP_INSTALLER);

    assert_eq!(download(&lsp).pointer("/output/file/name/windows").and_then(Value::as_str), Some("perllsp.exe"));
    assert_eq!(download(&lsp).pointer("/output/file/name/unix").and_then(Value::as_str), Some("perllsp"));
    assert_eq!(download(&dap).pointer("/output/file/name/windows").and_then(Value::as_str), Some("perl-dap.exe"));
    assert_eq!(download(&dap).pointer("/output/file/name/unix").and_then(Value::as_str), Some("perl-dap"));

    let lsp_command = download(&lsp)
        .pointer("/onSuccess/configureServer/command")
        .and_then(Value::as_str)
        .expect("configured LSP command");
    assert!(lsp_command.contains("${output.file.name}") && lsp_command.ends_with(" --stdio"));
}
