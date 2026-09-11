//! Canonical semantic digest of the stable train manifest.
//!
//! This replicates the order-invariant canonicalization defined by T01
//! (`#11764` `checklist.md`): a pure function of parsed content, independent
//! of map/array input order. Object keys and array elements are sorted before
//! concatenation, so shuffling the document leaves the digest unchanged while
//! any semantic edit changes it.
//!
//! Fidelity notes (the reference implementation was PowerShell):
//! - booleans encode capitalized (`b:True;` / `b:False;`), matching PowerShell
//!   string interpolation;
//! - string escaping mirrors `-replace '\\','\\\\' -replace ';','\\;'` with
//!   literal replacement strings: one backslash becomes four, a semicolon
//!   becomes two backslashes followed by the semicolon;
//! - only integers appear in the contract; any other number is a schema
//!   defect and fails closed instead of being formatted culture-sensitively.

use color_eyre::eyre::{Result, bail};
use serde_json::Value;

/// Compute the canonical SHA-256 semantic digest (uppercase hex).
pub fn canonical_digest(value: &Value) -> Result<String> {
    let mut buffer = String::new();
    walk(value, &mut buffer)?;
    let digest = Sha256Digest::of(buffer.as_bytes());
    Ok(digest.hex_uppercase())
}

fn walk(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("n;"),
        Value::Bool(b) => {
            if *b {
                out.push_str("b:True;");
            } else {
                out.push_str("b:False;");
            }
        }
        Value::Number(n) => {
            let Some(integer) = n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)) else {
                bail!(
                    "non-integer JSON number is a schema defect and cannot be canonically \
                     digested: {n}"
                );
            };
            out.push_str("i:");
            out.push_str(&integer.to_string());
            out.push(';');
        }
        Value::String(s) => {
            out.push_str("s:");
            out.push_str(&escape(s));
            out.push(';');
        }
        Value::Array(items) => {
            let mut encoded: Vec<String> = Vec::with_capacity(items.len());
            for item in items {
                let mut inner = String::new();
                walk(item, &mut inner)?;
                encoded.push(inner);
            }
            encoded.sort_unstable();
            out.push('[');
            for part in encoded {
                out.push_str(&part);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for key in keys {
                out.push_str(key);
                out.push('=');
                walk(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn escape(value: &str) -> String {
    // Backslash first (one becomes four), then semicolon (becomes two
    // backslashes plus the semicolon), exactly like the chained PowerShell
    // replacements with literal replacement strings.
    let doubled = value.replace('\\', "\\\\\\\\");
    doubled.replace(';', "\\\\;")
}

/// Minimal internal SHA-256 used for title fingerprints and the digest.
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let finalized = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&finalized);
        Sha256Digest(out)
    }

    pub fn hex_uppercase(&self) -> String {
        self.0.iter().map(|b| format!("{b:02X}")).collect()
    }
}

/// Recompute a node title fingerprint: first 16 uppercase hex characters of
/// the SHA-256 of the exact title text.
pub fn title_fingerprint(title: &str) -> String {
    Sha256Digest::of(title.as_bytes()).hex_uppercase()[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;
    use serde_json::json;

    #[test]
    fn encodes_scalars_like_the_reference_canonicalization() -> Result<()> {
        let doc = json!({
            "b": [true, false],
            "n": null,
            "i": 11682,
            "s": "a\\b;c",
            "nested": {"z": 1, "a": 2}
        });
        // sorted object keys: b, i, n, nested, s (nested keys sorted a, z);
        // object entries concatenate directly with no separator
        let expected = concat!(
            "{b=[b:False;b:True;]i=i:11682;n=n;",
            "nested={a=i:2;z=i:1;}s=s:a\\\\\\\\b\\\\;c;}"
        );
        let mut out = String::new();
        walk(&doc, &mut out)?;
        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn digest_is_independent_of_array_and_key_order() -> Result<()> {
        let a = json!({"x": [3, 1, 2], "y": {"k": ["b", "a"]}});
        let b = json!({"y": {"k": ["a", "b"]}, "x": [1, 2, 3]});
        let da = canonical_digest(&a)?;
        let db = canonical_digest(&b)?;
        assert_eq!(da, db);
        Ok(())
    }

    #[test]
    fn rejects_floating_point_numbers() -> Result<()> {
        let doc = json!({"x": 1.5});
        let err = canonical_digest(&doc)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("float must fail closed"))?;
        assert!(err.to_string().contains("non-integer"));
        Ok(())
    }
}
