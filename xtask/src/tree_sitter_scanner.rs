//! Tree-sitter Perl external scanner authority and projections (#7261).
//!
//! `tree-sitter-perl/src/scanner.c` is the sole behavior-owning source. Every
//! active compiled copy is a deterministic LF-normalized byte projection checked
//! by `cargo xtask tree-sitter-scanner check`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hand-authored inventory and provenance ledger.
pub const LEDGER_REL: &str = "policy/tree-sitter-scanner-authority.toml";

const SCHEMA: &str = "tree_sitter_scanner_authority.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: String,
    controlling_issue: u64,
    sync_command: String,
    check_command: String,
    generator: String,
    authority: AuthorityRow,
    #[serde(default)]
    projection: Vec<ProjectionRow>,
    #[serde(default)]
    archive: Vec<ArchiveRow>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthorityRow {
    path: String,
    role: String,
    content_sha256: String,
    note: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectionRow {
    path: String,
    role: String,
    authority: String,
    consumer: String,
    note: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchiveRow {
    path: String,
    role: String,
    #[serde(default)]
    optional: bool,
    note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationCode {
    LedgerRead,
    LedgerSchema,
    MissingAuthority,
    MissingProjection,
    AuthorityDrift,
    ProjectionDrift,
    StaleProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: ViolationCode,
    pub path: String,
    pub detail: String,
}

impl Violation {
    pub fn display_line(&self) -> String {
        match self.code {
            ViolationCode::AuthorityDrift | ViolationCode::ProjectionDrift => format!(
                "SCANNER_DRIFT: {} — {}; run `{}`",
                self.path, self.detail, "cargo xtask tree-sitter-scanner sync"
            ),
            ViolationCode::StaleProvenance => format!(
                "SCANNER_STALE_PROVENANCE: {} — {}; run `{}` and commit the ledger",
                self.path, self.detail, "cargo xtask tree-sitter-scanner sync"
            ),
            ViolationCode::MissingAuthority => {
                format!("SCANNER_MISSING_AUTHORITY: {} — {}", self.path, self.detail)
            }
            ViolationCode::MissingProjection => {
                format!("SCANNER_MISSING_PROJECTION: {} — {}", self.path, self.detail)
            }
            ViolationCode::LedgerRead => {
                format!("SCANNER_LEDGER_READ: {} — {}", self.path, self.detail)
            }
            ViolationCode::LedgerSchema => {
                format!("SCANNER_LEDGER_SCHEMA: {} — {}", self.path, self.detail)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub authority_path: String,
    pub updated_projections: Vec<String>,
    pub authority_sha256: String,
}

/// Normalize scanner bytes for semantic equality: CRLF/CR → LF.
pub fn normalize_scanner_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' if bytes.get(idx + 1) == Some(&b'\n') => {
                out.push(b'\n');
                idx += 2;
            }
            b'\r' => {
                out.push(b'\n');
                idx += 1;
            }
            byte => {
                out.push(byte);
                idx += 1;
            }
        }
    }
    out
}

pub fn content_sha256(bytes: &[u8]) -> String {
    let normalized = normalize_scanner_bytes(bytes);
    let digest = Sha256::digest(normalized);
    hex_digest(&digest)
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn load_ledger(root: &Path) -> Result<Ledger, Violation> {
    let path = root.join(LEDGER_REL);
    let raw = fs::read_to_string(&path).map_err(|error| Violation {
        code: ViolationCode::LedgerRead,
        path: LEDGER_REL.to_string(),
        detail: error.to_string(),
    })?;
    let ledger: Ledger = toml::from_str(&raw).map_err(|error| Violation {
        code: ViolationCode::LedgerRead,
        path: LEDGER_REL.to_string(),
        detail: error.to_string(),
    })?;
    if ledger.schema_version != SCHEMA {
        return Err(Violation {
            code: ViolationCode::LedgerSchema,
            path: LEDGER_REL.to_string(),
            detail: format!("expected `{SCHEMA}`, got `{}`", ledger.schema_version),
        });
    }
    Ok(ledger)
}

fn read_normalized(root: &Path, relative: &str) -> Result<Vec<u8>, Violation> {
    let path = root.join(relative);
    let bytes = fs::read(&path).map_err(|error| Violation {
        code: if relative == "tree-sitter-perl/src/scanner.c" {
            ViolationCode::MissingAuthority
        } else {
            ViolationCode::MissingProjection
        },
        path: relative.to_string(),
        detail: error.to_string(),
    })?;
    Ok(normalize_scanner_bytes(&bytes))
}

/// Fail closed when authority, projections, or ledger fingerprints drift.
pub fn check(root: &Path) -> Vec<Violation> {
    let ledger = match load_ledger(root) {
        Ok(ledger) => ledger,
        Err(violation) => return vec![violation],
    };

    let authority_rel = ledger.authority.path.clone();
    let authority_bytes = match read_normalized(root, &authority_rel) {
        Ok(bytes) => bytes,
        Err(violation) => return vec![violation],
    };
    let authority_hash = content_sha256(&authority_bytes);
    if authority_hash != ledger.authority.content_sha256 {
        return vec![Violation {
            code: ViolationCode::StaleProvenance,
            path: LEDGER_REL.to_string(),
            detail: format!(
                "ledger records `{0}` but authority `{authority_rel}` is `{authority_hash}`",
                ledger.authority.content_sha256
            ),
        }];
    }

    if ledger.projection.is_empty() {
        return vec![Violation {
            code: ViolationCode::LedgerSchema,
            path: LEDGER_REL.to_string(),
            detail: "projection inventory must list every active compiled scanner copy (#7261)"
                .to_string(),
        }];
    }

    let mut violations = Vec::new();
    for projection in &ledger.projection {
        if projection.authority != authority_rel {
            violations.push(Violation {
                code: ViolationCode::LedgerSchema,
                path: LEDGER_REL.to_string(),
                detail: format!(
                    "projection `{}` names unexpected authority `{}`",
                    projection.path, projection.authority
                ),
            });
            continue;
        }
        let projection_path = projection.path.clone();
        match read_normalized(root, &projection_path) {
            Ok(projection_bytes) if projection_bytes == authority_bytes => {}
            Ok(_) => violations.push(Violation {
                code: ViolationCode::ProjectionDrift,
                path: projection_path,
                detail: "normalized bytes differ from authority".to_string(),
            }),
            Err(violation) => violations.push(violation),
        }
    }

    violations
}

/// Copy authority bytes to every active projection and refresh ledger fingerprints.
pub fn sync(root: &Path) -> Result<SyncOutcome, Violation> {
    let mut ledger = load_ledger(root)?;
    let authority_rel = ledger.authority.path.clone();
    let authority_disk = fs::read(root.join(&authority_rel)).map_err(|error| Violation {
        code: ViolationCode::MissingAuthority,
        path: authority_rel.clone(),
        detail: error.to_string(),
    })?;
    let normalized = normalize_scanner_bytes(&authority_disk);
    let authority_hash = content_sha256(&normalized);

    if ledger.projection.is_empty() {
        return Err(Violation {
            code: ViolationCode::LedgerSchema,
            path: LEDGER_REL.to_string(),
            detail: "projection inventory must list every active compiled scanner copy (#7261)"
                .to_string(),
        });
    }

    let mut updated = Vec::new();
    for projection in &ledger.projection {
        if projection.authority != authority_rel {
            return Err(Violation {
                code: ViolationCode::LedgerSchema,
                path: LEDGER_REL.to_string(),
                detail: format!(
                    "projection `{}` names unexpected authority `{}`",
                    projection.path, projection.authority
                ),
            });
        }
        let projection_path = root.join(&projection.path);
        let existing = fs::read(&projection_path).ok();
        let existing_normalized = existing.as_ref().map(|bytes| normalize_scanner_bytes(bytes));
        if existing_normalized.as_ref() != Some(&normalized) {
            write_normalized(&projection_path, &normalized).map_err(|error| Violation {
                code: ViolationCode::MissingProjection,
                path: projection.path.clone(),
                detail: error.to_string(),
            })?;
            updated.push(projection.path.clone());
        }
    }

    ledger.authority.content_sha256 = authority_hash.clone();
    write_ledger(root, &ledger).map_err(|error| Violation {
        code: ViolationCode::LedgerRead,
        path: LEDGER_REL.to_string(),
        detail: error.to_string(),
    })?;

    Ok(SyncOutcome {
        authority_path: authority_rel,
        updated_projections: updated,
        authority_sha256: authority_hash,
    })
}

fn write_normalized(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

fn write_ledger(root: &Path, ledger: &Ledger) -> io::Result<()> {
    let serialized = toml::to_string_pretty(ledger)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(root.join(LEDGER_REL), serialized)
}

/// True when every archived ledger path that exists differs from authority bytes.
pub fn archived_copies_do_not_satisfy_gate(root: &Path) -> Result<bool, Violation> {
    let ledger = load_ledger(root)?;
    let authority = read_normalized(root, &ledger.authority.path)?;
    for archive in ledger.archive {
        let path = root.join(&archive.path);
        if !path.is_file() {
            if archive.optional {
                continue;
            }
            continue;
        }
        let bytes = read_normalized(root, &archive.path)?;
        if bytes == authority {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn ledger_path(root: &Path) -> PathBuf {
    root.join(LEDGER_REL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_fixture(root: &Path, authority: &str, projection: Option<&str>) {
        fs::create_dir_all(root.join("grammar")).expect("grammar dir");
        fs::create_dir_all(root.join("crate")).expect("crate dir");
        fs::create_dir_all(root.join("policy")).expect("policy dir");
        fs::write(root.join("grammar/scanner.c"), authority).expect("authority");
        if let Some(body) = projection {
            fs::write(root.join("crate/scanner.c"), body).expect("projection");
        }
        let hash = content_sha256(authority.as_bytes());
        let ledger = format!(
            r#"
schema_version = "tree_sitter_scanner_authority.v1"
controlling_issue = 7261
sync_command = "cargo xtask tree-sitter-scanner sync"
check_command = "cargo xtask tree-sitter-scanner check"
generator = "test"

[authority]
path = "grammar/scanner.c"
role = "authoritative"
content_sha256 = "{hash}"
note = "fixture"

[[projection]]
path = "crate/scanner.c"
role = "active_projection"
authority = "grammar/scanner.c"
consumer = "fixture"
note = "fixture"
"#
        );
        fs::write(root.join(LEDGER_REL), ledger).expect("ledger");
    }

    #[test]
    fn normalize_scanner_bytes_unifies_crlf_and_lf() {
        assert_eq!(normalize_scanner_bytes(b"a\r\nb\rc"), normalize_scanner_bytes(b"a\nb\nc"));
    }

    #[test]
    fn check_fails_when_projection_inventory_is_empty() {
        let dir = TempDir::new().expect("tempdir");
        fs::create_dir_all(dir.path().join("grammar")).expect("grammar dir");
        fs::write(dir.path().join("grammar/scanner.c"), b"x").expect("authority");
        fs::create_dir_all(dir.path().join("policy")).expect("policy dir");
        let hash = content_sha256(b"x");
        let ledger = format!(
            r#"
schema_version = "tree_sitter_scanner_authority.v1"
controlling_issue = 7261
sync_command = "cargo xtask tree-sitter-scanner sync"
check_command = "cargo xtask tree-sitter-scanner check"
generator = "test"

[authority]
path = "grammar/scanner.c"
role = "authoritative"
content_sha256 = "{hash}"
note = "fixture"
"#
        );
        fs::write(dir.path().join(LEDGER_REL), ledger).expect("ledger");
        let violations = check(dir.path());
        assert!(violations.iter().any(|v| v.code == ViolationCode::LedgerSchema));
    }

    #[test]
    fn check_fails_when_projection_differs() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(dir.path(), "authority-bytes", Some("edited-projection"));
        let violations = check(dir.path());
        assert!(violations.iter().any(|v| v.code == ViolationCode::ProjectionDrift));
    }

    #[test]
    fn sync_is_byte_stable_on_second_run() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(dir.path(), "stable-bytes", Some("wrong"));
        let first = sync(dir.path()).expect("first sync");
        assert_eq!(first.updated_projections, vec!["crate/scanner.c".to_string()]);
        let second = sync(dir.path()).expect("second sync");
        assert!(second.updated_projections.is_empty());
        assert!(check(dir.path()).is_empty());
    }
}
