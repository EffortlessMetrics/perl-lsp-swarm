//! Bounded source-reachability census for the LSP `initialize` path (#10040).
//!
//! The ledger in [`super::rows`] *declares* what each initialize-reachable
//! operation does. This module *derives* the same facts independently from the
//! current source, so a row cannot be validated by the prose that produced it.
//!
//! # Resolution discipline
//!
//! xtask has no compiler frontend and must not grow one, so call edges are
//! resolved by name. Bare name matching is not good enough: `handle_initialize`
//! names both the LSP request handler and an unrelated DAP handler, and names
//! like `new` or `var` collide workspace-wide. Following those edges would
//! attribute DAP TCP validation to the LSP initialize path.
//!
//! An edge is therefore followed only when the call site resolves it
//! unambiguously, narrowing by call syntax first, then locality:
//!
//! 1. a single definition of that name exists in the scanned crates; or
//! 2. exactly one definition sits in the calling file; or
//! 3. for a *path* call only, exactly one definition sits in the calling crate.
//!
//! A method call stops at the calling file. Its receiver's type is unknown, so a
//! crate-wide fallback invents edges for receivers this census does not index —
//! `params.get("capabilities")` on a `serde_json::Value` otherwise resolved to
//! whichever single `get` method the crate defined.
//!
//! Resolution is per call site, so a name with several definitions is not
//! automatically dropped: see [`Census::colliding_names`]. That makes the census
//! an under-approximation of edges, and a deliberately precise one.
//!
//! Lock acquisition is intentionally *not* derived. Interior-mutability locks
//! appear on essentially every server method, so a derived lock signal would be
//! true everywhere and discriminate nothing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use syn::visit::Visit;

/// Crates whose sources participate in the census.
pub const SCANNED_CRATES: &[&str] =
    &["perl-lsp-rs", "perl-lsp-rs-core", "perl-workspace", "perl-workspace-core", "perl-dap"];

/// Maximum call-graph depth walked from a census root.
pub const MAX_DEPTH: usize = 8;

/// Maximum number of distinct functions expanded during one traversal.
pub const MAX_VISITED: usize = 20_000;

/// A blocking or ambient-state exposure derived from source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exposure {
    /// Reads or stats the filesystem.
    Filesystem,
    /// Resolves an executable through `PATH`.
    PathLookup,
    /// Spawns a child process.
    ProcessSpawn,
    /// Performs network I/O.
    Network,
    /// Reads process environment variables.
    EnvRead,
}

impl Exposure {
    /// Stable lowercase identifier used in ledger rows and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::PathLookup => "path_lookup",
            Self::ProcessSpawn => "process_spawn",
            Self::Network => "network",
            Self::EnvRead => "env_read",
        }
    }

    /// Every derived exposure, in stable order.
    pub const fn all() -> [Self; 5] {
        [Self::Filesystem, Self::PathLookup, Self::ProcessSpawn, Self::Network, Self::EnvRead]
    }
}

/// One function discovered in the scanned sources.
#[derive(Debug, Clone)]
pub struct FunctionRecord {
    /// Function name as written.
    pub name: String,
    /// Repository-relative file it was found in.
    pub file: String,
    /// Whether the function takes a `self` receiver.
    ///
    /// This separates the two `command_exists` definitions that share one file:
    /// a `&self` method and a free function. `detect_tool` reaches the free one
    /// through a path call, and without the distinction that edge is ambiguous
    /// and the perltidy/perlcritic PATH lookup disappears from the census.
    pub is_method: bool,
    /// Names reached through method-call syntax.
    pub method_calls: BTreeSet<String>,
    /// Names reached through path-call syntax or function-reference arguments.
    pub path_calls: BTreeSet<String>,
    /// Exposures detected directly in the body, not through callees.
    pub exposures: BTreeSet<Exposure>,
}

impl FunctionRecord {
    /// Every callee name, regardless of call syntax.
    pub fn all_calls(&self) -> impl Iterator<Item = (&String, CallKind)> {
        self.method_calls
            .iter()
            .map(|name| (name, CallKind::Method))
            .chain(self.path_calls.iter().map(|name| (name, CallKind::Path)))
    }
}

/// How a callee was referenced at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// `receiver.name(..)`
    Method,
    /// `path::name(..)` or `name` passed as a function reference.
    Path,
}

/// How an exposure was reached from a census root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposureWitness {
    /// The exposure that was reached.
    pub exposure: Exposure,
    /// Call chain from the root to the function carrying the exposure.
    pub chain: Vec<String>,
}

impl ExposureWitness {
    /// Render the witness chain for error messages.
    pub fn render(&self) -> String {
        format!("{} via {}", self.exposure.label(), self.chain.join(" -> "))
    }
}

/// Failure while building the census.
#[derive(Debug)]
pub struct CensusError {
    /// Human-readable cause.
    pub message: String,
}

impl std::fmt::Display for CensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CensusError {}

/// A qualified, name-resolved view of the scanned sources.
#[derive(Debug, Clone, Default)]
pub struct Census {
    funcs: Vec<FunctionRecord>,
    by_name: BTreeMap<String, Vec<usize>>,
    ambiguous: BTreeSet<String>,
    methods: BTreeSet<String>,
    unparsable: Vec<(String, String)>,
}

impl Census {
    /// Build a census from `(repo_relative_path, source_text)` pairs.
    ///
    /// This is the seam the falsifier tests drive: a synthetic codebase can
    /// place a process spawn several hops beneath a root and assert the checker
    /// still attributes it.
    pub fn from_sources(sources: &[(String, String)]) -> Self {
        let mut funcs: Vec<FunctionRecord> = Vec::new();
        let mut methods: BTreeSet<String> = BTreeSet::new();
        let mut unparsable: Vec<(String, String)> = Vec::new();
        let mut test_only_modules: BTreeSet<String> = BTreeSet::new();

        for (path, text) in sources {
            let parsed = match syn::parse_file(text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    // A source this census cannot read is a shrunken
                    // denominator, not an absent one. Recording it lets the
                    // checker fail closed instead of passing on a partial view.
                    unparsable.push((path.clone(), error.to_string()));
                    continue;
                }
            };
            collect_test_only_modules(&parsed, &mut test_only_modules);
            let mut collector = FnCollector { file: path.clone(), found: Vec::new() };
            collector.visit_file(&parsed);
            funcs.extend(collector.found);

            let mut literals = MethodLiteralCollector { found: &mut methods };
            literals.visit_file(&parsed);
        }

        // A file reached only through a `#[cfg(test)] mod name;` declaration is
        // test-only even though it parses as an ordinary module of its own.
        // Keeping it would let a test helper's name suppress or redirect a real
        // production edge.
        funcs.retain(|record| !is_test_only_file(&record.file, &test_only_modules));
        funcs.sort_by(|left, right| (&left.file, &left.name).cmp(&(&right.file, &right.name)));

        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, record) in funcs.iter().enumerate() {
            by_name.entry(record.name.clone()).or_default().push(index);
        }
        let ambiguous = by_name
            .iter()
            .filter(|(_, indices)| indices.len() > 1)
            .map(|(name, _)| name.clone())
            .collect();

        Self { funcs, by_name, ambiguous, methods, unparsable }
    }

    /// Build a census by reading the allowlisted crates under `workspace_root`.
    pub fn from_workspace(workspace_root: &Path) -> Result<Self, CensusError> {
        let mut sources = Vec::new();
        for krate in SCANNED_CRATES {
            let src = workspace_root.join("crates").join(krate).join("src");
            if !src.is_dir() {
                return Err(CensusError {
                    message: format!("census crate source directory is missing: {}", src.display()),
                });
            }
            collect_rust_sources(&src, workspace_root, &mut sources)?;
        }
        if sources.is_empty() {
            return Err(CensusError { message: "census found no Rust sources".to_string() });
        }
        sources.sort();
        Ok(Self::from_sources(&sources))
    }

    /// Names carrying more than one definition in the scanned set.
    ///
    /// This is a raw definition-collision count, **not** a count of dropped
    /// edges. [`Census::resolve_edge`] never consults this set: it narrows each
    /// call site independently by call kind, then by file, then (for path calls)
    /// by crate, so a colliding name is often still resolved. `command_exists`
    /// has two definitions in one file and appears here, yet `detect_tool`
    /// resolves to the free function and its PATH lookup is attributed.
    /// Reporting this as "edges not traversed" would overstate the blind spot.
    pub fn colliding_names(&self) -> &BTreeSet<String> {
        &self.ambiguous
    }

    /// Total functions indexed.
    pub fn len(&self) -> usize {
        self.funcs.len()
    }

    /// Whether the census indexed no functions at all.
    pub fn is_empty(&self) -> bool {
        self.funcs.is_empty()
    }

    /// Resolve an exact `(file, name)` citation.
    ///
    /// Returns `None` when the pair matches more than one definition. One file
    /// can hold both a `&self` method and a free function of the same name —
    /// `command_exists` does — and silently binding a row to whichever came
    /// first would let it validate against, and derive exposure from, the wrong
    /// definition.
    pub fn resolve(&self, file: &str, name: &str) -> Option<usize> {
        let matches: Vec<usize> = self
            .by_name
            .get(name)?
            .iter()
            .copied()
            .filter(|index| self.funcs.get(*index).is_some_and(|record| record.file == file))
            .collect();
        match matches.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// How many definitions a `(file, name)` citation matches.
    pub fn citation_arity(&self, file: &str, name: &str) -> usize {
        self.by_name
            .get(name)
            .map(|indices| {
                indices
                    .iter()
                    .filter(|index| {
                        self.funcs.get(**index).is_some_and(|record| record.file == file)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Sources the census could not parse, as `(path, parse error)`.
    ///
    /// Never empty-and-ignored: [`super::ledger_errors`] turns each entry into a
    /// finding, so a file that becomes unreadable shrinks the denominator
    /// loudly rather than silently.
    pub fn unparsable_sources(&self) -> &[(String, String)] {
        &self.unparsable
    }

    /// Protocol method names appearing as string literals in scanned source.
    ///
    /// A ledger row's `side_effects` naming a notification it does not actually
    /// send is the same stale-prose failure this module exists to catch, so the
    /// names are derived rather than trusted.
    pub fn declares_method(&self, method: &str) -> bool {
        self.methods.contains(method)
    }

    /// Files a name was found in, sorted.
    pub fn files_for(&self, name: &str) -> Vec<String> {
        let mut files: Vec<String> = self
            .by_name
            .get(name)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|index| self.funcs.get(*index))
                    .map(|record| record.file.clone())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files.dedup();
        files
    }

    /// The record behind an index.
    pub fn record(&self, index: usize) -> Option<&FunctionRecord> {
        self.funcs.get(index)
    }

    /// A stable `file::name` label used in witnesses and findings.
    pub fn qualified(&self, index: usize) -> String {
        self.funcs
            .get(index)
            .map(|record| format!("{}::{}", record.file, record.name))
            .unwrap_or_else(|| format!("<unknown #{index}>"))
    }

    /// Resolve one call edge from `from` to a callee name, or `None` when the
    /// name is ambiguous or unknown.
    fn resolve_edge(&self, from: usize, callee: &str, kind: CallKind) -> Option<usize> {
        // Self-recursion adds no reachability, and keeping the caller in the
        // candidate set would let a same-file preference resolve
        // `server.auto_initialize_for_compat(..)` back to the free function of
        // the same name that contains the call.
        let mut candidates: Vec<usize> =
            self.by_name.get(callee)?.iter().copied().filter(|index| *index != from).collect();
        if candidates.is_empty() {
            return None;
        }

        // Call syntax narrows the candidate set before locality does.
        let wants_method = kind == CallKind::Method;
        let by_kind: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|index| {
                self.funcs.get(*index).is_some_and(|record| record.is_method == wants_method)
            })
            .collect();
        if !by_kind.is_empty() {
            candidates = by_kind;
        }

        if candidates.len() == 1 {
            return candidates.first().copied();
        }
        let origin = self.funcs.get(from)?;

        let same_file: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|index| self.funcs.get(*index).is_some_and(|record| record.file == origin.file))
            .collect();
        if same_file.len() == 1 {
            return same_file.first().copied();
        }

        // A method call resolves no further than the calling file. The
        // receiver's type is unknown, so a crate-wide fallback invents an edge
        // whenever the real receiver belongs to a type this census does not
        // index. `handle_initialize` calls `params.get("capabilities")` on a
        // `serde_json::Value`; with a crate-wide fallback that resolved to
        // whichever single `get` method the crate happened to define, and
        // manufactured a path from initialize into per-request document work
        // that initialize never performs.
        if kind == CallKind::Method {
            return None;
        }

        let origin_crate = crate_of(&origin.file);
        let same_crate: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|index| {
                self.funcs.get(*index).is_some_and(|record| crate_of(&record.file) == origin_crate)
            })
            .collect();
        if same_crate.len() == 1 {
            return same_crate.first().copied();
        }
        None
    }

    /// Whether an edge may be followed at all.
    ///
    /// A method call's receiver type is unknowable from a name, so a short
    /// generic name like `value` or `item` can otherwise resolve to whatever
    /// single definition happens to exist elsewhere in the workspace. Requiring
    /// method edges to stay inside the calling crate removes that whole class of
    /// invented cross-crate chains. Path calls keep module qualification at the
    /// call site, so they may cross crates.
    fn edge_is_admissible(&self, from: usize, to: usize, kind: CallKind) -> bool {
        if kind == CallKind::Path {
            return true;
        }
        let (Some(origin), Some(target)) = (self.funcs.get(from), self.funcs.get(to)) else {
            return false;
        };
        crate_of(&origin.file) == crate_of(&target.file)
    }

    /// Function indices transitively reachable from `root`, with first-reach
    /// depth.
    pub fn reachable_from(&self, root: usize, max_depth: usize) -> BTreeMap<usize, usize> {
        let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        seen.insert(root, 0);
        queue.push_back((root, 0));

        while let Some((index, depth)) = queue.pop_front() {
            if depth >= max_depth || seen.len() >= MAX_VISITED {
                continue;
            }
            let Some(record) = self.funcs.get(index) else {
                continue;
            };
            for (callee, kind) in record.all_calls() {
                let Some(next) = self.resolve_edge(index, callee, kind) else {
                    continue;
                };
                if !self.edge_is_admissible(index, next, kind) {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(slot) = seen.entry(next) {
                    slot.insert(depth + 1);
                    queue.push_back((next, depth + 1));
                }
            }
        }
        seen
    }

    /// Exposures transitively reachable from `root`, each with one shortest
    /// witness chain.
    ///
    /// The witness names the helper that carries the exposure, not merely the
    /// operation that owns it, which is what makes a finding actionable.
    pub fn transitive_exposures(
        &self,
        root: usize,
        max_depth: usize,
    ) -> BTreeMap<Exposure, ExposureWitness> {
        let mut witnesses: BTreeMap<Exposure, ExposureWitness> = BTreeMap::new();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<Vec<usize>> = VecDeque::new();

        seen.insert(root);
        queue.push_back(vec![root]);

        while let Some(chain) = queue.pop_front() {
            let Some(index) = chain.last().copied() else {
                continue;
            };
            let Some(record) = self.funcs.get(index) else {
                continue;
            };
            for exposure in &record.exposures {
                witnesses.entry(*exposure).or_insert_with(|| ExposureWitness {
                    exposure: *exposure,
                    chain: chain.iter().map(|step| self.qualified(*step)).collect(),
                });
            }
            if chain.len() > max_depth || seen.len() >= MAX_VISITED {
                continue;
            }
            for (callee, kind) in record.all_calls() {
                let Some(next) = self.resolve_edge(index, callee, kind) else {
                    continue;
                };
                if !self.edge_is_admissible(index, next, kind) {
                    continue;
                }
                if seen.contains(&next) {
                    continue;
                }
                seen.insert(next);
                let mut extended = chain.clone();
                extended.push(next);
                queue.push_back(extended);
            }
        }
        witnesses
    }

    /// Exposures written directly in one function body.
    pub fn direct_exposures(&self, index: usize) -> BTreeSet<Exposure> {
        self.funcs.get(index).map(|record| record.exposures.clone()).unwrap_or_default()
    }
}

fn crate_of(file: &str) -> &str {
    file.strip_prefix("crates/").and_then(|rest| rest.split('/').next()).unwrap_or("")
}

fn collect_rust_sources(
    dir: &Path,
    workspace_root: &Path,
    out: &mut Vec<(String, String)>,
) -> Result<(), CensusError> {
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.map_err(|error| CensusError {
            message: format!("failed to walk {}: {error}", dir.display()),
        })?;
        let path = entry.path();
        if !path.is_file() || path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(path).map_err(|error| CensusError {
            message: format!("failed to read {}: {error}", path.display()),
        })?;
        let rel = path.strip_prefix(workspace_root).unwrap_or(path);
        out.push((rel.to_string_lossy().replace('\\', "/"), text));
    }
    Ok(())
}

struct FnCollector {
    file: String,
    found: Vec<FunctionRecord>,
}

/// Whether an item carries `#[cfg(test)]`.
///
/// Test modules are excluded: the census describes production reachability, and
/// a test helper that shells out must not be attributed to the initialize path.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

impl<'ast> Visit<'ast> for FnCollector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        self.found.push(summarize(&self.file, node.sig.ident.to_string(), false, &node.block));
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let is_method = node.sig.receiver().is_some();
        self.found.push(summarize(&self.file, node.sig.ident.to_string(), is_method, &node.block));
        syn::visit::visit_impl_item_fn(self, node);
    }
}

fn summarize(file: &str, name: String, is_method: bool, block: &syn::Block) -> FunctionRecord {
    let mut body = BodyVisitor {
        method_calls: BTreeSet::new(),
        path_calls: BTreeSet::new(),
        exposures: BTreeSet::new(),
        saw_command_type: false,
        deferred_process_methods: false,
    };
    body.visit_block(block);

    let mut exposures = body.exposures;
    // `.spawn()`, `.output()` and `.status()` count as process work only when
    // the same body also names a `Command` type. Without that guard these
    // method names collide with unrelated APIs and the signal becomes noise.
    if body.saw_command_type && body.deferred_process_methods {
        exposures.insert(Exposure::ProcessSpawn);
    }

    FunctionRecord {
        name,
        file: file.to_string(),
        is_method,
        method_calls: body.method_calls,
        path_calls: body.path_calls,
        exposures,
    }
}

struct BodyVisitor {
    method_calls: BTreeSet<String>,
    path_calls: BTreeSet<String>,
    exposures: BTreeSet<Exposure>,
    saw_command_type: bool,
    deferred_process_methods: bool,
}

const FS_FREE_FUNCTIONS: &[&str] = &[
    "read_to_string",
    "write",
    "copy",
    "rename",
    "remove_file",
    "remove_dir_all",
    "create_dir_all",
    "canonicalize",
    "read_dir",
    "metadata",
    "symlink_metadata",
];

const FS_METHODS: &[&str] =
    &["exists", "is_file", "is_dir", "canonicalize", "read_dir", "symlink_metadata"];

const PROCESS_METHODS: &[&str] = &["spawn", "output", "status"];

const NETWORK_MARKERS: &[&str] = &["TcpStream", "UdpSocket", "TcpListener", "reqwest", "ureq"];

impl<'ast> Visit<'ast> for BodyVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        if FS_METHODS.contains(&method.as_str()) {
            self.exposures.insert(Exposure::Filesystem);
        }
        if PROCESS_METHODS.contains(&method.as_str()) {
            self.deferred_process_methods = true;
        }
        self.method_calls.insert(method);
        self.record_function_reference_args(&node.args);
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            let segments: Vec<String> =
                path.path.segments.iter().map(|seg| seg.ident.to_string()).collect();
            self.classify_path(&segments, node);
            if let Some(last) = segments.last() {
                self.path_calls.insert(last.clone());
            }
        }
        self.record_function_reference_args(&node.args);
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        for segment in &node.segments {
            let ident = segment.ident.to_string();
            if ident == "Command" {
                self.saw_command_type = true;
            }
            if NETWORK_MARKERS.contains(&ident.as_str()) {
                self.exposures.insert(Exposure::Network);
            }
        }
        syn::visit::visit_path(self, node);
    }
}

impl BodyVisitor {
    /// Record functions passed by reference as call edges.
    ///
    /// `set_root_uri` reaches `.perltidyrc` discovery only as
    /// `and_then(discover_perltidy_profile)`. Treating argument-position paths
    /// as edges captures that without turning every type or constant mention in
    /// a body into a spurious edge.
    fn record_function_reference_args(
        &mut self,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    ) {
        for arg in args {
            if let syn::Expr::Path(path) = arg
                && let Some(last) = path.path.segments.last()
            {
                self.path_calls.insert(last.ident.to_string());
            }
        }
    }

    fn classify_path(&mut self, segments: &[String], node: &syn::ExprCall) {
        let Some(last) = segments.last() else {
            return;
        };
        let joined = segments.join("::");

        if segments.iter().any(|seg| seg == "Command") && last == "new" {
            self.exposures.insert(Exposure::ProcessSpawn);
        }
        if joined == "which" || joined.starts_with("which::") {
            self.exposures.insert(Exposure::PathLookup);
        }
        if segments.iter().any(|seg| seg == "fs") && FS_FREE_FUNCTIONS.contains(&last.as_str()) {
            self.exposures.insert(Exposure::Filesystem);
        }
        if segments.iter().any(|seg| seg == "File") && (last == "open" || last == "create") {
            self.exposures.insert(Exposure::Filesystem);
        }
        // `OpenOptions::new().append(true).open(path)` never names `File`, so
        // the builder idiom needs its own marker.
        if segments.iter().any(|seg| seg == "OpenOptions") {
            self.exposures.insert(Exposure::Filesystem);
        }
        if segments.iter().any(|seg| seg == "env") && last == "current_dir" {
            self.exposures.insert(Exposure::EnvRead);
        }
        if segments.iter().any(|seg| seg == "env") && (last == "var" || last == "var_os") {
            // A `PATH` read is executable discovery, not ordinary configuration.
            if call_mentions_path_env(node) {
                self.exposures.insert(Exposure::PathLookup);
            } else {
                self.exposures.insert(Exposure::EnvRead);
            }
        }
    }
}

fn call_mentions_path_env(node: &syn::ExprCall) -> bool {
    node.args.iter().any(|arg| {
        matches!(
            arg,
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. })
                if text.value() == "PATH"
        )
    })
}

/// Collects LSP-style protocol method names from string literals.
struct MethodLiteralCollector<'a> {
    found: &'a mut BTreeSet<String>,
}

impl<'ast> Visit<'ast> for MethodLiteralCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        let value = node.value();
        if looks_like_protocol_method(&value) {
            self.found.insert(value);
        }
    }
}

/// Whether a string literal looks like a protocol method name such as
/// `window/showMessage` or `perl-lsp/index-ready`.
pub fn looks_like_protocol_method(value: &str) -> bool {
    let mut parts = value.split('/');
    let (Some(head), Some(tail), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && segment.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    };
    segment_ok(head) && segment_ok(tail)
}

/// Collect names declared as `#[cfg(test)] mod name;` without a body.
fn collect_test_only_modules(file: &syn::File, out: &mut BTreeSet<String>) {
    for item in &file.items {
        if let syn::Item::Mod(item_mod) = item
            && item_mod.content.is_none()
            && is_cfg_test(&item_mod.attrs)
        {
            out.insert(item_mod.ident.to_string());
        }
    }
}

/// Whether a file is reached only through a `#[cfg(test)] mod name;` declaration.
fn is_test_only_file(file: &str, test_only_modules: &BTreeSet<String>) -> bool {
    let Some(stem) = file.rsplit('/').next().and_then(|name| name.strip_suffix(".rs")) else {
        return false;
    };
    if stem == "mod" {
        // `foo/mod.rs` is named by its parent directory.
        return file.rsplit('/').nth(1).is_some_and(|parent| test_only_modules.contains(parent));
    }
    test_only_modules.contains(stem)
}
