//! Refactoring and modernization helpers.
//!
//! The absorbed filesystem-mutating refactoring engine was retired in
//! #5231: live refactor operations are owned by their operation-specific
//! providers (rename, code actions, modernization, import optimization,
//! workspace refactor), and any future reusable pure planner is owned by
//! #8281. Do not reintroduce a shared engine here.

pub mod import_optimizer;
pub mod inline;
pub mod modernize;
pub mod modernize_refactored;
pub mod workspace_refactor;
pub mod workspace_rename;
