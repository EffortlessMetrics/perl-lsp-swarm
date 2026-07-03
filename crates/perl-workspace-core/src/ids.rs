//! Deterministic, stable identity for files and symbols.
//!
//! Project facts need identity that is reproducible across indexing runs and
//! machines. We derive IDs from stable inputs (repo-relative path; package +
//! name + kind + range) via the pinned [`SourceDigest`](crate::SourceDigest)
//! algorithm, so the same logical entity always gets the same ID without a
//! central counter or run-order dependence.
//!
//! [`FileId`] is re-exported from [`perl_semantic_facts`] so the whole fact
//! ecosystem shares one identity space; [`file_id_for`] is the canonical way to
//! mint one from a [`RepoRelativePath`].

use crate::digest::SourceDigest;
use crate::path::RepoRelativePath;
use serde::{Deserialize, Serialize};

pub use perl_semantic_facts::FileId;

/// Derive a stable [`FileId`] from a repo-relative path.
///
/// Identity is a function of the path alone — it is stable across content
/// edits (the same file keeps its ID as you type) and independent of discovery
/// order. Distinct paths get distinct IDs modulo the 64-bit digest space.
#[must_use]
pub fn file_id_for(path: &RepoRelativePath) -> FileId {
    FileId(SourceDigest::of_str(path.as_str()).value())
}

/// A deterministic, location-anchored symbol identity.
///
/// Unlike [`perl_semantic_facts::EntityId`] (an index-assigned handle within a
/// single analysis), a `SymbolId` is derived from the symbol's stable
/// coordinates — owning file, package context, name, kind tag, and byte range —
/// so it is reproducible across runs and comparable across snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SymbolId(pub u64);

impl SymbolId {
    /// Derive a stable symbol ID from its coordinates.
    ///
    /// `kind_tag` should be a short stable discriminator (e.g. the
    /// [`perl_semantic_facts::EntityKind`] name) so that two symbols sharing a
    /// name but differing in kind do not collide. The range disambiguates
    /// same-named symbols within one file.
    #[must_use]
    pub fn derive(
        file: FileId,
        package: Option<&str>,
        name: &str,
        kind_tag: &str,
        start_byte: u32,
        end_byte: u32,
    ) -> Self {
        // Canonical, unambiguous encoding: length-prefixed fields joined by a
        // byte that cannot appear in the length prefixes. Using explicit field
        // separators plus the file id and range makes accidental collisions
        // between e.g. ("a", "bc") and ("ab", "c") impossible.
        let mut buf = Vec::new();
        buf.extend_from_slice(&file.0.to_le_bytes());
        push_field(&mut buf, package.unwrap_or(""));
        push_field(&mut buf, name);
        push_field(&mut buf, kind_tag);
        buf.extend_from_slice(&start_byte.to_le_bytes());
        buf.extend_from_slice(&end_byte.to_le_bytes());
        SymbolId(SourceDigest::of_bytes(&buf).value())
    }
}

/// Append a length-prefixed field to `buf`.
fn push_field(buf: &mut Vec<u8>, field: &str) {
    let bytes = field.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn path(s: &str) -> RepoRelativePath {
        RepoRelativePath::new(s).expect("valid path")
    }

    #[test]
    fn file_id_is_stable_and_path_derived() {
        let a = file_id_for(&path("lib/Foo.pm"));
        let b = file_id_for(&path("lib/Foo.pm"));
        assert_eq!(a, b);
    }

    #[test]
    fn distinct_paths_distinct_file_ids() {
        assert_ne!(file_id_for(&path("lib/Foo.pm")), file_id_for(&path("lib/Bar.pm")));
    }

    #[test]
    fn symbol_id_is_deterministic() {
        let f = file_id_for(&path("lib/Foo.pm"));
        let a = SymbolId::derive(f, Some("Foo"), "bar", "Subroutine", 10, 40);
        let b = SymbolId::derive(f, Some("Foo"), "bar", "Subroutine", 10, 40);
        assert_eq!(a, b);
    }

    #[test]
    fn symbol_id_distinguishes_kind_package_range() {
        let f = file_id_for(&path("lib/Foo.pm"));
        let base = SymbolId::derive(f, Some("Foo"), "bar", "Subroutine", 10, 40);
        assert_ne!(base, SymbolId::derive(f, Some("Foo"), "bar", "Method", 10, 40));
        assert_ne!(base, SymbolId::derive(f, Some("Bar"), "bar", "Subroutine", 10, 40));
        assert_ne!(base, SymbolId::derive(f, Some("Foo"), "bar", "Subroutine", 11, 40));
        assert_ne!(base, SymbolId::derive(f, None, "bar", "Subroutine", 10, 40));
    }

    #[test]
    fn symbol_id_no_field_boundary_collision() {
        let f = file_id_for(&path("lib/Foo.pm"));
        // ("a","bc") vs ("ab","c") must not collide thanks to length prefixes.
        let x = SymbolId::derive(f, Some("a"), "bc", "K", 0, 1);
        let y = SymbolId::derive(f, Some("ab"), "c", "K", 0, 1);
        assert_ne!(x, y);
    }
}
