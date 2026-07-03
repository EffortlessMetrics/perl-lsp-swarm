//! Honest limits: dynamic boundaries and model limitations.
//!
//! Perl is aggressively dynamic. Rather than fabricate certainty where the
//! language's runtime behaviour is not statically knowable, the substrate emits
//! explicit [`DynamicBoundary`] markers (a specific place where analysis stops
//! being sound) and [`ModelLimitation`] notes (a whole fact class known to be
//! incomplete). Consumers use these to lower confidence or surface caveats
//! instead of trusting a partial static picture as complete.

use crate::range::SourceRange;
use perl_semantic_facts::FileId;
use serde::{Deserialize, Serialize};

/// The category of a dynamic construct that bounds static analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DynamicBoundaryKind {
    /// `require`/`use` of a name computed at runtime.
    RuntimeRequire,
    /// `eval "..."` of a string.
    StringEval,
    /// Symbolic references (`$$name`, `&{$name}`, `no strict 'refs'`).
    SymbolicRef,
    /// An import system we do not statically model.
    UnknownImportSystem,
    /// Methods installed at runtime (e.g. via `*{...} = sub {...}`).
    GeneratedMethod,
    /// `AUTOLOAD`-dispatched calls.
    Autoload,
    /// Direct typeglob mutation (`*foo = ...`).
    TypeglobMutation,
    /// XS / `Inline` / other native boundaries.
    XsOrInlineBoundary,
}

/// A specific source location where Perl semantics become dynamic and static
/// facts past this point are unsound.
///
/// Emitting a boundary is the substrate's way of saying "analysis is honest up
/// to here; beyond this the answer depends on runtime state." Producers pair a
/// boundary with lowered [`Confidence`](perl_semantic_facts::Confidence) or a
/// [`Provenance::DynamicBoundary`](perl_semantic_facts::Provenance) on the
/// affected facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicBoundary {
    /// What kind of dynamic construct this is.
    pub kind: DynamicBoundaryKind,
    /// The file the boundary occurs in, when attributable.
    pub file_id: Option<FileId>,
    /// The source range of the dynamic construct, when known.
    pub range: Option<SourceRange>,
    /// Human-readable note on what is unknown past this point.
    pub note: String,
}

impl DynamicBoundary {
    /// Construct a boundary anchored to a file and range.
    #[must_use]
    pub fn at(
        kind: DynamicBoundaryKind,
        file_id: FileId,
        range: SourceRange,
        note: impl Into<String>,
    ) -> Self {
        Self { kind, file_id: Some(file_id), range: Some(range), note: note.into() }
    }
}

/// A known, structural incompleteness in a fact class (as opposed to a single
/// located dynamic construct).
///
/// Example: "POD facts do not resolve `=for` region owners", or "dist deps read
/// only from `cpanfile`, not `Makefile.PL` `PREREQ_PM`". A limitation records
/// that the substrate is *not claiming* completeness for the affected class, so
/// downstream code treats missing facts as UNKNOWN rather than absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimitation {
    /// A short, stable machine tag for the limitation class
    /// (e.g. `"pod.malformed"`, `"dist.makefile_pl_prereqs"`).
    pub code: String,
    /// Human-readable explanation of what is not modelled.
    pub message: String,
    /// The file this limitation applies to, when file-scoped.
    pub file_id: Option<FileId>,
    /// The source range the limitation applies to, when localisable.
    pub range: Option<SourceRange>,
}

impl ModelLimitation {
    /// A workspace-wide (non-file-scoped) limitation.
    #[must_use]
    pub fn global(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), file_id: None, range: None }
    }

    /// A file-scoped limitation.
    #[must_use]
    pub fn in_file(code: impl Into<String>, message: impl Into<String>, file_id: FileId) -> Self {
        Self { code: code.into(), message: message.into(), file_id: Some(file_id), range: None }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::ids::file_id_for;
    use crate::path::RepoRelativePath;

    fn fid() -> FileId {
        file_id_for(&RepoRelativePath::new("lib/Foo.pm").expect("valid"))
    }

    #[test]
    fn dynamic_boundary_carries_location() {
        let b = DynamicBoundary::at(
            DynamicBoundaryKind::StringEval,
            fid(),
            SourceRange::new(10, 30),
            "eval of runtime-constructed string",
        );
        assert_eq!(b.kind, DynamicBoundaryKind::StringEval);
        assert!(b.range.is_some());
        assert!(b.file_id.is_some());
    }

    #[test]
    fn model_limitation_scopes() {
        let g = ModelLimitation::global("pod.malformed", "cannot associate =for owner");
        assert!(g.file_id.is_none());
        let f = ModelLimitation::in_file("dist.prereqs", "Makefile.PL prereqs unread", fid());
        assert!(f.file_id.is_some());
    }

    #[test]
    fn limitations_roundtrip_json() {
        let f = ModelLimitation::in_file("x.y", "z", fid());
        let json = serde_json::to_string(&f).expect("serialize");
        let back: ModelLimitation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(f, back);
    }
}
