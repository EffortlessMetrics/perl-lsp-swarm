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
    /// Distribution metadata exists without a signature marker.
    Packaged,
    /// No recognized distribution metadata was found.
    Unknown,
}

impl ModuleProvenance {
    /// Classify marker presence without making a trust decision.
    #[must_use]
    pub const fn class(self) -> ModuleProvenanceClass {
        if self.has_signature {
            ModuleProvenanceClass::ClaimsSignature
        } else if self.has_meta {
            ModuleProvenanceClass::Packaged
        } else {
            ModuleProvenanceClass::Unknown
        }
    }
}

/// Detect distribution markers for a module file.
///
/// The search starts at the module's containing directory and walks upward,
/// stopping at the first directory containing a marker. This handles the
/// normal `lib/Foo/Bar.pm` distribution layout without scanning every parent
/// on every marker check. A missing or non-file module path returns the same
/// result as an unmarked module.
#[must_use]
pub fn detect_module_provenance(module_file: &Path) -> ModuleProvenance {
    if !module_file.is_file() {
        return ModuleProvenance::default();
    }
    let Some(start) = module_file.parent() else {
        return ModuleProvenance::default();
    };

    let mut directory = start.to_path_buf();
    loop {
        let provenance = markers_in(&directory);
        if provenance != ModuleProvenance::default() {
            return provenance;
        }
        if !directory.pop() {
            return ModuleProvenance::default();
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

/// Return the first directory at or above `module_file` containing a marker.
#[must_use]
pub fn module_provenance_root(module_file: &Path) -> Option<PathBuf> {
    if !module_file.is_file() {
        return None;
    }
    let mut directory = module_file.parent()?.to_path_buf();
    loop {
        if markers_in(&directory) != ModuleProvenance::default() {
            return Some(directory);
        }
        if !directory.pop() {
            return None;
        }
    }
}
