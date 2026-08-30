//! Inventory every consumer, symbol, and claimed behavior owned by
//! `perl-tree-sitter-compat` (#8880, train #8877 row PR-09).
//!
//! This layer is the *removal denominator* for the duplicate Tree-sitter
//! facade. #8889 migrates legitimate consumers and #8890 deletes the crate;
//! neither can act without knowing what the crate actually owns and who
//! actually depends on it. This task answers that question mechanically and
//! fails closed rather than guessing.
//!
//! It changes no compatibility behavior and removes no source.
//!
//! # Why two evidence planes
//!
//! Text search cannot prove a symbol unused: a crate can declare a Cargo
//! dependency and never write a `use` line, and a dependency can be reached
//! through a re-export the grep never sees. So discovery reconciles two
//! independent populations:
//!
//! * **Cargo evidence** — every workspace manifest that declares the package
//!   as a normal, dev, or build dependency. This is authoritative for the
//!   question "can any crate link against this?".
//! * **Text evidence** — every tracked file that mentions the package or its
//!   Rust module path.
//!
//! A ledger row may only claim `unused` when the Cargo population is empty.
//! When a manifest declares the dependency, `unused` is refused no matter what
//! the text plane says.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Hand-authored disposition ledger. Humans own this file.
pub const LEDGER_PATH: &str = "policy/tree-sitter-compat-inventory.toml";
/// Generated human-readable projection of the reconciled inventory.
pub const PROJECTION_PATH: &str = "docs/architecture/tree-sitter-compat-inventory.md";
/// Generated machine-readable inventory consumed by #8889/#8890.
pub const JSON_PATH: &str = "target/policy/tree-sitter-compat-inventory.json";

const SCHEMA: &str = "tree_sitter_compat_inventory.v1";
const PACKAGE: &str = "perl-tree-sitter-compat";
const MODULE_PATH: &str = "perl_tree_sitter_compat";
const CRATE_DIR: &str = "crates/perl-tree-sitter-compat";

/// Files whose only reason to mention the package is that they *are* this
/// inventory. Counting them as consumers would make the ledger self-feeding.
const SELF_ARTIFACTS: &[&str] = &[LEDGER_PATH, PROJECTION_PATH];

// ---------------------------------------------------------------------------
// Ledger model
// ---------------------------------------------------------------------------

/// The authored side of the inventory: one disposition per symbol and per
/// reference, with the owner and removal condition that make it actionable.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema_version: String,
    pub package: String,
    /// Issue that owns this inventory.
    pub controlling_issue: String,
    /// Issue that migrates legitimate consumers.
    pub migration_owner: String,
    /// Issue that deletes the crate.
    pub removal_owner: String,
    /// Digest of the crate's semantic source at the time the dispositions were
    /// audited. A source change invalidates the audit.
    pub source_digest: String,
    pub symbols: Vec<SymbolRow>,
    pub consumers: Vec<ConsumerRow>,
}

/// One public symbol owned by the crate.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SymbolRow {
    pub name: String,
    pub module: String,
    pub kind: SymbolKind,
    /// Defaults to `public`, which is what every current row is. An internal row
    /// must say so, so the two populations cannot be confused for one another.
    #[serde(default)]
    pub visibility: Visibility,
    pub disposition: Disposition,
    #[serde(default)]
    pub canonical_owner: Option<String>,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub migration_dependency: Option<String>,
    #[serde(default)]
    pub removal_condition: Option<String>,
    pub note: String,
}

/// One tracked file that references the package.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConsumerRow {
    pub path: String,
    pub reference_kind: ReferenceKind,
    pub disposition: Disposition,
    #[serde(default)]
    pub canonical_owner: Option<String>,
    #[serde(default)]
    pub removal_condition: Option<String>,
    pub note: String,
}

/// The six dispositions #8880 admits. Every row is exactly one of them.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// A canonical owner already provides equivalent behavior.
    AlreadyEquivalent,
    /// Behavior exists only here and must survive migration.
    UniqueAndRequired,
    /// Behavior exists only here but is invalid, obsolete, or unsupportable.
    UniqueButInvalidOrObsolete,
    /// Exists only so a consumer can be migrated; carries no authority.
    MigrationOnlyEntryPoint,
    /// Nothing consumes it, proven through Cargo evidence rather than grep.
    Unused,
    /// Disposition cannot be established. Blocks the inventory.
    UnknownBlocking,
}

impl Disposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyEquivalent => "already_equivalent",
            Self::UniqueAndRequired => "unique_and_required",
            Self::UniqueButInvalidOrObsolete => "unique_but_invalid_or_obsolete",
            Self::MigrationOnlyEntryPoint => "migration_only_entry_point",
            Self::Unused => "unused",
            Self::UnknownBlocking => "unknown_blocking",
        }
    }

    /// Every disposition except `unused` asserts a relationship to some other
    /// authority, so it must name that authority and how the row retires.
    fn requires_owner_and_removal_condition(self) -> bool {
        !matches!(self, Self::Unused)
    }

    /// A row that must survive migration has to name the proof that keeps it
    /// honest and the issue that carries it across.
    fn requires_fixture_and_migration_dependency(self) -> bool {
        matches!(self, Self::UniqueAndRequired)
    }
}

/// How far a declared item can be consumed from.
///
/// #8880 asks for a disposition per *public or internal* symbol. Both are
/// discovered and both need a row: dropping restricted-visibility items on the
/// floor would let the inventory report a complete surface while its denominator
/// quietly shrank — the same failure mode as an unbalanced brace.
#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// `pub` — consumable from outside the crate.
    #[default]
    Public,
    /// `pub(crate)`, `pub(super)`, or `pub(in ...)` — reachable only in-crate, so
    /// it is deleted with the crate rather than migrated.
    Internal,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
        }
    }
}

/// Rust item kinds the crate's modules expose.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    /// An inherent `pub fn` on a type, recorded as `Type::method`.
    Method,
}

impl SymbolKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Method => "method",
        }
    }
}

/// How a tracked file reaches the package.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// A manifest declares the package as a dependency.
    CargoDependency,
    /// Workspace membership or dependency declaration in the root manifest.
    WorkspaceManifest,
    /// A lockfile entry.
    Lockfile,
    /// Source inside the crate itself.
    OwnCrate,
    /// Rust source elsewhere that names the module path.
    RustImport,
    /// Prose, ADR, or generated documentation.
    Documentation,
    /// Policy, topology, or CI configuration data.
    Policy,
    /// Repository automation that operates on the package by name.
    Tooling,
}

impl ReferenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::CargoDependency => "cargo_dependency",
            Self::WorkspaceManifest => "workspace_manifest",
            Self::Lockfile => "lockfile",
            Self::OwnCrate => "own_crate",
            Self::RustImport => "rust_import",
            Self::Documentation => "documentation",
            Self::Policy => "policy",
            Self::Tooling => "tooling",
        }
    }
}

// ---------------------------------------------------------------------------
// Discovered model
// ---------------------------------------------------------------------------

/// An out-of-line module declared in `lib.rs`.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Module {
    pub name: String,
    /// `pub mod` rather than `mod`. A `pub` item in a private module is not
    /// crate-public unless `lib.rs` re-exports it.
    pub public: bool,
}

/// One declared symbol owned by the crate.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Export {
    pub module: String,
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    /// Whether `lib.rs` re-exports it at the crate root. Always false for an
    /// internal item, which cannot be re-exported publicly.
    pub reexported_at_root: bool,
}

/// A workspace manifest that declares the package as a dependency.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoDependent {
    /// Manifest path, repository-relative.
    pub manifest: String,
    /// `normal`, `dev`, or `build`.
    pub kind: String,
}

/// The mechanical side of the inventory.
#[derive(Debug, Serialize, Clone, Default)]
pub struct Discovered {
    pub exports: Vec<Export>,
    pub cargo_dependents: Vec<CargoDependent>,
    /// Tracked files mentioning the package, excluding this inventory's own
    /// artifacts.
    pub references: Vec<String>,
    /// Crate source files the digest is computed over.
    pub source_files: Vec<String>,
    pub source_digest: String,
    /// `path::fn_name` for every function declared in the crate's own tracked
    /// Rust sources. A ledger `fixture` must name one of these.
    pub fixtures: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Reconcile the authored ledger against the crate's real surface, then either
/// write the projections or assert they are current.
pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let ledger = load(&root)?;
    let discovered = discover(&root)?;
    validate(&ledger, &discovered)?;

    let markdown = render_markdown(&ledger, &discovered);
    let json = render_json(&ledger, &discovered)?;

    let projection_path = root.join(PROJECTION_PATH);

    if check {
        let existing = fs::read_to_string(&projection_path)
            .wrap_err_with(|| format!("failed to read {PROJECTION_PATH}"))?;
        if normalize_newlines(&existing) != markdown {
            bail!("{PROJECTION_PATH} is stale; run `cargo xtask compat-inventory`");
        }
        println!(
            "{PACKAGE} inventory is valid and current: {} symbols, {} references, {} cargo dependents",
            ledger.symbols.len(),
            ledger.consumers.len(),
            discovered.cargo_dependents.len()
        );
        return Ok(());
    }

    write_file(&projection_path, &markdown, PROJECTION_PATH)?;
    write_file(&root.join(JSON_PATH), &json, JSON_PATH)?;
    println!(
        "wrote {PROJECTION_PATH} and {JSON_PATH} from {} symbols and {} references",
        ledger.symbols.len(),
        ledger.consumers.len()
    );
    Ok(())
}

fn write_file(path: &Path, contents: &str, label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, contents).wrap_err_with(|| format!("failed to write {label}"))
}

fn load(root: &Path) -> Result<Ledger> {
    let path = root.join(LEDGER_PATH);
    let text =
        fs::read_to_string(&path).wrap_err_with(|| format!("failed to read {LEDGER_PATH}"))?;
    parse(&text)
}

/// Parse the authored ledger. Unknown fields are rejected so a typo cannot
/// silently drop a disposition.
pub fn parse(text: &str) -> Result<Ledger> {
    toml::from_str(text).wrap_err_with(|| format!("failed to parse {LEDGER_PATH}"))
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Build the mechanical population from the working tree.
pub fn discover(root: &Path) -> Result<Discovered> {
    let tracked = tracked_files(root)?;

    let lib_rs = fs::read_to_string(root.join(CRATE_DIR).join("src/lib.rs"))
        .wrap_err_with(|| format!("failed to read {CRATE_DIR}/src/lib.rs"))?;
    let modules = parse_modules(&lib_rs);
    let root_reexports = parse_root_reexports(&lib_rs);

    let mut exports = Vec::new();
    for module in &modules {
        let module_name = &module.name;
        let module_path = root.join(CRATE_DIR).join(format!("src/{module_name}.rs"));
        let source = fs::read_to_string(&module_path)
            .wrap_err_with(|| format!("failed to read {}", module_path.display()))?;
        for (name, kind, declared) in parse_declared_items(&source) {
            let reexported = root_reexports.iter().any(|(m, n)| m == module_name && n == &name);
            // Rust reachability, not the declaration keyword: a `pub` item in a
            // private module leaves the crate only through a root `pub use`.
            // Anything else stays internal and retires with the crate.
            let visibility = if declared == Visibility::Public && (module.public || reexported) {
                Visibility::Public
            } else {
                Visibility::Internal
            };
            let reexported_at_root = visibility == Visibility::Public && reexported;
            exports.push(Export {
                module: module_name.clone(),
                name,
                kind,
                visibility,
                reexported_at_root,
            });
        }
    }

    // A root re-export that no module actually defines means `lib.rs` and the
    // module files disagree; the inventory must not paper over that.
    if let Some((module, name)) = unresolved_reexports(&exports, &root_reexports).first() {
        bail!(
            "{CRATE_DIR}/src/lib.rs re-exports `{module}::{name}` but no public item of that \
             name was found in {CRATE_DIR}/src/{module}.rs"
        );
    }
    exports.sort();

    let mut cargo_dependents = discover_cargo_dependents(root, &tracked)?;
    cargo_dependents.sort();

    let references =
        merge_reference_population(discover_references(root, &tracked)?, &cargo_dependents);

    let (source_files, source_digest) = digest_sources(root, &tracked)?;
    let fixtures = discover_fixtures(root, &tracked)?;

    Ok(Discovered { exports, cargo_dependents, references, source_files, source_digest, fixtures })
}

/// `path::fn_name` for every `#[test]` function in the crate's own tracked Rust
/// sources.
///
/// Requiring a ledger fixture to resolve here does three things: it catches a
/// fixture naming a test that no longer exists, it keeps proof inside the
/// digested source so a renamed test cannot quietly rot a
/// `unique_and_required` row without also invalidating the audit, and — by
/// admitting only `#[test]` functions — it stops a row citing a helper or an
/// ordinary function as though it were proof.
fn discover_fixtures(root: &Path, tracked: &[String]) -> Result<BTreeSet<String>> {
    let prefix = format!("{CRATE_DIR}/");
    let mut out = BTreeSet::new();
    for path in tracked.iter().filter(|p| p.starts_with(&prefix) && p.ends_with(".rs")) {
        let text = fs::read_to_string(root.join(path))
            .wrap_err_with(|| format!("failed to read {path}"))?;
        let mut is_test = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with("#[") {
                // `#[test]` and `#[tokio::test]` mark a test; `#[cfg(test)]`
                // marks a module and must not.
                is_test |= trimmed.contains("test]");
                continue;
            }
            let after_qualifiers =
                strip_fn_qualifiers(trimmed.strip_prefix("pub ").unwrap_or(trimmed));
            if let Some(rest) = after_qualifiers.strip_prefix("fn ") {
                let name: String =
                    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if is_test && !name.is_empty() {
                    out.insert(format!("{path}::{name}"));
                }
            }
            is_test = false;
        }
    }
    Ok(out)
}

/// The reference population: tracked files whose text names the package, plus
/// every manifest that declares a Cargo dependency on it.
///
/// The union is required, not cosmetic. A member inheriting a renamed workspace
/// dependency (`tsc = { workspace = true }`) is a Cargo dependent whose manifest
/// never spells the package name, so it would be absent from the text plane.
/// `validate_consumers` demands a `cargo_dependency` row for every dependent and
/// rejects any row whose path is not in this population — so without the union
/// such a manifest could neither be recorded nor omitted, and no ledger could
/// satisfy both rules.
fn merge_reference_population(
    text_hits: Vec<String>,
    dependents: &[CargoDependent],
) -> Vec<String> {
    let mut references: BTreeSet<String> = text_hits.into_iter().collect();
    references.extend(dependents.iter().map(|d| d.manifest.clone()));
    references.into_iter().collect()
}

/// Root re-exports that no module actually defines.
///
/// Non-empty means `lib.rs` and the module files disagree about what the crate
/// exposes, so neither can be trusted as the surface the ledger accounts for.
fn unresolved_reexports(
    exports: &[Export],
    reexports: &[(String, String)],
) -> Vec<(String, String)> {
    reexports
        .iter()
        .filter(|(module, name)| {
            // An internal item of the right name does not satisfy a public
            // `pub use`; treating it as a match would hide a real disagreement
            // between `lib.rs` and the module files.
            !exports.iter().any(|e| {
                &e.module == module && &e.name == name && e.visibility == Visibility::Public
            })
        })
        .cloned()
        .collect()
}

fn tracked_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("ls-files")
        .current_dir(root)
        .output()
        .wrap_err("failed to run `git ls-files`")?;
    if !output.status.success() {
        bail!("`git ls-files` failed with status {}", output.status);
    }
    let text = String::from_utf8(output.stdout).wrap_err("`git ls-files` emitted non-UTF-8")?;
    Ok(text.lines().map(str::to_string).collect())
}

/// Blank comments and literal contents, one output line per input line, so
/// that brace depth reflects real nesting and no item is read out of prose.
///
/// This is load-bearing rather than cosmetic. `convert.rs` contains
/// `"{".repeat(5000)` inside its test module; counting that brace would leave
/// the scanner permanently one level deep, and every item declared after it
/// would be silently dropped from discovery — a fail-open hole in a task whose
/// entire job is to fail closed.
fn code_lines(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut block_comment_depth: usize = 0;
    let mut in_string = false;
    let mut raw_hashes: Option<usize> = None;

    for line in source.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut sanitized = String::with_capacity(line.len());
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            if block_comment_depth > 0 {
                // Rust block comments nest, so only the terminator that returns
                // the depth to zero ends sanitization. Treating the inner `*/`
                // as the end would expose the rest of the outer comment as
                // code — braces and all.
                if c == '/' && chars.get(i + 1) == Some(&'*') {
                    block_comment_depth += 1;
                    i += 2;
                } else if c == '*' && chars.get(i + 1) == Some(&'/') {
                    block_comment_depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(hashes) = raw_hashes {
                let closes = c == '"'
                    && chars[i + 1..].iter().take(hashes).filter(|c| **c == '#').count() == hashes;
                if closes {
                    raw_hashes = None;
                    i += 1 + hashes;
                } else {
                    i += 1;
                }
                continue;
            }
            if in_string {
                if c == '\\' {
                    i += 2;
                } else {
                    if c == '"' {
                        in_string = false;
                    }
                    i += 1;
                }
                continue;
            }
            if c == '/' && chars.get(i + 1) == Some(&'/') {
                break;
            }
            if c == '/' && chars.get(i + 1) == Some(&'*') {
                block_comment_depth = 1;
                i += 2;
                continue;
            }
            if c == 'r' && matches!(chars.get(i + 1), Some('"') | Some('#')) {
                let mut j = i + 1;
                let mut hashes = 0;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'"') {
                    raw_hashes = Some(hashes);
                    i = j + 1;
                    continue;
                }
            }
            if c == '"' {
                in_string = true;
                i += 1;
                continue;
            }
            if c == '\'' {
                // A char literal, or a lifetime like `'tree` that must not be
                // mistaken for one.
                if chars.get(i + 1) == Some(&'\\') {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] != '\'' {
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
                if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                    continue;
                }
                i += 1;
                continue;
            }
            sanitized.push(c);
            i += 1;
        }
        out.push(sanitized);
    }
    out
}

/// `pub mod <name>;` declarations in `lib.rs`.
pub fn parse_modules(lib_rs: &str) -> Vec<Module> {
    let mut modules = Vec::new();
    for line in code_lines(lib_rs) {
        let line = line.trim();
        // A private `mod` is scanned too: it cannot expose public surface, but it
        // can hold `pub(crate)` items, and it can be the body of a `pub use`
        // facade. Skipping it would leave internal symbols uninventoried.
        let (rest, public) = match line.strip_prefix("pub mod ") {
            Some(rest) => (rest, true),
            None => match line.strip_prefix("mod ") {
                Some(rest) => (rest, false),
                None => continue,
            },
        };
        if let Some(name) = rest.strip_suffix(';') {
            let name = name.trim();
            if !name.is_empty() {
                modules.push(Module { name: name.to_string(), public });
            }
        }
    }
    modules.sort();
    modules.dedup();
    modules
}

/// `pub use <module>::{A, B};` and `pub use <module>::A;` in `lib.rs`.
pub fn parse_root_reexports(lib_rs: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in code_lines(lib_rs) {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub use ") else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(';') else {
            continue;
        };
        let Some((module, tail)) = rest.split_once("::") else {
            continue;
        };
        let module = module.trim().to_string();
        let tail = tail.trim();
        if let Some(inner) = tail.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
            for name in inner.split(',') {
                let name = name.trim();
                if !name.is_empty() {
                    out.push((module.clone(), name.to_string()));
                }
            }
        } else if !tail.is_empty() {
            out.push((module, tail.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every `pub` or restricted-visibility item declared in one module file.
///
/// Brace depth is tracked so an inherent `pub fn` inside `impl TsNode` is
/// recorded as the method `TsNode::child_count` rather than as a free function
/// the module does not have. Getting that wrong would send #8889 looking for a
/// module-level symbol that does not exist.
///
/// Internal items are returned alongside public ones rather than skipped, so the
/// ledger has to account for them too.
pub fn parse_declared_items(source: &str) -> Vec<(String, SymbolKind, Visibility)> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut impl_type: Option<String> = None;
    // Set from an `impl` header until its body actually opens. The brace can sit
    // on a later line — rustfmt does that for a long header, and a `where`
    // clause always does — and depth is still 0 in between, so without this the
    // type would be cleared before any method is read.
    let mut awaiting_impl_body = false;

    for line in code_lines(source) {
        let trimmed = line.trim();

        if depth == 0
            && !awaiting_impl_body
            && (trimmed.starts_with("impl ") || trimmed.starts_with("impl<"))
        {
            impl_type = inherent_impl_type(trimmed);
            awaiting_impl_body = true;
        } else if let Some((name, kind, visibility)) = parse_item(trimmed) {
            if depth == 0 {
                out.push((name, kind, visibility));
            } else if depth == 1
                && kind == SymbolKind::Function
                && let Some(ty) = &impl_type
            {
                out.push((format!("{ty}::{name}"), SymbolKind::Method, visibility));
            }
        }

        let delta = brace_delta(trimmed);
        if awaiting_impl_body && impl_body_opens(trimmed, delta) {
            awaiting_impl_body = false;
        }

        depth += delta;
        if depth <= 0 {
            depth = 0;
            // Only a closed impl clears the type; a header still waiting for its
            // brace keeps it.
            if !awaiting_impl_body {
                impl_type = None;
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Whether an `impl` body's opening brace is on this line.
///
/// A brace left open can only be the body. The hard case is a line whose braces
/// balance, which is either an empty body or a braced const-generic argument
/// inside the type. What separates them is not where the line sits but what the
/// final brace group *contains*: an empty body encloses nothing, a const
/// argument encloses an expression.
///
/// | line | final group | body? |
/// |---|---|---|
/// | `impl Foo {}` | empty | yes |
/// | `impl Foo<{ N + 1 }>` | does not end the line | no |
/// | `{ N + 1 }` | `N + 1` | no — a const argument spanning lines |
/// | `{}`, `{ }`, `where T: Copy {}` | empty | yes |
///
/// This deliberately reads brace structure rather than tracking angle-bracket
/// depth: `<` and `>` are ambiguous in Rust (comparison, shifts, nested
/// generics, and `->` in a bound), so counting them would add a larger failure
/// surface than the one it closes. Literals and comments are already removed by
/// `code_lines`, so `{ /* … */ }` reaches here as an empty group.
fn impl_body_opens(trimmed: &str, delta: i32) -> bool {
    delta > 0 || (delta == 0 && closes_an_empty_brace_group(trimmed))
}

/// Whether the line ends by closing a brace group that encloses nothing.
fn closes_an_empty_brace_group(trimmed: &str) -> bool {
    let Some(head) = trimmed.strip_suffix('}') else {
        return false;
    };
    match head.rfind('{') {
        Some(open) => head[open + 1..].trim().is_empty(),
        None => false,
    }
}

/// The type an inherent `impl` block belongs to, or `None` for a trait impl.
///
/// Trait-impl methods are not `pub`, so they never reach the caller anyway;
/// returning `None` keeps the intent explicit.
fn inherent_impl_type(line: &str) -> Option<String> {
    let rest = line.strip_prefix("impl")?;
    if rest.contains(" for ") {
        return None;
    }
    // Skip a generic parameter list: `impl<T> Wrapper<T> {`.
    let rest = match rest.strip_prefix('<') {
        Some(generics) => generics.split_once('>')?.1,
        None => rest,
    };
    let name: String =
        rest.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}

/// Strip the qualifiers Rust allows between `pub` and `fn`.
///
/// `pub async fn`, `pub unsafe fn`, `pub const fn`, and `pub extern "C" fn` are
/// all ordinary public functions; missing them would silently drop real public
/// surface from the inventory.
fn strip_fn_qualifiers(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start();
        let trimmed = if let Some(after) = rest.strip_prefix("extern ") {
            // `extern "C" fn` carries an ABI string, though `code_lines` has
            // usually already blanked it; `extern fn` carries none.
            match after.trim_start().strip_prefix('"').and_then(|abi| abi.split_once('"')) {
                Some((_, tail)) => tail,
                None => after,
            }
        } else if let Some(after) = rest
            .strip_prefix("async ")
            .or_else(|| rest.strip_prefix("unsafe "))
            .or_else(|| rest.strip_prefix("const "))
        {
            after
        } else {
            return rest;
        };
        rest = trimmed;
    }
}

fn parse_item(trimmed: &str) -> Option<(String, SymbolKind, Visibility)> {
    // `pub(crate) fn` has no space after `pub`, so the two spellings are split
    // before the shared item parsing.
    let (rest, visibility) = match trimmed.strip_prefix("pub(") {
        // `pub(crate)`, `pub(super)`, `pub(in path::to)` — restricted, but still
        // a declared symbol the ledger must account for.
        Some(after) => (after.split_once(')')?.1, Visibility::Internal),
        None => (trimmed.strip_prefix("pub ")?, Visibility::Public),
    };
    let rest = strip_fn_qualifiers(rest);
    let (kind, rest) = if let Some(r) = rest.strip_prefix("fn ") {
        (SymbolKind::Function, r)
    } else if let Some(r) = rest.strip_prefix("struct ") {
        (SymbolKind::Struct, r)
    } else if let Some(r) = rest.strip_prefix("enum ") {
        (SymbolKind::Enum, r)
    } else {
        return None;
    };
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some((name, kind, visibility)) }
}

fn brace_delta(line: &str) -> i32 {
    let opens = line.matches('{').count() as i32;
    let closes = line.matches('}').count() as i32;
    opens - closes
}

/// Manifests declaring the package as a dependency of any kind.
///
/// This is the plane a grep cannot replace: it reads dependency *tables*, so a
/// manifest that declares the package without any source ever importing it is
/// still found.
fn discover_cargo_dependents(root: &Path, tracked: &[String]) -> Result<Vec<CargoDependent>> {
    let aliases = workspace_dependency_aliases(root)?;
    let mut out = Vec::new();
    for path in tracked {
        if !path.ends_with("Cargo.toml") {
            continue;
        }
        // The package's own manifest is not a dependent of itself.
        if path == &format!("{CRATE_DIR}/Cargo.toml") {
            continue;
        }
        let text = fs::read_to_string(root.join(path))
            .wrap_err_with(|| format!("failed to read {path}"))?;
        // `toml::Value`'s `FromStr` parses a single value, not a document, so
        // a manifest must be read as a `Table` or every parse fails and the
        // dependent population is silently empty.
        let value = match toml::from_str::<toml::Table>(&text) {
            Ok(table) => toml::Value::Table(table),
            // A manifest that names the package but will not parse could be
            // declaring the dependency, and skipping it would let a false
            // `unused` through. One that never mentions the string cannot
            // declare it under any spelling: a renamed entry writes
            // `package = "perl-tree-sitter-compat"`, a workspace-inherited
            // alias is resolved from the root manifest, and a path entry must
            // end in the crate's directory name, which is the package name. So
            // an unrelated malformed fixture manifest is safe to pass over.
            Err(error) if text.contains(PACKAGE) => {
                bail!(
                    "failed to parse {path}, which names {PACKAGE}; the Cargo dependent \
                     population must be complete: {error}"
                );
            }
            Err(_) => continue,
        };

        collect_dependents(&value, path, None, &aliases, &mut out);
        // `[target.'cfg(...)'.dependencies]` links just as hard as a plain one.
        if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
            for (cfg, target_value) in targets {
                collect_dependents(target_value, path, Some(cfg), &aliases, &mut out);
            }
        }
    }
    Ok(out)
}

/// Aliases in the root `[workspace.dependencies]` table that resolve to the
/// package.
///
/// A member writing `tsc = { workspace = true }` names neither the package nor
/// a `package` key of its own; the rename lives in the root manifest. Without
/// this table such a member contributes no Cargo evidence at all.
fn workspace_dependency_aliases(root: &Path) -> Result<BTreeSet<String>> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path).wrap_err("failed to read the workspace Cargo.toml")?;
    let manifest: toml::Table =
        toml::from_str(&text).wrap_err("failed to parse the workspace Cargo.toml")?;
    let Some(entries) = manifest
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table)
    else {
        return Ok(BTreeSet::new());
    };
    Ok(entries
        .iter()
        .filter(|(key, entry)| workspace_alias_resolves(key, entry))
        .map(|(key, _)| key.clone())
        .collect())
}

/// Whether one `[workspace.dependencies]` entry identifies the package.
///
/// Three spellings, matching the three a member manifest can use, because a
/// member that inherits this alias names the package nowhere itself. The path
/// form names it nowhere *here* either — Cargo takes the identity from the
/// manifest the path points at — and missing it would drop every inheriting
/// member from the Cargo plane, which is the plane that gates `unused`.
/// Paths in the root manifest resolve from the workspace root.
fn workspace_alias_resolves(key: &str, entry: &toml::Value) -> bool {
    key == PACKAGE
        || entry.get("package").and_then(toml::Value::as_str) == Some(PACKAGE)
        || entry
            .get("path")
            .and_then(toml::Value::as_str)
            .is_some_and(|relative| path_targets_package("Cargo.toml", relative))
}

/// Record every dependency table in `value` that resolves to the package.
///
/// A dependency resolves by table key, through a renamed entry
/// (`tsc = { package = "perl-tree-sitter-compat" }`), or through a workspace
/// alias (`tsc = { workspace = true }` where the root renames `tsc`). Each form
/// links against the crate while spelling its name differently or not at all,
/// so matching keys alone would report zero dependents and wrongly permit
/// `unused`.
fn collect_dependents(
    value: &toml::Value,
    manifest: &str,
    cfg: Option<&str>,
    aliases: &BTreeSet<String>,
    out: &mut Vec<CargoDependent>,
) {
    for (table, kind) in
        [("dependencies", "normal"), ("dev-dependencies", "dev"), ("build-dependencies", "build")]
    {
        let Some(entries) = value.get(table).and_then(toml::Value::as_table) else {
            continue;
        };
        let declared = entries.iter().any(|(key, entry)| {
            key.as_str() == PACKAGE
                || entry.get("package").and_then(toml::Value::as_str) == Some(PACKAGE)
                || (entry.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                    && aliases.contains(key.as_str()))
                // `compat = { path = "../perl-tree-sitter-compat" }` names the
                // package nowhere: Cargo takes the identity from the manifest
                // the path points at.
                || entry
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|relative| path_targets_package(manifest, relative))
        });
        if declared {
            let kind = match cfg {
                Some(cfg) => format!("{kind} (target `{cfg}`)"),
                None => kind.to_string(),
            };
            out.push(CargoDependent { manifest: manifest.to_string(), kind });
        }
    }
}

/// Whether a `path = "…"` dependency in `manifest` points at the package.
///
/// Resolution is lexical on repository-relative paths, so it needs no
/// filesystem access and cannot fail on a manifest outside the workspace. A
/// path dependency that resolves to the crate's directory links the crate
/// whatever the entry is keyed as, because Cargo reads the identity out of the
/// manifest at that path.
fn path_targets_package(manifest: &str, relative: &str) -> bool {
    let Some(dir) = Path::new(manifest).parent() else {
        return false;
    };
    normalize_lexically(&dir.join(relative)) == Path::new(CRATE_DIR)
}

/// Resolve `.` and `..` without touching the filesystem.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Tracked files mentioning the package name or its Rust module path.
fn discover_references(root: &Path, tracked: &[String]) -> Result<Vec<String>> {
    let mut out = BTreeSet::new();
    for path in tracked {
        if SELF_ARTIFACTS.contains(&path.as_str()) {
            continue;
        }
        let full = root.join(path);
        // `git ls-files` reports submodule gitlinks, which are directories on
        // disk and carry no content of their own.
        if full.is_dir() {
            continue;
        }
        let text = match fs::read_to_string(&full) {
            Ok(text) => text,
            // A non-UTF-8 file cannot carry a Rust reference, so skipping it is
            // sound. Any other failure — a permission error, a broken symlink —
            // means this file's contents are unknown, and silently dropping it
            // would let a real reference escape the ledger.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => continue,
            Err(error) => {
                bail!(
                    "failed to read tracked file {path}; the reference population must be \
                     complete: {error}"
                );
            }
        };
        if text.contains(PACKAGE) || text.contains(MODULE_PATH) {
            out.insert(path.clone());
        }
    }
    Ok(out.into_iter().collect())
}

/// Digest the crate's semantic source so a behavior change invalidates the
/// audited dispositions. Prose files are deliberately excluded: a README typo
/// must not force a re-audit, but a source edit must.
fn digest_sources(root: &Path, tracked: &[String]) -> Result<(Vec<String>, String)> {
    let prefix = format!("{CRATE_DIR}/");
    let mut files: Vec<String> = tracked
        .iter()
        .filter(|p| p.starts_with(&prefix))
        .filter(|p| p.ends_with(".rs") || p.ends_with("Cargo.toml"))
        .cloned()
        .collect();
    files.sort();

    let mut hasher = Sha256::new();
    for path in &files {
        let bytes = fs::read(root.join(path)).wrap_err_with(|| format!("failed to read {path}"))?;
        // Length-prefix each field so a rename cannot collide with a content
        // change that happens to shift bytes across the boundary.
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let digest = format!("sha256:{hex}");
    Ok((files, digest))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Reconcile authored dispositions against discovered reality. Every failure
/// mode here is a way the inventory could otherwise lie to #8889/#8890.
pub fn validate(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    if ledger.schema_version != SCHEMA {
        bail!(
            "{LEDGER_PATH} declares schema_version `{}`; expected `{SCHEMA}`",
            ledger.schema_version
        );
    }
    if ledger.package != PACKAGE {
        bail!("{LEDGER_PATH} declares package `{}`; expected `{PACKAGE}`", ledger.package);
    }
    for (field, value) in [
        ("controlling_issue", &ledger.controlling_issue),
        ("migration_owner", &ledger.migration_owner),
        ("removal_owner", &ledger.removal_owner),
    ] {
        if !is_issue_ref(value) {
            bail!(
                "{LEDGER_PATH} field `{field}` must be an issue reference like `#8880`, got `{value}`"
            );
        }
    }

    if ledger.source_digest != discovered.source_digest {
        bail!(
            "{PACKAGE} source changed since the inventory was audited.\n  \
             ledger source_digest:    {}\n  \
             current source_digest:   {}\n\
             Re-audit the symbol dispositions against the new source, then update \
             `source_digest` in {LEDGER_PATH}.",
            ledger.source_digest,
            discovered.source_digest
        );
    }

    validate_symbols(ledger, discovered)?;
    validate_consumers(ledger, discovered)?;
    validate_row_requirements(ledger)?;
    validate_fixtures_resolve(ledger, discovered)?;
    validate_unused_needs_cargo_evidence(ledger, discovered)?;
    validate_no_unknown_rows(ledger)?;
    Ok(())
}

fn validate_symbols(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for row in &ledger.symbols {
        let key = (row.module.clone(), row.name.clone());
        if !seen.insert(key) {
            bail!("{LEDGER_PATH} declares duplicate symbol row `{}::{}`", row.module, row.name);
        }
    }

    let discovered_kinds: BTreeMap<(String, String), (SymbolKind, Visibility)> = discovered
        .exports
        .iter()
        .map(|e| ((e.module.clone(), e.name.clone()), (e.kind, e.visibility)))
        .collect();

    for export in &discovered.exports {
        let key = (export.module.clone(), export.name.clone());
        if !seen.contains(&key) {
            bail!(
                "{PACKAGE} declares {} symbol `{}::{}` but {LEDGER_PATH} has no row for it; every \
                 public and internal symbol needs exactly one disposition",
                export.visibility.as_str(),
                export.module,
                export.name
            );
        }
    }

    for row in &ledger.symbols {
        let key = (row.module.clone(), row.name.clone());
        match discovered_kinds.get(&key) {
            None => bail!(
                "{LEDGER_PATH} has a row for `{}::{}` but {PACKAGE} no longer exposes it; remove \
                 the stale row",
                row.module,
                row.name
            ),
            Some((kind, _)) if *kind != row.kind => bail!(
                "{LEDGER_PATH} records `{}::{}` as `{}` but the source declares `{}`",
                row.module,
                row.name,
                row.kind.as_str(),
                kind.as_str()
            ),
            Some((_, visibility)) if *visibility != row.visibility => bail!(
                "{LEDGER_PATH} records `{}::{}` as `{}` but the source declares it `{}`; a symbol \
                 that changed visibility needs its disposition re-audited",
                row.module,
                row.name,
                row.visibility.as_str(),
                visibility.as_str()
            ),
            Some(_) => {}
        }
    }
    Ok(())
}

fn validate_consumers(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in &ledger.consumers {
        if !seen.insert(row.path.as_str()) {
            bail!("{LEDGER_PATH} declares duplicate consumer row `{}`", row.path);
        }
    }

    for reference in &discovered.references {
        if !seen.contains(reference.as_str()) {
            bail!(
                "`{reference}` references {PACKAGE} but {LEDGER_PATH} does not explain it; an \
                 unexplained reference blocks the inventory"
            );
        }
    }

    let discovered_set: BTreeSet<&str> = discovered.references.iter().map(String::as_str).collect();
    for row in &ledger.consumers {
        if !discovered_set.contains(row.path.as_str()) {
            bail!(
                "{LEDGER_PATH} explains `{}` but no tracked file at that path references \
                 {PACKAGE}; remove the stale row",
                row.path
            );
        }
    }

    // Every Cargo dependent must be explained *as* a Cargo dependency, so a
    // real link-time consumer cannot be filed away as mere prose.
    for dependent in &discovered.cargo_dependents {
        let row = ledger.consumers.iter().find(|r| r.path == dependent.manifest);
        match row {
            None => bail!(
                "`{}` declares a Cargo dependency on {PACKAGE} but {LEDGER_PATH} has no row for it",
                dependent.manifest
            ),
            Some(row) if row.reference_kind != ReferenceKind::CargoDependency => bail!(
                "`{}` declares a {} Cargo dependency on {PACKAGE} but {LEDGER_PATH} records it as \
                 `{}`; a link-time consumer must be recorded as `cargo_dependency`",
                dependent.manifest,
                dependent.kind,
                row.reference_kind.as_str()
            ),
            Some(_) => {}
        }
    }
    Ok(())
}

fn validate_row_requirements(ledger: &Ledger) -> Result<()> {
    for row in &ledger.symbols {
        let label = format!("symbol `{}::{}`", row.module, row.name);
        if row.disposition.requires_owner_and_removal_condition() {
            require(&row.canonical_owner, "canonical_owner", &label, row.disposition)?;
            require(&row.removal_condition, "removal_condition", &label, row.disposition)?;
        }
        if row.disposition.requires_fixture_and_migration_dependency() {
            require(&row.fixture, "fixture", &label, row.disposition)?;
            require(&row.migration_dependency, "migration_dependency", &label, row.disposition)?;
        }
        if let Some(owner) = &row.canonical_owner
            && !is_issue_ref(owner)
        {
            bail!("{label} canonical_owner must be an issue reference like `#8803`, got `{owner}`");
        }
        if let Some(dep) = &row.migration_dependency
            && !is_issue_ref(dep)
        {
            bail!("{label} migration_dependency must be an issue reference, got `{dep}`");
        }
        if row.note.trim().is_empty() {
            bail!("{label} must carry a non-empty note");
        }
    }

    for row in &ledger.consumers {
        let label = format!("consumer `{}`", row.path);
        if row.disposition.requires_owner_and_removal_condition() {
            require(&row.canonical_owner, "canonical_owner", &label, row.disposition)?;
            require(&row.removal_condition, "removal_condition", &label, row.disposition)?;
        }
        if let Some(owner) = &row.canonical_owner
            && !is_issue_ref(owner)
        {
            bail!("{label} canonical_owner must be an issue reference, got `{owner}`");
        }
        if row.note.trim().is_empty() {
            bail!("{label} must carry a non-empty note");
        }
    }
    Ok(())
}

fn require(
    value: &Option<String>,
    field: &str,
    label: &str,
    disposition: Disposition,
) -> Result<()> {
    let missing = match value {
        None => true,
        Some(v) => v.trim().is_empty(),
    };
    if missing {
        bail!("{label} is `{}` and must record `{field}`", disposition.as_str());
    }
    Ok(())
}

/// A named fixture must resolve to a real function in the crate's digested
/// source. A non-empty string is not proof that the proof exists.
fn validate_fixtures_resolve(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    for row in &ledger.symbols {
        let Some(fixture) = &row.fixture else {
            continue;
        };
        if !discovered.fixtures.contains(fixture) {
            bail!(
                "symbol `{}::{}` names fixture `{fixture}`, which does not resolve to a function \
                 in {PACKAGE}'s tracked sources. A fixture must be `<tracked path>::<fn name>` \
                 inside {CRATE_DIR}, so that renaming the proof also invalidates this audit.",
                row.module,
                row.name
            );
        }
    }
    Ok(())
}

/// The rule #8880 states outright: text search alone cannot declare a row
/// unused. When any manifest declares the dependency, `unused` is refused.
fn validate_unused_needs_cargo_evidence(ledger: &Ledger, discovered: &Discovered) -> Result<()> {
    if discovered.cargo_dependents.is_empty() {
        return Ok(());
    }
    let dependents = discovered
        .cargo_dependents
        .iter()
        .map(|d| format!("{} ({})", d.manifest, d.kind))
        .collect::<Vec<_>>()
        .join(", ");

    for row in &ledger.symbols {
        if row.disposition == Disposition::Unused {
            bail!(
                "symbol `{}::{}` is recorded `unused`, but {PACKAGE} has Cargo dependents: {}. A \
                 declared dependency can consume a symbol without any text reference, so `unused` \
                 needs an empty Cargo population.",
                row.module,
                row.name,
                dependents
            );
        }
    }
    for row in &ledger.consumers {
        if row.disposition == Disposition::Unused
            && row.reference_kind == ReferenceKind::CargoDependency
        {
            bail!("consumer `{}` is a Cargo dependency and cannot be recorded `unused`", row.path);
        }
    }
    Ok(())
}

fn validate_no_unknown_rows(ledger: &Ledger) -> Result<()> {
    let mut unknown = Vec::new();
    for row in &ledger.symbols {
        if row.disposition == Disposition::UnknownBlocking {
            unknown.push(format!("symbol `{}::{}`", row.module, row.name));
        }
    }
    for row in &ledger.consumers {
        if row.disposition == Disposition::UnknownBlocking {
            unknown.push(format!("consumer `{}`", row.path));
        }
    }
    if !unknown.is_empty() {
        bail!(
            "{} row(s) remain `unknown_blocking`; the inventory is not complete:\n  {}",
            unknown.len(),
            unknown.join("\n  ")
        );
    }
    Ok(())
}

fn is_issue_ref(value: &str) -> bool {
    match value.strip_prefix('#') {
        Some(digits) => !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Canonical row order, so the projection cannot depend on ledger insertion
/// order.
fn sorted_symbols(ledger: &Ledger) -> Vec<SymbolRow> {
    let mut rows = ledger.symbols.clone();
    rows.sort_by(|a, b| (&a.module, &a.name).cmp(&(&b.module, &b.name)));
    rows
}

fn sorted_consumers(ledger: &Ledger) -> Vec<ConsumerRow> {
    let mut rows = ledger.consumers.clone();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

fn counts(rows: impl Iterator<Item = Disposition>) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for disposition in rows {
        *out.entry(disposition.as_str()).or_insert(0) += 1;
    }
    out
}

/// Escape a note for a Markdown table cell.
fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn opt(value: &Option<String>) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => v.clone(),
        _ => "—".to_string(),
    }
}

/// Render the human-readable projection.
pub fn render_markdown(ledger: &Ledger, discovered: &Discovered) -> String {
    let mut out = String::new();
    out.push_str("<!-- auto-generated by `cargo xtask compat-inventory`; do not edit -->\n\n");
    out.push_str("# `perl-tree-sitter-compat` consumer and behavior inventory\n\n");
    let _ = writeln!(
        out,
        "Generated from `{LEDGER_PATH}` reconciled against the crate's real surface. That file is\n\
         the authority; this page is a projection of it. Edit the ledger and run\n\
         `cargo xtask compat-inventory`.\n"
    );
    let _ = writeln!(
        out,
        "Controlling issue {}, train #8877 row PR-09. Migration is {}; crate removal is {}.\n\
         This inventory changes no compatibility behavior and removes no source.\n",
        ledger.controlling_issue, ledger.migration_owner, ledger.removal_owner
    );

    out.push_str("## Source binding\n\n");
    let _ = writeln!(
        out,
        "The dispositions below were audited against this exact source. A change to any file\n\
         listed here invalidates the audit and the task fails until the rows are re-checked.\n"
    );
    let _ = writeln!(out, "- Digest: `{}`", discovered.source_digest);
    let _ = writeln!(out, "- Files ({}):", discovered.source_files.len());
    for file in &discovered.source_files {
        let _ = writeln!(out, "  - `{file}`");
    }
    out.push('\n');

    out.push_str("## Cargo dependent evidence\n\n");
    if discovered.cargo_dependents.is_empty() {
        let _ = writeln!(
            out,
            "No workspace manifest declares a normal, dev, or build dependency on\n\
             `{PACKAGE}`. This is the evidence that permits an `unused` disposition; a text\n\
             search alone could not establish it.\n"
        );
    } else {
        out.push_str("| Manifest | Dependency kind |\n| --- | --- |\n");
        for dependent in &discovered.cargo_dependents {
            let _ = writeln!(out, "| `{}` | `{}` |", dependent.manifest, dependent.kind);
        }
        out.push('\n');
    }

    let symbols = sorted_symbols(ledger);
    let consumers = sorted_consumers(ledger);

    out.push_str("## Disposition summary\n\n");
    out.push_str("| Disposition | Symbols | References |\n| --- | ---: | ---: |\n");
    let symbol_counts = counts(symbols.iter().map(|r| r.disposition));
    let consumer_counts = counts(consumers.iter().map(|r| r.disposition));
    let mut dispositions: BTreeSet<&'static str> = BTreeSet::new();
    dispositions.extend(symbol_counts.keys().copied());
    dispositions.extend(consumer_counts.keys().copied());
    for disposition in &dispositions {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} |",
            disposition,
            symbol_counts.get(disposition).copied().unwrap_or(0),
            consumer_counts.get(disposition).copied().unwrap_or(0)
        );
    }
    out.push('\n');

    out.push_str("## Symbols\n\n");
    let internal = symbols.iter().filter(|r| r.visibility == Visibility::Internal).count();
    let _ = writeln!(
        out,
        "Every `pub` and restricted-visibility item the crate's modules declare. Internal symbols\n\
         ({internal} here) are reachable only in-crate, so they retire with the crate rather than\n\
         migrating; they are recorded rather than skipped so the surface has no unexamined\n\
         remainder.\n"
    );
    out.push_str(
        "| Symbol | Kind | Visibility | Root re-export | Disposition | Canonical owner | Fixture | Migration | Removal condition | Note |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &symbols {
        let reexported = discovered
            .exports
            .iter()
            .find(|e| e.module == row.module && e.name == row.name)
            .is_some_and(|e| e.reexported_at_root);
        let _ = writeln!(
            out,
            "| `{}::{}` | `{}` | `{}` | {} | `{}` | {} | {} | {} | {} | {} |",
            row.module,
            row.name,
            row.kind.as_str(),
            row.visibility.as_str(),
            if reexported { "yes" } else { "no" },
            row.disposition.as_str(),
            opt(&row.canonical_owner),
            cell(&opt(&row.fixture)),
            opt(&row.migration_dependency),
            cell(&opt(&row.removal_condition)),
            cell(&row.note)
        );
    }
    out.push('\n');

    out.push_str("## References\n\n");
    let _ = writeln!(
        out,
        "Every tracked file that mentions `{PACKAGE}` or `{MODULE_PATH}`, excluding this\n\
         inventory's own artifacts. An unexplained reference fails the task.\n"
    );
    out.push_str(
        "| Path | Reference kind | Disposition | Canonical owner | Removal condition | Note |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in &consumers {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | `{}` | {} | {} | {} |",
            row.path,
            row.reference_kind.as_str(),
            row.disposition.as_str(),
            opt(&row.canonical_owner),
            cell(&opt(&row.removal_condition)),
            cell(&row.note)
        );
    }
    out
}

/// The machine-readable inventory #8889/#8890 consume.
pub fn render_json(ledger: &Ledger, discovered: &Discovered) -> Result<String> {
    let document = serde_json::json!({
        "schema_version": SCHEMA,
        "package": PACKAGE,
        "controlling_issue": ledger.controlling_issue,
        "migration_owner": ledger.migration_owner,
        "removal_owner": ledger.removal_owner,
        "source": {
            "digest": discovered.source_digest,
            "files": discovered.source_files,
        },
        "cargo_dependents": discovered.cargo_dependents,
        "exports": discovered.exports,
        "symbols": sorted_symbols(ledger),
        "consumers": sorted_consumers(ledger),
    });
    let mut text = serde_json::to_string_pretty(&document)
        .wrap_err("failed to serialize the compat inventory")?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    /// Public declarations only, as `(name, kind)`. Most parser tests are about
    /// item shape rather than visibility, so they assert against this narrower
    /// projection of `parse_declared_items`.
    fn parse_public_items(source: &str) -> Vec<(String, SymbolKind)> {
        parse_declared_items(source)
            .into_iter()
            .filter(|(_, _, visibility)| *visibility == Visibility::Public)
            .map(|(name, kind, _)| (name, kind))
            .collect()
    }

    fn symbol(name: &str, disposition: Disposition) -> SymbolRow {
        SymbolRow {
            name: name.to_string(),
            module: "convert".to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            disposition,
            canonical_owner: Some("#8803".to_string()),
            fixture: Some("crates/perl-tree-sitter-compat/tests/adapter.rs::t".to_string()),
            migration_dependency: Some("#8889".to_string()),
            removal_condition: Some("retires when #8803 lands".to_string()),
            note: "note".to_string(),
        }
    }

    fn consumer(path: &str, kind: ReferenceKind) -> ConsumerRow {
        ConsumerRow {
            path: path.to_string(),
            reference_kind: kind,
            disposition: Disposition::MigrationOnlyEntryPoint,
            canonical_owner: Some("#8889".to_string()),
            removal_condition: Some("retires with the crate".to_string()),
            note: "note".to_string(),
        }
    }

    fn ledger(symbols: Vec<SymbolRow>, consumers: Vec<ConsumerRow>) -> Ledger {
        Ledger {
            schema_version: SCHEMA.to_string(),
            package: PACKAGE.to_string(),
            controlling_issue: "#8880".to_string(),
            migration_owner: "#8889".to_string(),
            removal_owner: "#8890".to_string(),
            source_digest: "sha256:abc".to_string(),
            symbols,
            consumers,
        }
    }

    fn discovered(exports: Vec<Export>, references: Vec<&str>) -> Discovered {
        Discovered {
            exports,
            cargo_dependents: Vec::new(),
            references: references.into_iter().map(str::to_string).collect(),
            source_files: vec![format!("{CRATE_DIR}/src/lib.rs")],
            source_digest: "sha256:abc".to_string(),
            fixtures: [format!("{CRATE_DIR}/tests/adapter.rs::t")].into_iter().collect(),
        }
    }

    fn dependents_of(text: &str) -> Vec<CargoDependent> {
        dependents_with_aliases(text, &BTreeSet::new())
    }

    fn dependents_with_aliases(text: &str, aliases: &BTreeSet<String>) -> Vec<CargoDependent> {
        let Ok(table) = toml::from_str::<toml::Table>(text) else {
            return vec![CargoDependent {
                manifest: "UNPARSEABLE TEST MANIFEST".to_string(),
                kind: "invalid".to_string(),
            }];
        };
        let value = toml::Value::Table(table);
        let mut out = Vec::new();
        collect_dependents(&value, "crates/other/Cargo.toml", None, aliases, &mut out);
        if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
            for (cfg, target_value) in targets {
                collect_dependents(
                    target_value,
                    "crates/other/Cargo.toml",
                    Some(cfg),
                    aliases,
                    &mut out,
                );
            }
        }
        out
    }

    fn export(name: &str) -> Export {
        Export {
            module: "convert".to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            reexported_at_root: true,
        }
    }

    /// The healthy case: authored rows match discovered reality exactly.
    #[test]
    fn reconciled_ledger_validates() -> TestResult {
        let l = ledger(
            vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)],
            vec![consumer("docs/x.md", ReferenceKind::Documentation)],
        );
        let d = discovered(vec![export("parse_to_tree")], vec!["docs/x.md"]);
        validate(&l, &d)
    }

    /// A Cargo dependency with no text reference anywhere is exactly the case
    /// a grep-only inventory gets wrong.
    #[test]
    fn cargo_dependency_forbids_an_unused_symbol() -> TestResult {
        let l = ledger(
            vec![symbol("parse_to_tree", Disposition::Unused)],
            vec![consumer("crates/other/Cargo.toml", ReferenceKind::CargoDependency)],
        );
        let mut d = discovered(vec![export("parse_to_tree")], vec!["crates/other/Cargo.toml"]);
        d.cargo_dependents = vec![CargoDependent {
            manifest: "crates/other/Cargo.toml".to_string(),
            kind: "normal".to_string(),
        }];

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("needs an empty Cargo population"), "unexpected error: {err}");
        Ok(())
    }

    /// The same ledger is valid once the Cargo population really is empty, so
    /// the rule above discriminates rather than banning `unused` outright.
    #[test]
    fn unused_is_allowed_when_no_manifest_declares_the_dependency() -> TestResult {
        let mut row = symbol("parse_to_tree", Disposition::Unused);
        row.canonical_owner = None;
        row.fixture = None;
        row.migration_dependency = None;
        row.removal_condition = None;
        let l = ledger(vec![row], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);
        validate(&l, &d)
    }

    /// A Cargo dependent filed as prose would hide a real link-time consumer.
    #[test]
    fn cargo_dependent_must_be_recorded_as_a_cargo_dependency() -> TestResult {
        let l = ledger(
            vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)],
            vec![consumer("crates/other/Cargo.toml", ReferenceKind::Documentation)],
        );
        let mut d = discovered(vec![export("parse_to_tree")], vec!["crates/other/Cargo.toml"]);
        d.cargo_dependents = vec![CargoDependent {
            manifest: "crates/other/Cargo.toml".to_string(),
            kind: "normal".to_string(),
        }];

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("must be recorded as `cargo_dependency`"), "unexpected error: {err}");
        Ok(())
    }

    /// A renamed dependency links against the crate without ever spelling its
    /// name as a table key. Matching keys alone would report zero dependents
    /// and wrongly permit `unused`.
    #[test]
    fn a_renamed_dependency_is_discovered() -> TestResult {
        let found = dependents_of(
            "[dependencies]\ntsc = { package = \"perl-tree-sitter-compat\", version = \"0.17.0\" }\n",
        );
        assert_eq!(found.len(), 1, "renamed dependency must be found: {found:?}");
        assert_eq!(found[0].kind, "normal");
        Ok(())
    }

    /// A target-gated dependency links just as hard as a plain one.
    #[test]
    fn a_target_specific_dependency_is_discovered() -> TestResult {
        let found = dependents_of(
            "[target.'cfg(unix)'.dev-dependencies]\nperl-tree-sitter-compat = { workspace = true }\n",
        );
        assert_eq!(found.len(), 1, "target dependency must be found: {found:?}");
        assert!(found[0].kind.contains("dev"), "kind should record dev: {}", found[0].kind);
        assert!(
            found[0].kind.contains("cfg(unix)"),
            "kind should record the cfg: {}",
            found[0].kind
        );
        Ok(())
    }

    /// The control for the two above: an unrelated crate, and an alias whose
    /// `package` is something else, must not be counted. Without this a
    /// permissive matcher would look correct.
    #[test]
    fn an_unrelated_dependency_is_not_discovered() -> TestResult {
        let found = dependents_of(concat!(
            "[dependencies]\n",
            "serde = \"1\"\n",
            "tsc = { package = \"perl-tree-sitter-other\" }\n",
            "\n",
            "[target.'cfg(windows)'.dependencies]\n",
            "winapi = \"0.3\"\n",
        ));
        assert!(found.is_empty(), "unrelated dependencies must not count: {found:?}");
        Ok(())
    }

    /// A manifest that inherits a renamed workspace dependency is a Cargo
    /// dependent whose text never names the package. Without the union it would
    /// be simultaneously required as a consumer row (because it is a dependent)
    /// and rejected as stale (because it is not in the reference population) —
    /// a ledger that cannot be written either way.
    #[test]
    fn a_cargo_dependent_is_always_in_the_reference_population() -> TestResult {
        let dependents = vec![CargoDependent {
            manifest: "crates/silent/Cargo.toml".to_string(),
            kind: "normal".to_string(),
        }];
        let references = merge_reference_population(vec!["docs/x.md".to_string()], &dependents);
        assert_eq!(
            references,
            vec!["crates/silent/Cargo.toml".to_string(), "docs/x.md".to_string()],
            "a dependent manifest must be referenceable even with no text hit"
        );

        // And a manifest already found by text must not be duplicated.
        let both =
            merge_reference_population(vec!["crates/silent/Cargo.toml".to_string()], &dependents);
        assert_eq!(both, vec!["crates/silent/Cargo.toml".to_string()]);
        Ok(())
    }

    /// The whole point of the union: such a ledger now validates.
    #[test]
    fn a_silent_cargo_dependent_can_be_reconciled() -> TestResult {
        let l = ledger(
            vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)],
            vec![consumer("crates/silent/Cargo.toml", ReferenceKind::CargoDependency)],
        );
        let dependents = vec![CargoDependent {
            manifest: "crates/silent/Cargo.toml".to_string(),
            kind: "normal".to_string(),
        }];
        let mut d = discovered(vec![export("parse_to_tree")], vec![]);
        d.references = merge_reference_population(Vec::new(), &dependents);
        d.cargo_dependents = dependents;

        validate(&l, &d)
    }

    /// `lib.rs` and the module files must agree about the crate's surface.
    /// Without this control, weakening either parser could remove the guard
    /// silently.
    #[test]
    fn a_reexport_no_module_defines_is_detected() -> TestResult {
        let exports = vec![export("parse_to_tree")];
        let agreeing = vec![("convert".to_string(), "parse_to_tree".to_string())];
        assert!(
            unresolved_reexports(&exports, &agreeing).is_empty(),
            "a re-export the module defines must resolve"
        );

        let disagreeing = vec![
            ("convert".to_string(), "parse_to_tree".to_string()),
            ("convert".to_string(), "vanished".to_string()),
        ];
        assert_eq!(
            unresolved_reexports(&exports, &disagreeing),
            vec![("convert".to_string(), "vanished".to_string())],
            "a re-export no module defines must be reported"
        );
        Ok(())
    }

    /// A member inheriting a renamed workspace dependency names the package
    /// nowhere: the rename lives in the root manifest and the member writes
    /// only `{ workspace = true }`. Without alias resolution it contributes no
    /// Cargo evidence and the crate looks unused.
    #[test]
    fn a_workspace_inherited_rename_is_discovered() -> TestResult {
        let aliases: BTreeSet<String> = ["tsc".to_string()].into_iter().collect();
        let found =
            dependents_with_aliases("[dependencies]\ntsc = { workspace = true }\n", &aliases);
        assert_eq!(found.len(), 1, "workspace-inherited rename must be found: {found:?}");
        Ok(())
    }

    /// The control: the same `{ workspace = true }` shape for an alias that
    /// does NOT resolve to this package must not count.
    #[test]
    fn an_unrelated_workspace_inherited_dependency_is_not_discovered() -> TestResult {
        let aliases: BTreeSet<String> = ["tsc".to_string()].into_iter().collect();
        let found =
            dependents_with_aliases("[dependencies]\nserde = { workspace = true }\n", &aliases);
        assert!(found.is_empty(), "unrelated workspace alias must not count: {found:?}");
        Ok(())
    }

    /// `pub async fn`, `pub unsafe fn`, `pub const fn` and `pub extern "C" fn`
    /// are ordinary public functions; dropping them would silently shrink the
    /// inventory's denominator.
    #[test]
    fn qualified_public_functions_are_discovered() -> TestResult {
        let source = concat!(
            "pub async fn a() {}\n",
            "pub unsafe fn b() {}\n",
            "pub const fn c() {}\n",
            "pub extern \"C\" fn d() {}\n",
            "pub unsafe extern \"C\" fn e() {}\n",
            "pub const MAX: u32 = 1;\n",
            "async fn private_f() {}\n",
        );
        assert_eq!(
            parse_public_items(source),
            vec![
                ("a".to_string(), SymbolKind::Function),
                ("b".to_string(), SymbolKind::Function),
                ("c".to_string(), SymbolKind::Function),
                ("d".to_string(), SymbolKind::Function),
                ("e".to_string(), SymbolKind::Function),
            ],
            "qualified public fns count; a const item and a private fn do not"
        );
        Ok(())
    }

    /// A fixture string that names nothing is not proof that proof exists.
    #[test]
    fn an_unresolvable_fixture_fails_closed() -> TestResult {
        let mut row = symbol("parse_to_tree", Disposition::UniqueAndRequired);
        row.fixture = Some(format!("{CRATE_DIR}/tests/adapter.rs::renamed_away"));
        let l = ledger(vec![row], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("does not resolve to a function"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn an_unexplained_reference_fails_closed() -> TestResult {
        let l = ledger(vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec!["docs/new.md"]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("does not explain it"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_symbol_without_a_row_fails_closed() -> TestResult {
        let l = ledger(vec![], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("has no row for it"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_stale_symbol_row_fails_closed() -> TestResult {
        let l = ledger(vec![symbol("removed_fn", Disposition::UniqueAndRequired)], vec![]);
        let d = discovered(vec![], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("remove the stale row"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_duplicate_symbol_row_fails_closed() -> TestResult {
        let l = ledger(
            vec![
                symbol("parse_to_tree", Disposition::UniqueAndRequired),
                symbol("parse_to_tree", Disposition::AlreadyEquivalent),
            ],
            vec![],
        );
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("duplicate symbol row"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_duplicate_consumer_row_fails_closed() -> TestResult {
        let l = ledger(
            vec![],
            vec![
                consumer("docs/x.md", ReferenceKind::Documentation),
                consumer("docs/x.md", ReferenceKind::Policy),
            ],
        );
        let d = discovered(vec![], vec!["docs/x.md"]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("duplicate consumer row"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn an_unknown_row_without_an_owner_fails_closed() -> TestResult {
        let mut row = symbol("parse_to_tree", Disposition::UnknownBlocking);
        row.canonical_owner = None;
        let l = ledger(vec![row], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("must record `canonical_owner`"), "unexpected error: {err}");
        Ok(())
    }

    /// Even a fully-populated unknown row blocks: "unknown" is not a
    /// disposition the inventory can ship with.
    #[test]
    fn a_well_formed_unknown_row_still_blocks() -> TestResult {
        let l = ledger(vec![symbol("parse_to_tree", Disposition::UnknownBlocking)], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("remain `unknown_blocking`"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_required_row_must_name_its_fixture_and_migration_dependency() -> TestResult {
        let mut row = symbol("parse_to_tree", Disposition::UniqueAndRequired);
        row.fixture = None;
        let l = ledger(vec![row], vec![]);
        let d = discovered(vec![export("parse_to_tree")], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("must record `fixture`"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn a_kind_that_disagrees_with_the_source_fails_closed() -> TestResult {
        let mut row = symbol("TreeError", Disposition::UniqueAndRequired);
        row.kind = SymbolKind::Function;
        let l = ledger(vec![row], vec![]);
        let d = discovered(
            vec![Export {
                module: "convert".to_string(),
                name: "TreeError".to_string(),
                kind: SymbolKind::Enum,
                visibility: Visibility::Public,
                reexported_at_root: true,
            }],
            vec![],
        );

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("but the source declares `enum`"), "unexpected error: {err}");
        Ok(())
    }

    /// A source edit must invalidate the audited dispositions.
    #[test]
    fn a_source_change_invalidates_the_inventory() -> TestResult {
        let l = ledger(vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)], vec![]);
        let mut d = discovered(vec![export("parse_to_tree")], vec![]);
        d.source_digest = "sha256:different".to_string();

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("source changed since"), "unexpected error: {err}");
        Ok(())
    }

    /// The projection must depend on row content, never on ledger order.
    #[test]
    fn rendering_is_independent_of_insertion_order() -> TestResult {
        let a = symbol("aaa", Disposition::UniqueAndRequired);
        let mut b = symbol("zzz", Disposition::UniqueAndRequired);
        b.module = "sexp".to_string();

        let exports = vec![
            Export {
                module: "convert".to_string(),
                name: "aaa".to_string(),
                kind: SymbolKind::Function,
                visibility: Visibility::Public,
                reexported_at_root: true,
            },
            Export {
                module: "sexp".to_string(),
                name: "zzz".to_string(),
                kind: SymbolKind::Function,
                visibility: Visibility::Public,
                reexported_at_root: true,
            },
        ];
        let c1 = consumer("docs/a.md", ReferenceKind::Documentation);
        let c2 = consumer("docs/z.md", ReferenceKind::Policy);

        let forward = ledger(vec![a.clone(), b.clone()], vec![c1.clone(), c2.clone()]);
        let reverse = ledger(vec![b, a], vec![c2, c1]);
        let d = discovered(exports, vec!["docs/a.md", "docs/z.md"]);

        assert_eq!(render_markdown(&forward, &d), render_markdown(&reverse, &d));
        assert_eq!(render_json(&forward, &d)?, render_json(&reverse, &d)?);
        Ok(())
    }

    #[test]
    fn schema_and_package_identity_are_enforced() -> TestResult {
        let mut l = ledger(vec![], vec![]);
        l.schema_version = "tree_sitter_compat_inventory.v2".to_string();
        let d = discovered(vec![], vec![]);
        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("expected `tree_sitter_compat_inventory.v1`"));

        let mut l = ledger(vec![], vec![]);
        l.package = "perl-parser".to_string();
        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("expected `perl-tree-sitter-compat`"));
        Ok(())
    }

    #[test]
    fn issue_references_must_be_numeric() -> TestResult {
        let mut l = ledger(vec![], vec![]);
        l.migration_owner = "the migration issue".to_string();
        let d = discovered(vec![], vec![]);
        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("must be an issue reference"), "got: {err}");
        Ok(())
    }

    // -- source parsing -----------------------------------------------------

    #[test]
    fn modules_and_reexports_are_parsed_from_lib_rs() -> TestResult {
        let lib = "\
pub mod convert;
pub mod sexp;
mod hidden;

pub use convert::{TreeError, parse_to_tree, to_ts_node};
pub use sexp::to_sexp;
";
        // A private `mod` is discovered too, flagged as non-public: it cannot
        // expose public surface but can still hold internal symbols.
        assert_eq!(
            parse_modules(lib),
            vec![
                Module { name: "convert".to_string(), public: true },
                Module { name: "hidden".to_string(), public: false },
                Module { name: "sexp".to_string(), public: true },
            ]
        );
        assert_eq!(
            parse_root_reexports(lib),
            vec![
                ("convert".to_string(), "TreeError".to_string()),
                ("convert".to_string(), "parse_to_tree".to_string()),
                ("convert".to_string(), "to_ts_node".to_string()),
                ("sexp".to_string(), "to_sexp".to_string()),
            ]
        );
        Ok(())
    }

    /// #8880 asks for a disposition per public *or internal* symbol. A restricted
    /// item must therefore be discovered and tagged, not dropped: dropping it
    /// would let the ledger claim a complete surface while the denominator
    /// silently shrank. A module-private item stays out — it is not consumable
    /// from anywhere, so it carries no disposition.
    #[test]
    fn declared_items_are_parsed_and_tagged_by_visibility() -> TestResult {
        let source = "\
pub fn parse_to_tree(source: &str) -> Result<TsNode, TreeError> {}
pub struct TsNode {}
pub enum TreeError {}
pub(crate) fn crate_helper() {}
pub(super) struct SuperHelper {}
pub(in crate::node) enum ScopedHelper {}
fn private() {}
";
        assert_eq!(
            parse_declared_items(source),
            vec![
                ("ScopedHelper".to_string(), SymbolKind::Enum, Visibility::Internal),
                ("SuperHelper".to_string(), SymbolKind::Struct, Visibility::Internal),
                ("TreeError".to_string(), SymbolKind::Enum, Visibility::Public),
                ("TsNode".to_string(), SymbolKind::Struct, Visibility::Public),
                ("crate_helper".to_string(), SymbolKind::Function, Visibility::Internal),
                ("parse_to_tree".to_string(), SymbolKind::Function, Visibility::Public),
            ],
            "all three restricted spellings are internal; a private item is not a symbol"
        );
        Ok(())
    }

    /// The population the ledger's header claims is empty must be *checked*
    /// empty, not asserted. An internal symbol with no row fails the run, and the
    /// error says which visibility it has so the repair is obvious.
    #[test]
    fn an_internal_symbol_without_a_row_fails_closed() -> TestResult {
        let l = ledger(vec![], vec![]);
        let mut internal = export("crate_helper");
        internal.visibility = Visibility::Internal;
        internal.reexported_at_root = false;
        let d = discovered(vec![internal], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(
            err.contains("declares internal symbol `convert::crate_helper`"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    /// The control for the rule above: an internal symbol that *is* recorded
    /// reconciles. Without this, a rule that simply banned internal symbols would
    /// pass the failure test while making the population unrecordable.
    #[test]
    fn a_recorded_internal_symbol_reconciles() -> TestResult {
        let mut row = symbol("crate_helper", Disposition::Unused);
        row.visibility = Visibility::Internal;
        row.canonical_owner = None;
        row.fixture = None;
        row.migration_dependency = None;
        row.removal_condition = None;

        let mut internal = export("crate_helper");
        internal.visibility = Visibility::Internal;
        internal.reexported_at_root = false;

        validate(&ledger(vec![row], vec![]), &discovered(vec![internal], vec![]))
    }

    /// rustfmt breaks a long `impl` header before its brace, and a `where`
    /// clause always does. The type must survive those lines: at depth 0 with no
    /// brace yet, clearing it drops every method in the block.
    #[test]
    fn an_impl_header_that_wraps_before_its_brace_keeps_its_methods() -> TestResult {
        let source = "\
pub struct TsNode {}

impl TsNode
{
    pub fn child_count(&self) -> usize {
        0
    }
}

pub fn free(name: &str) -> String {
    String::new()
}
";
        assert_eq!(
            parse_public_items(source),
            vec![
                ("TsNode".to_string(), SymbolKind::Struct),
                ("TsNode::child_count".to_string(), SymbolKind::Method),
                ("free".to_string(), SymbolKind::Function),
            ],
            "a wrapped impl header still qualifies its methods, and the impl still closes"
        );
        Ok(())
    }

    /// A const-generic argument puts a *balanced* brace group in the header, so
    /// "this line has a brace" is not the same as "the body opened here". The
    /// body arrives on a later line, and the type has to survive to meet it.
    #[test]
    fn a_const_generic_impl_header_keeps_its_methods() -> TestResult {
        let source = "\
impl Grid<{ N + 1 }>
{
    pub fn cells(&self) -> usize {
        0
    }
}

pub fn free() -> usize {
    0
}
";
        assert_eq!(
            parse_public_items(source),
            vec![
                ("Grid::cells".to_string(), SymbolKind::Method),
                ("free".to_string(), SymbolKind::Function),
            ],
            "the braced const argument is part of the type, not the body"
        );
        Ok(())
    }

    /// A const argument that itself spans lines puts a balanced `{ … }` on a
    /// continuation line, where the header's "balanced group that ends the line
    /// is the body" rule would misfire. On a continuation line only a bare `{}`
    /// is a body.
    #[test]
    fn a_multiline_const_generic_header_keeps_its_methods() -> TestResult {
        let source = "\
impl Grid<
    { N + 1 }
>
{
    pub fn cells(&self) -> usize {
        0
    }
}

pub fn free() -> usize {
    0
}
";
        assert_eq!(
            parse_public_items(source),
            vec![
                ("Grid::cells".to_string(), SymbolKind::Method),
                ("free".to_string(), SymbolKind::Function),
            ],
            "a const argument spanning lines is type syntax, not the body"
        );
        Ok(())
    }

    /// An empty body on a continuation line is not always the bare token `{}`:
    /// a where clause can precede it, and the braces can enclose whitespace or a
    /// comment. All of them close the impl, and failing to notice leaves the
    /// header pending and swallows whatever `impl` comes next.
    #[test]
    fn empty_bodies_close_in_every_spelling() -> TestResult {
        for body in ["{}", "{ }", "{ /* nothing to do */ }", "where T: Copy {}"] {
            let source = format!(
                "impl Grid<T>\n    {body}\n\nimpl Other {{\n    pub fn owned(&self) -> usize {{\n        0\n    }}\n}}\n"
            );
            assert_eq!(
                parse_public_items(&source),
                vec![("Other::owned".to_string(), SymbolKind::Method)],
                "an empty body spelled `{body}` must close so the next impl is parsed"
            );
        }
        Ok(())
    }

    /// The control that keeps the rule above from swallowing const arguments: a
    /// brace group with an expression in it is type syntax, whatever it ends.
    #[test]
    fn a_nonempty_brace_group_on_a_continuation_line_is_not_a_body() -> TestResult {
        assert!(!impl_body_opens("{ N + 1 }", 0), "a const argument is not an empty body");
        assert!(!impl_body_opens("where T: Copy", 0), "a bare where clause opens nothing");
        assert!(impl_body_opens("{ }", 0), "whitespace between braces is still an empty body");
        assert!(impl_body_opens("impl Foo {", 1), "an unclosed brace is the body");
        Ok(())
    }

    /// The control the rule above needs: a body written as a bare `{}` on its
    /// own line is still a body. Without this, narrowing the continuation rule
    /// to \"nothing balanced is ever a body\" would leave the header pending and
    /// swallow whatever follows.
    #[test]
    fn a_wrapped_header_with_an_empty_body_on_its_own_line_closes() -> TestResult {
        let source = "\
impl Grid<
    { N + 1 }
>
{}

pub fn free() -> usize {
    0
}
";
        assert_eq!(
            parse_public_items(source),
            vec![("free".to_string(), SymbolKind::Function)],
            "the empty body closes, so the following function stays free"
        );
        Ok(())
    }

    /// The same header with its body opening on the same line must behave
    /// identically. Without this, a rule that keyed off the *last* brace group
    /// rather than the delta could pass the test above and fail here.
    #[test]
    fn a_const_generic_impl_header_with_an_inline_body_keeps_its_methods() -> TestResult {
        let source = "\
impl Grid<{ N + 1 }> {
    pub fn cells(&self) -> usize {
        0
    }
}
";
        assert_eq!(
            parse_public_items(source),
            vec![("Grid::cells".to_string(), SymbolKind::Method)],
            "a const-generic header that opens its body inline still qualifies the method"
        );
        Ok(())
    }

    /// `impl Foo {}` opens and closes on one line, so its net brace delta is
    /// zero. Treating "depth went positive" as the signal that the body opened
    /// leaves the header pending forever, which swallows the *next* `impl` and
    /// hands its methods the stale type. This is misattribution rather than
    /// omission — the worse of the two, because the row looks plausible.
    #[test]
    fn an_empty_impl_does_not_capture_the_next_impls_methods() -> TestResult {
        let source = "\
pub struct Foo {}
pub struct Bar {}

impl Foo {}

impl Bar {
    pub fn only_on_bar(&self) -> usize {
        0
    }
}
";
        assert_eq!(
            parse_public_items(source),
            vec![
                ("Bar".to_string(), SymbolKind::Struct),
                ("Bar::only_on_bar".to_string(), SymbolKind::Method),
                ("Foo".to_string(), SymbolKind::Struct),
            ],
            "the method belongs to Bar, not to the empty Foo impl"
        );
        Ok(())
    }

    /// The same shape with an empty *trait* impl, which is what this crate
    /// actually contains (`impl std::error::Error for TreeError {}`). Here the
    /// pending type is `None`, so the failure is omission rather than
    /// misattribution — the following inherent block loses its methods entirely.
    #[test]
    fn an_empty_trait_impl_does_not_suppress_the_next_impl() -> TestResult {
        let source = "\
impl std::error::Error for TreeError {}

impl TsNode {
    pub fn child_count(&self) -> usize {
        0
    }
}
";
        assert_eq!(
            parse_public_items(source),
            vec![("TsNode::child_count".to_string(), SymbolKind::Method)],
            "the inherent impl after an empty trait impl is still recognised"
        );
        Ok(())
    }

    /// The control for the rule above: a trait impl whose header wraps must stay
    /// excluded. An implementation that simply kept the last seen type across
    /// lines would attribute these to the preceding inherent block.
    #[test]
    fn a_wrapped_trait_impl_header_contributes_no_methods() -> TestResult {
        let source = "\
impl TsNode
{
    pub fn kept(&self) -> usize {
        0
    }
}

impl fmt::Display
    for TsNode
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, \"x\")
    }
}
";
        assert_eq!(
            parse_public_items(source),
            vec![("TsNode::kept".to_string(), SymbolKind::Method)],
            "the trait impl adds nothing and does not resurrect the earlier type"
        );
        Ok(())
    }

    /// A workspace alias can identify the package by path alone, naming it
    /// nowhere. Missing it drops every member that inherits the alias from the
    /// Cargo plane — the plane that gates `unused` — so a real link-time
    /// consumer could sit behind a false `unused` disposition.
    ///
    /// This closes an asymmetry in the earlier path fix: `collect_dependents`
    /// learned to resolve a path in a *member* manifest, but the root alias
    /// table was still matched by name only.
    #[test]
    fn a_workspace_alias_identified_only_by_path_resolves() -> TestResult {
        let entry: toml::Value =
            toml::from_str::<toml::Table>(r#"path = "crates/perl-tree-sitter-compat""#)?.into();
        assert!(
            workspace_alias_resolves("tsc", &entry),
            "a path-only alias identifies the package"
        );
        Ok(())
    }

    /// The controls. A path to a different crate must not resolve — including
    /// the prefix case, which a `starts_with` implementation would admit — and
    /// an alias carrying no identifying evidence at all must not either.
    #[test]
    fn an_unrelated_workspace_alias_does_not_resolve() -> TestResult {
        for spelling in [
            r#"path = "crates/perl-parser""#,
            r#"path = "crates/perl-tree-sitter-compat-extras""#,
            r#"version = "1.0""#,
        ] {
            let entry: toml::Value = toml::from_str::<toml::Table>(spelling)?.into();
            assert!(
                !workspace_alias_resolves("other", &entry),
                "`{spelling}` must not identify the package"
            );
        }
        // The two name-based spellings still resolve, so the added clause did
        // not narrow what was already working.
        let named: toml::Value =
            toml::from_str::<toml::Table>(r#"package = "perl-tree-sitter-compat""#)?.into();
        assert!(workspace_alias_resolves("tsc", &named));
        let bare: toml::Value = toml::from_str::<toml::Table>(r#"version = "0.17""#)?.into();
        assert!(workspace_alias_resolves(PACKAGE, &bare));
        Ok(())
    }

    /// A private `mod` can hold `pub(crate)` items. Matching only `pub mod`
    /// would skip the file entirely, so the inventory would certify complete
    /// internal coverage having never opened it.
    #[test]
    fn a_private_module_is_discovered_so_its_internal_symbols_are_not_skipped() -> TestResult {
        let modules = parse_modules("pub mod convert;\nmod hidden;\n");
        assert!(
            modules.contains(&Module { name: "hidden".to_string(), public: false }),
            "a private module must be scanned: {modules:?}"
        );
        Ok(())
    }

    /// Visibility is Rust reachability, not the declaration keyword. A `pub` item
    /// in a private module leaves the crate only through a root `pub use`;
    /// recording it as public would tell #8889 to migrate something no consumer
    /// can name.
    #[test]
    fn a_pub_item_in_a_private_module_is_public_only_when_reexported() -> TestResult {
        let exported = Export {
            module: "hidden".to_string(),
            name: "Reachable".to_string(),
            kind: SymbolKind::Struct,
            visibility: Visibility::Public,
            reexported_at_root: true,
        };
        let sealed = Export {
            module: "hidden".to_string(),
            name: "Sealed".to_string(),
            kind: SymbolKind::Struct,
            visibility: Visibility::Internal,
            reexported_at_root: false,
        };
        // `unresolved_reexports` must accept the re-exported one and still
        // reject a name only an internal item carries.
        let reexports = vec![
            ("hidden".to_string(), "Reachable".to_string()),
            ("hidden".to_string(), "Sealed".to_string()),
        ];
        assert_eq!(
            unresolved_reexports(&[exported, sealed], &reexports),
            vec![("hidden".to_string(), "Sealed".to_string())],
            "an internal item does not satisfy a public re-export"
        );
        Ok(())
    }

    /// The fourth dependency spelling: `compat = { path = "..." }` names the
    /// package nowhere, because Cargo reads the identity from the manifest the
    /// path points at.
    #[test]
    fn a_path_dependency_is_discovered() -> TestResult {
        assert!(
            path_targets_package("crates/perl-lsp-rs/Cargo.toml", "../perl-tree-sitter-compat"),
            "a sibling path dependency links the crate"
        );
        assert!(
            path_targets_package("tools/thing/Cargo.toml", "../../crates/perl-tree-sitter-compat"),
            "a deeper relative path resolves the same way"
        );
        Ok(())
    }

    /// The control. Without it, a matcher that admitted every `path` entry would
    /// pass the test above and report the whole workspace as a dependent.
    #[test]
    fn an_unrelated_path_dependency_is_not_discovered() -> TestResult {
        assert!(!path_targets_package("crates/perl-lsp-rs/Cargo.toml", "../perl-parser"));
        assert!(!path_targets_package(
            "crates/perl-lsp-rs/Cargo.toml",
            "../perl-tree-sitter-compat-extras"
        ));
        Ok(())
    }

    /// A symbol that changes visibility keeps its name, so nothing else in the
    /// reconciliation would notice. Its disposition was audited against the old
    /// reachability and has to be re-examined.
    #[test]
    fn a_visibility_that_disagrees_with_the_source_fails_closed() -> TestResult {
        let l = ledger(vec![symbol("parse_to_tree", Disposition::UniqueAndRequired)], vec![]);
        let mut moved = export("parse_to_tree");
        moved.visibility = Visibility::Internal;
        let d = discovered(vec![moved], vec![]);

        let err = validate(&l, &d).unwrap_err().to_string();
        assert!(err.contains("but the source declares it `internal`"), "unexpected error: {err}");
        Ok(())
    }

    /// An inherent method is part of the public surface, but it is not a free
    /// function. Recording it as one would send the migration looking for a
    /// module-level symbol the crate does not have.
    #[test]
    fn inherent_methods_are_qualified_not_reported_as_free_functions() -> TestResult {
        let source = "\
pub struct TsNode {
    pub kind: String,
}

impl TsNode {
    /// Doc comment with a stray brace { that must not shift depth.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
}

impl fmt::Display for TsNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, \"x\")
    }
}

pub fn pascal_to_snake(name: &str) -> String {
    String::new()
}
";
        let items = parse_public_items(source);
        assert_eq!(
            items,
            vec![
                ("TsNode".to_string(), SymbolKind::Struct),
                ("TsNode::child_count".to_string(), SymbolKind::Method),
                ("pascal_to_snake".to_string(), SymbolKind::Function),
            ],
            "methods must be qualified and free functions must stay free"
        );
        Ok(())
    }

    /// The real failure this scanner exists to prevent: `convert.rs` holds
    /// `"{".repeat(5000)` inside its test module. Counting that brace leaves
    /// the scanner one level deep forever, so every later item is dropped from
    /// discovery and never needs a ledger row — the inventory would silently
    /// stop covering the crate.
    #[test]
    fn an_unbalanced_brace_in_a_literal_does_not_hide_later_items() -> TestResult {
        let source = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn rejects_deep_nesting() {\n",
            "        let bad = \"{\".repeat(5000);\n",
            "        let brace = '{';\n",
            "        let raw = r#\"} } {\"#;\n",
            "    }\n",
            "}\n",
            "\n",
            "pub fn declared_after_the_test_module() -> u32 {\n",
            "    42\n",
            "}\n",
        );
        assert_eq!(
            parse_public_items(source),
            vec![("declared_after_the_test_module".to_string(), SymbolKind::Function)],
            "an item after an unbalanced literal brace must still be discovered"
        );
        Ok(())
    }

    /// Rust block comments nest. Ending sanitization at the inner `*/` would
    /// expose the rest of the outer comment as code, so a brace in that tail
    /// would shift depth and hide every later item — the same failure mode as
    /// the unbalanced literal brace, reached a different way.
    #[test]
    fn a_nested_block_comment_does_not_hide_later_items() -> TestResult {
        let source = concat!(
            "/* outer\n",
            "   /* inner */\n",
            "   still commented { and an unbalanced brace\n",
            "*/\n",
            "pub fn declared_after_the_comment() -> u32 {\n",
            "    7\n",
            "}\n",
        );
        assert_eq!(
            parse_public_items(source),
            vec![("declared_after_the_comment".to_string(), SymbolKind::Function)],
            "an item after a nested block comment must still be discovered"
        );
        Ok(())
    }

    /// A lifetime is not a char literal; treating `'tree` as one would swallow
    /// the rest of the signature.
    #[test]
    fn a_lifetime_is_not_mistaken_for_a_char_literal() -> TestResult {
        let source = concat!(
            "pub struct Node<'tree> {\n",
            "    inner: &'tree str,\n",
            "}\n",
            "\n",
            "pub fn borrow<'tree>(node: &'tree Node<'tree>) -> &'tree str {\n",
            "    node.inner\n",
            "}\n",
        );
        assert_eq!(
            parse_public_items(source),
            vec![
                ("Node".to_string(), SymbolKind::Struct),
                ("borrow".to_string(), SymbolKind::Function),
            ]
        );
        Ok(())
    }

    /// A generic inherent impl still names its type, and the generic parameter
    /// list is not mistaken for it.
    #[test]
    fn a_generic_inherent_impl_is_attributed_to_its_type() -> TestResult {
        let source = "\
impl<T> Wrapper<T> {
    pub fn get(&self) -> &T {
        &self.inner
    }
}
";
        assert_eq!(
            parse_public_items(source),
            vec![("Wrapper::get".to_string(), SymbolKind::Method)]
        );
        Ok(())
    }

    // -- checked-in state ---------------------------------------------------

    /// The committed ledger must reconcile against the real crate, and the
    /// committed projection must be current. This is the integration control
    /// that keeps the other tests from passing over a fictional repository.
    #[test]
    fn the_checked_in_inventory_is_complete_and_current() -> TestResult {
        let root = project_root()?;
        let ledger = load(&root)?;
        let discovered = discover(&root)?;
        validate(&ledger, &discovered)?;

        let expected = render_markdown(&ledger, &discovered);
        let committed = fs::read_to_string(root.join(PROJECTION_PATH))
            .wrap_err_with(|| format!("failed to read {PROJECTION_PATH}"))?;
        assert_eq!(
            normalize_newlines(&committed),
            expected,
            "{PROJECTION_PATH} is stale; run `cargo xtask compat-inventory`"
        );
        Ok(())
    }
}
