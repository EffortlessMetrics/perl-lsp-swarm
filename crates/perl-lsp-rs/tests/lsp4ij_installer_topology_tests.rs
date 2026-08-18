// Integration test: `expect()`/`panic!` carry the assertion message when a
// checked-in fixture is malformed. The workspace-wide deny is a
// production-code rule.
#![allow(clippy::expect_used, clippy::panic)]
use serde_json::Value;
use std::collections::BTreeSet;

const DOWNSTREAM: &str = include_str!("../../../docs/reference/downstream-dap-integrations.json");
const INSTALLER_POLICY: &str = include_str!("../../../integrations/lsp4ij/installer-policy.json");
const LSP_INSTALLER: &str = include_str!("../../../integrations/lsp4ij/perl-lsp/installer.json");
const DAP_INSTALLER: &str = include_str!("../../../integrations/lsp4ij/perl-dap/installer.json");
const RELEASE_WORKFLOW: &str = include_str!("../../../.github/workflows/release.yml");

fn parse(input: &str) -> Value {
    serde_json::from_str(input).expect("checked JSON fixture must parse")
}

fn download(installer: &Value) -> &Value {
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

/// Release archive identity is owned by `downstream-dap-integrations.json`, so the
/// expected LSP4IJ pattern is derived from that authority rather than restated here.
/// A wildcard replaces `{version}` because LSP4IJ selects the current release asset.
fn expected_pattern(downstream: &Value, triple: &str) -> String {
    let name_pattern = downstream
        .get("archive_name_pattern")
        .and_then(Value::as_str)
        .expect("downstream archive_name_pattern");
    let platform = downstream_platform(downstream, triple);
    let extension = downstream
        .pointer(&format!("/platforms/{platform}/ext"))
        .and_then(Value::as_str)
        .expect("downstream platform ext");

    name_pattern.replace("{version}", "*").replace("{triple}", triple).replace("{ext}", extension)
}

fn downstream_platform<'a>(downstream: &'a Value, triple: &str) -> &'a str {
    downstream
        .get("targets")
        .and_then(Value::as_array)
        .expect("downstream targets")
        .iter()
        .find(|entry| entry.get("triple").and_then(Value::as_str) == Some(triple))
        .and_then(|entry| entry.get("platform"))
        .and_then(Value::as_str)
        .expect("every managed target must exist in the release contract")
}

/// LSP4IJ selects an asset by its own OS and CPU keys, so each slot must resolve to the
/// one release triple that actually runs there. Set-level parity alone would still pass
/// if, for example, the `mac` and `unix` archives were swapped.
fn selector_slot_triples() -> [(&'static str, &'static str, &'static str); 6] {
    [
        ("windows", "x86_64", "x86_64-pc-windows-msvc"),
        ("windows", "arm64", "aarch64-pc-windows-msvc"),
        ("unix", "x86_64", "x86_64-unknown-linux-gnu"),
        ("unix", "arm64", "aarch64-unknown-linux-gnu"),
        ("mac", "x86_64", "x86_64-apple-darwin"),
        ("mac", "arm64", "aarch64-apple-darwin"),
    ]
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

/// The installer may not invent a platform. This binds the release contract this PR
/// repairs — including the Windows ARM64 row — to the workflow that actually builds the
/// archives. It still proves only that the producer is configured to emit the target;
/// #7974 remains the authority for a downloaded, extracted, executed artifact.
#[test]
fn every_release_contract_target_is_produced_by_the_release_workflow() {
    let downstream = parse(DOWNSTREAM);

    let workflow_targets: BTreeSet<&str> = RELEASE_WORKFLOW
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- target:"))
        .map(str::trim)
        .collect();
    assert!(
        !workflow_targets.is_empty(),
        "release workflow target matrix must be readable; the parser or the workflow shape drifted"
    );

    let contract_targets: BTreeSet<&str> = downstream
        .get("targets")
        .and_then(Value::as_array)
        .expect("downstream targets")
        .iter()
        .filter_map(|entry| entry.get("triple").and_then(Value::as_str))
        .collect();

    assert_eq!(
        contract_targets, workflow_targets,
        "the downstream release contract and the release workflow must describe one target set"
    );
}

#[test]
fn installer_asset_patterns_match_exactly_the_managed_release_targets() {
    let downstream = parse(DOWNSTREAM);
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

    let expected: BTreeSet<_> =
        managed.iter().map(|triple| expected_pattern(&downstream, triple)).collect();
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
fn each_selector_slot_resolves_to_its_own_native_release_target() {
    let downstream = parse(DOWNSTREAM);
    let policy = parse(INSTALLER_POLICY);
    let lsp = parse(LSP_INSTALLER);
    let dap = parse(DAP_INSTALLER);

    let managed: BTreeSet<_> = policy
        .get("managed_targets")
        .and_then(Value::as_array)
        .expect("managed targets")
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();

    for (os, arch, triple) in selector_slot_triples() {
        assert!(
            managed.contains(triple),
            "selector slot {os}/{arch} names {triple}, which the policy does not manage"
        );
        let expected = expected_pattern(&downstream, triple);
        for (installer, label) in [(&lsp, "LSP"), (&dap, "DAP")] {
            assert_eq!(
                download(installer)
                    .pointer(&format!("/github/asset/{os}/{arch}"))
                    .and_then(Value::as_str),
                Some(expected.as_str()),
                "{label} installer slot {os}/{arch} must select the native {triple} archive"
            );
        }
    }
}

#[test]
fn installers_resolve_assets_from_the_public_release_repository() {
    for (installer, label) in [(parse(LSP_INSTALLER), "LSP"), (parse(DAP_INSTALLER), "DAP")] {
        let github = download(&installer).get("github").expect("github selector");
        assert_eq!(
            github.get("owner").and_then(Value::as_str),
            Some("EffortlessMetrics"),
            "{label} installer must resolve against the public release owner"
        );
        assert_eq!(
            github.get("repository").and_then(Value::as_str),
            Some("perl-lsp"),
            "{label} installer must resolve against the public release repository, not the development repository"
        );
        assert_eq!(
            github.get("prerelease"),
            Some(&Value::Bool(false)),
            "{label} installer must not silently install a prerelease"
        );
    }
}

#[test]
fn installer_fails_closed_instead_of_falling_back_to_a_stale_release() {
    let policy = parse(INSTALLER_POLICY);
    let lsp = parse(LSP_INSTALLER);
    let dap = parse(DAP_INSTALLER);

    assert_eq!(policy.get("stale_url_fallback_allowed"), Some(&Value::Bool(false)));
    assert!(
        download(&lsp).get("url").is_none(),
        "LSP installer must not contain a static URL fallback"
    );
    assert!(
        download(&dap).get("url").is_none(),
        "DAP installer must not contain a static URL fallback"
    );

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

    // Every platform slot must be represented: an absent `mac` entry would leave macOS
    // extraction unnamed rather than fail closed.
    for (installer, stem, label) in [(&lsp, "perllsp", "LSP"), (&dap, "perl-dap", "DAP")] {
        for platform in ["windows", "unix", "mac"] {
            let expected =
                if platform == "windows" { format!("{stem}.exe") } else { stem.to_owned() };
            assert_eq!(
                download(installer)
                    .pointer(&format!("/output/file/name/{platform}"))
                    .and_then(Value::as_str),
                Some(expected.as_str()),
                "{label} installer must extract the {platform} release member {expected}"
            );
        }
        assert_eq!(
            download(installer).pointer("/output/file/executable"),
            Some(&Value::Bool(true)),
            "{label} installer must mark the extracted member executable"
        );
    }

    // Both members ship in the same archive, so the extracted member names are what keep
    // the two installers distinct.
    assert_ne!(
        download(&lsp).pointer("/output/file/name/unix"),
        download(&dap).pointer("/output/file/name/unix"),
        "LSP and DAP share one archive and must not extract the same member"
    );

    let lsp_command = download(&lsp)
        .pointer("/onSuccess/configureServer/command")
        .and_then(Value::as_str)
        .expect("configured LSP command");
    assert!(lsp_command.contains("${output.file.name}") && lsp_command.ends_with(" --stdio"));

    // The DAP adapter speaks DAP over stdio without the LSP `--stdio` flag; asserting the
    // absence keeps a copied LSP command from silently reaching the debugger surface.
    let dap_command = download(&dap)
        .pointer("/onSuccess/configureServer/command")
        .and_then(Value::as_str)
        .expect("configured DAP command");
    assert!(
        dap_command.contains("${output.file.name}") && !dap_command.contains("--stdio"),
        "DAP command must launch the extracted adapter without the LSP stdio flag"
    );
}
