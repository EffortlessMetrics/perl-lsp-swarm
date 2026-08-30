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
use std::path::Path;
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

/// One public item found in the crate's own source.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Export {
    pub module: String,
    pub name: String,
    pub kind: SymbolKind,
    /// Whether `lib.rs` re-exports it at the crate root.
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
        let module_path = root.join(CRATE_DIR).join(format!("src/{module}.rs"));
        let source = fs::read_to_string(&module_path)
            .wrap_err_with(|| format!("failed to read {}", module_path.display()))?;
        for (name, kind) in parse_public_items(&source) {
            let reexported_at_root = root_reexports.iter().any(|(m, n)| m == module && n == &name);
            exports.push(Export { module: module.clone(), name, kind, reexported_at_root });
        }
    }

    // A root re-export that no module actually defines means `lib.rs` and the
    // module files disagree; the inventory must not paper over that.
    for (module, name) in &root_reexports {
        if !exports.iter().any(|e| &e.module == module && &e.name == name) {
            bail!(
                "{CRATE_DIR}/src/lib.rs re-exports `{module}::{name}` but no public item of that \
                 name was found in {CRATE_DIR}/src/{module}.rs"
            );
        }
    }
    exports.sort();

    let mut cargo_dependents = discover_cargo_dependents(root, &tracked)?;
    cargo_dependents.sort();

    let mut references = discover_references(root, &tracked)?;
    references.sort();

    let (source_files, source_digest) = digest_sources(root, &tracked)?;

    Ok(Discovered { exports, cargo_dependents, references, source_files, source_digest })
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

/// `pub mod <name>;` declarations in `lib.rs`.
pub fn parse_modules(lib_rs: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for line in lib_rs.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pub mod ")
            && let Some(name) = rest.strip_suffix(';')
        {
            let name = name.trim();
            if !name.is_empty() {
                modules.push(name.to_string());
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
    for line in lib_rs.lines() {
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

/// Public items declared in one module file.
///
/// Brace depth is tracked so an inherent `pub fn` inside `impl TsNode` is
/// recorded as the method `TsNode::child_count` rather than as a free function
/// the module does not have. Getting that wrong would send #8889 looking for a
/// module-level symbol that does not exist.
pub fn parse_public_items(source: &str) -> Vec<(String, SymbolKind)> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut impl_type: Option<String> = None;

    for line in source.lines() {
        let trimmed = line.trim();
        // Comment lines cannot declare an item, and their braces are prose.
        if trimmed.starts_with("//") {
            continue;
        }

        if depth == 0 && (trimmed.starts_with("impl ") || trimmed.starts_with("impl<")) {
            impl_type = inherent_impl_type(trimmed);
        } else if let Some(item) = parse_item(trimmed) {
            let (name, kind) = item;
            if depth == 0 {
                out.push((name, kind));
            } else if depth == 1
                && kind == SymbolKind::Function
                && let Some(ty) = &impl_type
            {
                out.push((format!("{ty}::{name}"), SymbolKind::Method));
            }
        }

        depth += brace_delta(trimmed);
        if depth <= 0 {
            depth = 0;
            impl_type = None;
        }
    }

    out.sort();
    out.dedup();
    out
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

fn parse_item(trimmed: &str) -> Option<(String, SymbolKind)> {
    let rest = trimmed.strip_prefix("pub ")?;
    // `pub(crate)` and friends are not part of the crate's public surface.
    if rest.starts_with('(') {
        return None;
    }
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
    if name.is_empty() { None } else { Some((name, kind)) }
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
        let Ok(value) = text.parse::<toml::Value>() else {
            continue;
        };
        for (table, kind) in [
            ("dependencies", "normal"),
            ("dev-dependencies", "dev"),
            ("build-dependencies", "build"),
        ] {
            if value
                .get(table)
                .and_then(toml::Value::as_table)
                .is_some_and(|t| t.contains_key(PACKAGE))
            {
                out.push(CargoDependent { manifest: path.clone(), kind: kind.to_string() });
            }
        }
    }
    Ok(out)
}

/// Tracked files mentioning the package name or its Rust module path.
fn discover_references(root: &Path, tracked: &[String]) -> Result<Vec<String>> {
    let mut out = BTreeSet::new();
    for path in tracked {
        if SELF_ARTIFACTS.contains(&path.as_str()) {
            continue;
        }
        let full = root.join(path);
        let Ok(text) = fs::read_to_string(&full) else {
            // Binary or unreadable files cannot carry a Rust reference.
            continue;
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

    let discovered_kinds: BTreeMap<(String, String), SymbolKind> =
        discovered.exports.iter().map(|e| ((e.module.clone(), e.name.clone()), e.kind)).collect();

    for export in &discovered.exports {
        let key = (export.module.clone(), export.name.clone());
        if !seen.contains(&key) {
            bail!(
                "{PACKAGE} exposes `{}::{}` but {LEDGER_PATH} has no row for it; every public \
                 symbol needs exactly one disposition",
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
            Some(kind) if *kind != row.kind => bail!(
                "{LEDGER_PATH} records `{}::{}` as `{}` but the source declares `{}`",
                row.module,
                row.name,
                row.kind.as_str(),
                kind.as_str()
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

    out.push_str("## Public symbols\n\n");
    out.push_str(
        "| Symbol | Kind | Root re-export | Disposition | Canonical owner | Fixture | Migration | Removal condition | Note |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in &symbols {
        let reexported = discovered
            .exports
            .iter()
            .find(|e| e.module == row.module && e.name == row.name)
            .is_some_and(|e| e.reexported_at_root);
        let _ = writeln!(
            out,
            "| `{}::{}` | `{}` | {} | `{}` | {} | {} | {} | {} | {} |",
            row.module,
            row.name,
            row.kind.as_str(),
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

    fn symbol(name: &str, disposition: Disposition) -> SymbolRow {
        SymbolRow {
            name: name.to_string(),
            module: "convert".to_string(),
            kind: SymbolKind::Function,
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
        }
    }

    fn export(name: &str) -> Export {
        Export {
            module: "convert".to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
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
                reexported_at_root: true,
            },
            Export {
                module: "sexp".to_string(),
                name: "zzz".to_string(),
                kind: SymbolKind::Function,
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

pub use convert::{TreeError, parse_to_tree, to_ts_node};
pub use sexp::to_sexp;
";
        assert_eq!(parse_modules(lib), vec!["convert", "sexp"]);
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

    #[test]
    fn public_items_are_parsed_and_restricted_visibility_is_excluded() -> TestResult {
        let source = "\
pub fn parse_to_tree(source: &str) -> Result<TsNode, TreeError> {}
pub struct TsNode {}
pub enum TreeError {}
pub(crate) fn helper() {}
fn private() {}
";
        assert_eq!(
            parse_public_items(source),
            vec![
                ("TreeError".to_string(), SymbolKind::Enum),
                ("TsNode".to_string(), SymbolKind::Struct),
                ("parse_to_tree".to_string(), SymbolKind::Function),
            ]
        );
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
