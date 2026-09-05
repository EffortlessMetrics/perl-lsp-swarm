//! The `@types/vscode` compatibility fence (#13358).
//!
//! `engines.vscode` is the minimum hosted VS Code the extension claims to
//! support; `@types/vscode` is the compile-time API surface the source is
//! built against. When types float above the engines floor, a later change
//! can typecheck and pass on the current Electron host while crashing or
//! missing members on a supported older installation. That is exactly how the
//! #12207 dependabot bump became mergeable: it raised `@types/vscode` from
//! `~1.125.0` to `~1.134.0` while `engines.vscode` stayed `^1.125.0`, and
//! nothing in the repository flagged the divergence.
//!
//! The decision recorded for #13358 is the conservative one: types stay
//! aligned to the engines floor. Raising types is a support-claim change and
//! may only happen in the same candidate that deliberately moves
//! `engines.vscode` (and the minimum-host test/marketplace contract with it).
//! Both halves of that decision are enforced here:
//!
//! 1. the manifest must keep `@types/vscode` on the `engines.vscode` floor
//!    (major.minor equality), and
//! 2. dependabot must be unable to raise `@types/vscode` standalone, so the
//!    fence cannot silently reopen through an automated bump.
//!
//! Both checks parse the committed contract files rather than asserting on
//! loose prose, so any edit that breaks the fence turns these red.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

const PACKAGE_JSON: &str = "vscode-extension/package.json";
const DEPENDABOT_YML: &str = ".github/dependabot.yml";
const TYPES_DEP: &str = "@types/vscode";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn read_package_json() -> Result<Value> {
    let path = project_root().join(PACKAGE_JSON);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

/// Extract the major.minor floor of an npm range such as `^1.125.0`,
/// `~1.125.0`, or `1.125.0`.
///
/// Unknown or exotic range shapes fail closed instead of guessing, so a
/// rewritten fence cannot slip through as a false green.
fn range_floor_major_minor(range: &str) -> Result<(u64, u64)> {
    let trimmed = range.trim();
    let numeric = trimmed
        .strip_prefix('^')
        .or_else(|| trimmed.strip_prefix('~'))
        .or_else(|| trimmed.strip_prefix(">="))
        .unwrap_or(trimmed);
    let mut parts = numeric.split('.');
    let major = parts.next().ok_or_else(|| anyhow!("range `{range}` has no major component"))?;
    let minor = parts.next().ok_or_else(|| anyhow!("range `{range}` has no minor component"))?;
    let major: u64 =
        major.parse().with_context(|| format!("range `{range}` major is not numeric"))?;
    let minor: u64 =
        minor.parse().with_context(|| format!("range `{range}` minor is not numeric"))?;
    Ok((major, minor))
}

fn manifest_engines_floor() -> Result<String> {
    read_package_json()?
        .get("engines")
        .and_then(|engines| engines.get("vscode"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("`engines.vscode` must be a string in {PACKAGE_JSON}"))
}

fn manifest_types_range() -> Result<String> {
    read_package_json()?
        .get("devDependencies")
        .and_then(|deps| deps.get(TYPES_DEP))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow!("`devDependencies[\"{TYPES_DEP}\"]` must be a string in {PACKAGE_JSON}")
        })
}

/// The manifest keeps `@types/vscode` on the `engines.vscode` floor.
///
/// Mutating `@types/vscode` to any higher major.minor (replaying the #12207
/// bump) must fail here, while patch-level drift inside the same major.minor
/// stays green.
#[test]
fn types_stay_on_the_engines_floor() -> Result<()> {
    let engines = manifest_engines_floor()?;
    let types = manifest_types_range()?;
    let engines_floor = range_floor_major_minor(&engines)
        .with_context(|| format!("parsing engines.vscode range `{engines}`"))?;
    let types_floor = range_floor_major_minor(&types)
        .with_context(|| format!("parsing {TYPES_DEP} range `{types}`"))?;
    if types_floor != engines_floor {
        bail!(
            "{TYPES_DEP} `{types}` floats above the engines.vscode floor `{engines}`: \
             the types pin is the compile-time compatibility fence for the minimum \
             supported host. Raise types only in the same change that deliberately \
             moves engines.vscode and the minimum-host support contract (#13358)."
        );
    }
    Ok(())
}

fn dependabot_document() -> Result<serde_yaml_ng::Value> {
    let path = project_root().join(DEPENDABOT_YML);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

/// The dependabot rule that blocks standalone `@types/vscode` raises.
///
/// Removing or narrowing the ignore rule (for example dropping the minor
/// class so `~1.125.0 -> ~1.134.0` becomes proposable again) must fail here.
#[test]
fn dependabot_cannot_raise_types_standalone() -> Result<()> {
    let doc = dependabot_document()?;
    let updates = doc
        .get("updates")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("{DEPENDABOT_YML} must declare an `updates` sequence"))?;
    let npm_entry = updates
        .iter()
        .find(|update| {
            update.get("package-ecosystem").and_then(|value| value.as_str()) == Some("npm")
                && update.get("directory").and_then(|value| value.as_str())
                    == Some("/vscode-extension")
        })
        .ok_or_else(|| {
            anyhow!("{DEPENDABOT_YML} must keep an npm update entry for /vscode-extension")
        })?;
    let ignore =
        npm_entry.get("ignore").and_then(serde_yaml_ng::Value::as_sequence).ok_or_else(|| {
            anyhow!("the /vscode-extension npm update entry must declare an `ignore` sequence")
        })?;
    let types_rule = ignore
        .iter()
        .find(|rule| {
            rule.get("dependency-name").and_then(|value| value.as_str()) == Some(TYPES_DEP)
        })
        .ok_or_else(|| {
            anyhow!(
                "the /vscode-extension ignore list must pin `{TYPES_DEP}` so automated \
             bumps cannot float types above the engines floor (#13358)"
            )
        })?;
    let blocked = types_rule
        .get("update-types")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| {
            anyhow!("the `{TYPES_DEP}` ignore rule must declare blocked `update-types`")
        })?
        .iter()
        .filter_map(serde_yaml_ng::Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for required in ["version-update:semver-minor", "version-update:semver-major"] {
        if !blocked.iter().any(|entry| entry == required) {
            bail!(
                "the `{TYPES_DEP}` ignore rule must block `{required}` so dependabot \
                 cannot propose a types raise that leaves the engines.vscode floor \
                 behind (#13358); found {blocked:?}"
            );
        }
    }
    Ok(())
}
