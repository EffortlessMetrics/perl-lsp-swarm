//! Currentness guard binding the checked Zed launch-contract projection to the
//! live clap surface of the perllsp launcher (#11304).
//!
//! The staged Zed extension embeds `.ci/fixtures/zed-perl-upstream/
//! launch-contract.v1.json` and classifies user argv against it fail-closed.
//! This integration test derives the flag inventory from the actual parser
//! (`LspArgs`) instead of a copied list, so any product CLI drift fails here
//! before stale host evidence can be manufactured.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use clap::CommandFactory;
use perl_lsp_rs_core::runtime::launcher::{
    LaunchAction, LaunchParseError, LspArgs, TransportMode, parse_args,
};
use serde_json::Value;

const PROJECTION_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/launch-contract.v1.json";
const PROJECTION_SCHEMA_VERSION: &str = "zed_perllsp_launch_contract.v1";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    // CARGO_MANIFEST_DIR = <root>/crates/perl-lsp-rs-core; walk to the repository root.
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates_dir =
        crate_dir.parent().ok_or_else(|| "crate directory has no parent".to_string())?;
    crates_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "perl-lsp-rs-core has no repository parent".into())
}

fn load_projection(root: &Path) -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(root.join(PROJECTION_RELATIVE_PATH))?;
    Ok(serde_json::from_str(&text)?)
}

/// Projection flags are spelled with their CLI dashes; clap reports long flags bare.
fn canonical_long(flag: &str) -> String {
    flag.trim_start_matches('-').to_string()
}

fn string_array(value: &Value, pointer: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let entries = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array at `{pointer}`"))?;
    let mut strings = Vec::with_capacity(entries.len());
    for entry in entries {
        let text = entry.as_str().ok_or_else(|| format!("non-string entry at `{pointer}`"))?;
        strings.push(text.to_string());
    }
    Ok(strings)
}

#[test]
fn projection_classifies_the_live_clap_surface_exactly() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = load_projection(&root)?;

    assert_eq!(
        projection.get("schema_version").and_then(Value::as_str),
        Some(PROJECTION_SCHEMA_VERSION),
        "projection schema version drifted"
    );

    let mut command = LspArgs::command();
    command.build();

    assert!(
        command.get_subcommands().count() == 0,
        "a subcommand appeared in the perllsp CLI; classify it explicitly in the Zed \
         launch projection (protocol selector vs utility route) before shipping host evidence"
    );

    let mut canonical_flags: BTreeSet<String> = BTreeSet::new();
    let mut boolean_flags: BTreeSet<String> = BTreeSet::new();
    let mut value_flags: BTreeSet<String> = BTreeSet::new();
    for argument in command.get_arguments() {
        let Some(long) = argument.get_long() else { continue };
        let long = long.to_string();
        if argument.get_num_args().is_some_and(|range| range.takes_values()) {
            value_flags.insert(long);
        } else {
            boolean_flags.insert(long);
        }
    }
    canonical_flags.extend(boolean_flags.iter().cloned());
    canonical_flags.extend(value_flags.iter().cloned());
    assert!(
        !canonical_flags.is_empty(),
        "the perllsp CLI exposed no long options; the projection cannot be bound to it"
    );

    let required_transport = projection
        .get("required_transport_flag")
        .and_then(Value::as_str)
        .ok_or("missing required_transport_flag")?;
    let transport_key = canonical_long(required_transport);
    assert!(
        boolean_flags.contains(&transport_key),
        "required transport `{required_transport}` must remain a boolean switch in the CLI"
    );

    let admitted = projection
        .get("admitted_arguments")
        .and_then(Value::as_array)
        .ok_or("missing admitted_arguments")?;
    let mut classified: BTreeSet<String> = BTreeSet::new();
    classify_exactly_once(&mut classified, transport_key, "`required_transport_flag`")?;
    for row in admitted {
        let flag = row.get("flag").and_then(Value::as_str).ok_or("admitted row lacks flag")?;
        let takes_value = row
            .get("value")
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("admitted row `{flag}` lacks value kind"))?;
        let key = canonical_long(flag);
        let expected = if takes_value { &value_flags } else { &boolean_flags };
        assert!(
            expected.contains(&key),
            "admitted flag `{flag}` no longer matches its declared value kind in the CLI"
        );
        classify_exactly_once(&mut classified, key, &format!("admitted flag `{flag}`"))?;
    }

    for flag in string_array(&projection, "/rejected_flags")? {
        let key = canonical_long(&flag);
        assert!(
            canonical_flags.contains(&key),
            "rejected flag `{flag}` no longer exists in the CLI; update the projection"
        );
        classify_exactly_once(&mut classified, key, &format!("rejected flag `{flag}`"))?;
    }

    assert_eq!(
        classified, canonical_flags,
        "every canonical long option must be classified exactly once (transport, admitted, or rejected)"
    );

    Ok(())
}

/// Insert one classified flag, failing when the same flag is classified
/// again (review 3848275693): a flag listed twice — or in both
/// `admitted_arguments` and `rejected_flags` — must invalidate the
/// projection even though a deduplicated set would compare equal.
fn classify_exactly_once(
    classified: &mut BTreeSet<String>,
    key: String,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    if !classified.insert(key) {
        return Err(format!(
            "{label} is classified more than once; duplicate or cross-class \
             projection entries make the exactly-once claim false"
        )
        .into());
    }
    Ok(())
}

#[test]
fn duplicate_or_cross_class_classifications_are_rejected() -> Result<(), Box<dyn Error>> {
    // Review 3848275693 mutation controls: duplicate and cross-class
    // classification must be rejected, though the deduplicated final set
    // would be identical.
    let mut classified: BTreeSet<String> = BTreeSet::new();
    classify_exactly_once(&mut classified, "--stdio".to_string(), "transport seed")?;
    classify_exactly_once(&mut classified, "--log".to_string(), "admitted `--log`")?;
    assert!(
        classify_exactly_once(&mut classified, "--log".to_string(), "rejected `--log`").is_err(),
        "a flag in both admitted_arguments and rejected_flags must fail the currentness check"
    );
    assert!(
        classify_exactly_once(&mut classified, "--stdio".to_string(), "transport seed again")
            .is_err(),
        "a duplicated transport seed must fail the currentness check"
    );
    Ok(())
}

#[test]
fn rejected_short_flags_match_the_live_clap_shorts() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = load_projection(&root)?;

    let mut command = LspArgs::command();
    command.build();
    let mut live_shorts: BTreeSet<String> = BTreeSet::new();
    for argument in command.get_arguments() {
        if argument.is_positional() && argument.get_long().is_none() {
            continue;
        }
        if let Some(short) = argument.get_short() {
            live_shorts.insert(format!("-{short}"));
        }
    }

    let rejected: BTreeSet<String> =
        string_array(&projection, "/rejected_short_flags")?.into_iter().collect();
    assert_eq!(rejected, live_shorts, "short-flag surface drifted from the projection");

    Ok(())
}

#[test]
fn projection_authority_points_at_the_real_parser_source() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let projection = load_projection(&root)?;

    let source_path = projection
        .pointer("/authority/source_path")
        .and_then(Value::as_str)
        .ok_or("missing authority.source_path")?;
    let source = fs::read_to_string(root.join(source_path))?;
    assert!(
        source.contains("pub struct LspArgs"),
        "authority source `{source_path}` no longer declares LspArgs"
    );
    assert!(
        source.contains("McpAliasRejected"),
        "authority source no longer pins the retired `--mcp` alias boundary"
    );

    Ok(())
}

#[test]
fn product_parser_pins_the_lsp_mcp_boundary() -> Result<(), Box<dyn Error>> {
    assert!(
        matches!(parse_args(["perllsp", "--mcp"]), Err(LaunchParseError::McpAliasRejected)),
        "`--mcp` must stay a rejected non-product alias"
    );
    assert!(
        parse_args(["perllsp", "mcp", "--stdio"]).is_err(),
        "`perllsp mcp --stdio` is the MCP route and must not parse as an LSP launch"
    );

    let stdio_plan = parse_args(["perllsp", "--stdio"]).map_err(|e| e.to_string())?;
    assert_eq!(stdio_plan.action, LaunchAction::Run);
    assert_eq!(stdio_plan.config.transport, TransportMode::Stdio);

    // The product itself admits socket transport; rejecting it is the Zed
    // layer's stricter policy recorded in the projection, so both facts are
    // pinned here to keep the layers honest.
    let socket_plan = parse_args(["perllsp", "--socket"]).map_err(|e| e.to_string())?;
    assert_eq!(socket_plan.action, LaunchAction::Run);
    assert!(matches!(socket_plan.config.transport, TransportMode::Socket { .. }));

    Ok(())
}
