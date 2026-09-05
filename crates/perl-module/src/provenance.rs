//! Optional, filesystem-only provenance signals for resolved Perl modules.
//!
//! This module deliberately does not verify signatures or infer trust from a
//! path. It reports distribution markers for consumers that explicitly opt
//! into the additional filesystem work.

use std::path::{Path, PathBuf};

/// Distribution markers found for a Perl module.
///
/// Marker presence is evidence about packaging, not proof of authenticity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleProvenance {
    /// A `META.json` or `META.yml` marker was found.
    pub has_meta: bool,
    /// A `SIGNATURE` marker was found. Its contents were not verified.
    pub has_signature: bool,
    /// A `CHECKSUMS` marker was found. Its contents were not verified.
    pub has_checksums: bool,
}

/// Informational classification derived from marker presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleProvenanceClass {
    /// A signature marker exists, but the signature is not verified.
    ClaimsSignature,
    /// Distribution metadata exists (`META.json`/`META.yml` or `CHECKSUMS`)
    /// without a signature marker.
    Packaged,
    /// No recognized distribution metadata was found within the searched
    /// authority boundary.
    Unknown,
}

impl ModuleProvenance {
    /// Classify marker presence without making a trust decision.
    ///
    /// Every recognized marker participates in the classification: a
    /// `CHECKSUMS`-only distribution is `Packaged`, exactly like a
    /// `META.json`-only one — recognized checksum metadata must never be
    /// reported as indistinguishable from absence.
    #[must_use]
    pub const fn class(self) -> ModuleProvenanceClass {
        if self.has_signature {
            ModuleProvenanceClass::ClaimsSignature
        } else if self.has_meta || self.has_checksums {
            ModuleProvenanceClass::Packaged
        } else {
            ModuleProvenanceClass::Unknown
        }
    }
}

/// Detect distribution markers for a module file within an authority root.
///
/// The search starts at the module's containing directory and walks upward,
/// stopping at the first directory containing a marker. The walk never
/// leaves `authority_root`: the authority root itself is searched
/// (inclusive), and the search stops there, so an unmarked module cannot
/// inherit markers from unrelated ancestor directories above the admitted
/// root (for example a workspace-level `META.json` above a vendored install
/// root, or a stray marker in a home or temporary directory). A module
/// outside `authority_root`, or a missing or non-file module path, returns
/// the same result as an unmarked module.
#[must_use]
pub fn detect_module_provenance(module_file: &Path, authority_root: &Path) -> ModuleProvenance {
    marker_walk(module_file, authority_root)
        .map_or_else(ModuleProvenance::default, |(_, markers)| markers)
}

/// Return the first directory at or above `module_file`, within
/// `authority_root`, containing a marker.
///
/// The walk is bounded by `authority_root` exactly like
/// [`detect_module_provenance`]; `None` means no marker exists inside the
/// admitted boundary (or the module path is missing, not a file, or outside
/// the boundary).
#[must_use]
pub fn module_provenance_root(module_file: &Path, authority_root: &Path) -> Option<PathBuf> {
    marker_walk(module_file, authority_root).map(|(directory, _)| directory)
}

/// Walk from the module's containing directory up to (and including) the
/// authority root, returning the first marker directory and its markers.
fn marker_walk(module_file: &Path, authority_root: &Path) -> Option<(PathBuf, ModuleProvenance)> {
    if !module_file.is_file() {
        return None;
    }
    let mut directory = module_file.parent()?.to_path_buf();
    if !directory.starts_with(authority_root) {
        return None;
    }
    loop {
        let provenance = markers_in(&directory);
        if provenance != ModuleProvenance::default() {
            return Some((directory, provenance));
        }
        if directory == authority_root || !directory.pop() {
            return None;
        }
    }
}

fn markers_in(directory: &Path) -> ModuleProvenance {
    ModuleProvenance {
        has_meta: directory.join("META.json").is_file() || directory.join("META.yml").is_file(),
        has_signature: directory.join("SIGNATURE").is_file(),
        has_checksums: directory.join("CHECKSUMS").is_file(),
    }
}
