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
//!    (an exact or tilde stable version on the same major.minor API line), and
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

/// Only complete stable versions are admitted. Parsing every component avoids
/// accepting malformed suffixes or extra comparators as a valid floor.
fn stable_version(version: &str) -> Result<(u64, u64, u64)> {
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        bail!("`{version}` must be a complete stable major.minor.patch version");
    }
    let component = |part: &str| -> Result<u64> {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            bail!("`{version}` must contain canonical numeric version components");
        }
        let value: u64 =
            part.parse().with_context(|| format!("version component in `{version}` overflows"))?;
        // node-semver rejects components beyond Number.MAX_SAFE_INTEGER.
        if value > 9_007_199_254_740_991 {
            bail!("version component in `{version}` exceeds npm's safe integer limit");
        }
        Ok(value)
    };
    Ok((component(parts[0])?, component(parts[1])?, component(parts[2])?))
}

/// This deliberately admits the repository's stable exact/tilde types pins,
/// not a general npm range grammar. npm `~M.m.p` permits patches below the next
/// minor; `^M.m.p` (for M > 0) and `>=M.m.p` can select newer API lines even
/// when their lower floor matches. Unsupported forms fail closed.
fn validate_types_range(engines: &str, types: &str) -> Result<()> {
    let engines = engines.trim();
    let host = stable_version(
        engines
            .strip_prefix('^')
            .or_else(|| engines.strip_prefix('~'))
            .or_else(|| engines.strip_prefix(">="))
            .unwrap_or(engines),
    )?;
    let types = types.trim();
    let api = stable_version(types.strip_prefix('~').unwrap_or(types))?;
    if (api.0, api.1) != (host.0, host.1) {
        bail!("{TYPES_DEP} `{types}` must stay on engines.vscode `{engines}` API line");
    }
    Ok(())
}

#[test]
fn compatibility_range_controls() -> Result<()> {
    // Patch updates within the configured API line remain legitimate. A
    // coordinated floor change also works, without hard-coding 1.125.
    for (host, types) in [
        ("^1.125.0", "~1.125.0"),
        ("^1.125.0", "~1.125.7"),
        ("^1.125.0", "1.125.7"),
        ("^1.134.0", "~1.134.0"),
        ("^0.3.0", "~0.3.4"),
    ] {
        validate_types_range(host, types)?;
    }
    for types in [
        "~1.134.0",
        "~1.124.0",
        "~2.125.0", // wrong API lines
        ">=1.125.0",
        "^1.125.0", // matching floor, unsafe upper range
        ">=1.125.0 <2.0.0",
        "1.125.0 || 1.134.0", // wider compound ranges
        "~1.125",
        "1.125.0.1",
        "~1.125.garbage",
        "~1.125.0-beta.1",
        "~01.125.0",
        "~1.125.9007199254740992",
        "~1.125.0 <2.0.0",
        "*",
        "",
    ] {
        assert!(validate_types_range("^1.125.0", types).is_err(), "accepted `{types}`");
    }
    for host in ["^1.125", "^1.125.bad", "^1.125.0 || ^2.0.0"] {
        assert!(validate_types_range(host, "~1.125.0").is_err(), "accepted host `{host}`");
    }
    Ok(())
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
    validate_types_range(&engines, &types).with_context(|| {
        format!(
            "{TYPES_DEP} `{types}` must be an exact or tilde stable version on the \
             engines.vscode `{engines}` API line. Raise types only with the \
             minimum-host support contract (#13358)."
        )
    })?;
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
