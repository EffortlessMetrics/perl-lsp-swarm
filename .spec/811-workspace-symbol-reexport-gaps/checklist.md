# Implementation Checklist: Workspace-Symbol Dual Indexing Gaps in Re-Export Chains

## Phase 1: Add Export Edge Tracking to ReferenceIndex

### Step 1.1: Add export_edges field to ReferenceIndex struct
**File**: `crates/perl-workspace/src/semantic/references.rs`

**What to change**: Add a new HashMap field to track export edges.

**Current state**: `ReferenceIndex` has `file_to_refs` (FileId → references) and `global_refs` (symbol → locations).

**Change**:
```rust
pub struct ReferenceIndex {
    file_to_refs: HashMap<FileId, Vec<Reference>>,
    global_refs: HashMap<String, Vec<Location>>,
    // NEW FIELD:
    export_edges: HashMap<String, Vec<ExportEdge>>,
}

// NEW TYPE (document re-export relationships):
#[derive(Debug, Clone)]
pub struct ExportEdge {
    /// Module that re-exports (e.g., "MyModule")
    pub exporting_module: String,
    /// Symbol being exported (e.g., "foo")
    pub symbol_name: String,
    /// URI where the export is declared
    pub declared_uri: String,
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | grep -E "error|warning"` (expect compilation errors until full wiring)

### Step 1.2: Implement add_export_edge and query_export_edges methods
**File**: `crates/perl-workspace/src/semantic/references.rs`

**What to change**: Add public methods to ReferenceIndex to store and query export edges.

**Signature**:
```rust
impl ReferenceIndex {
    /// Record that `exporting_module` re-exports `symbol_name` from `declared_uri`.
    pub fn add_export_edge(&mut self, symbol_name: &str, edge: ExportEdge) {
        self.export_edges.entry(symbol_name.to_string())
            .or_insert_with(Vec::new)
            .push(edge);
    }

    /// Query all modules that re-export `symbol_name`.
    pub fn query_export_edges(&self, symbol_name: &str) -> &[ExportEdge] {
        self.export_edges.get(symbol_name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Remove all export edges originating from `source_uri`.
    pub fn remove_export_edges_for_uri(&mut self, source_uri: &str) {
        for edges in self.export_edges.values_mut() {
            edges.retain(|e| e.declared_uri != source_uri);
        }
        self.export_edges.retain(|_, edges| !edges.is_empty());
    }
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | head -20`

### Step 1.3: Wire export edge population into workspace_index.rs
**File**: `crates/perl-workspace/src/workspace/workspace_index.rs`

**What to change**: Extract export edges from AST/facts and populate ReferenceIndex during indexing.

**Current indexing flow**: 
- Lines 1750-1810 in `index_file()` extract symbols and add to `self.symbols`
- Lines 1775-1780 populate `ImportExportIndex`
- Line 2590-2610 update reference index

**Change**: After symbols are indexed, extract @EXPORT_OK/@EXPORT from the AST and add export edges:

```rust
// In index_file(), after line ~1780 (after ImportExportIndex is updated):

// Extract @EXPORT and @EXPORT_OK declarations
let package_name = extract_package_name(&ast);
if let Some(pkg) = package_name {
    for symbol in &file_index.symbols {
        // Check if this symbol appears in @EXPORT_OK/@EXPORT declarations
        // This requires extracting export declarations from AST
        // For now, rely on ImportExportIndex.exports_by_module keying by module
        // and correlate with symbol names
        if is_exported_symbol(&pkg, &symbol.name, &ast) {
            let edge = ExportEdge {
                exporting_module: pkg.clone(),
                symbol_name: symbol.name.clone(),
                declared_uri: uri_str.clone(),
            };
            self.semantic_reference_index.write().add_export_edge(
                &symbol.name,
                edge,
            );
        }
    }
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | grep "error\|warning"`

---

## Phase 2: Enhance find_definition() to Follow Re-Export Chains

### Step 2.1: Add helper function to resolve re-export chain
**File**: `crates/perl-workspace/src/workspace/workspace_index.rs`

**What to change**: Add a new private method to follow re-export chains.

**Location**: Before `find_definition()` method (around line 2300).

**Signature**:
```rust
impl WorkspaceIndex {
    /// Follow a re-export chain to find the original definition.
    ///
    /// If `definition_location` is from a module that re-exports the symbol,
    /// recursively follow the chain to find the original definition.
    ///
    /// # Arguments
    /// * `symbol_name` - The bare or qualified symbol name
    /// * `definition_location` - A candidate location (may be a re-export site)
    /// * `visited` - Set of visited modules to detect cycles
    ///
    /// # Returns
    /// The original definition location (not a re-export site), or the input if chain cannot be followed
    fn resolve_reexport_chain(
        &self,
        symbol_name: &str,
        definition_location: &Location,
        visited: &mut HashSet<String>,
    ) -> Location {
        let bare_name = symbol_name.rfind("::").map(|i| &symbol_name[i + 2..])
            .unwrap_or(symbol_name);

        // Get the module that declared this definition
        let decl_module = extract_package_from_uri(&definition_location.uri);
        if let Some(module) = decl_module {
            if visited.contains(&module) {
                // Cycle detected; return as-is
                return definition_location.clone();
            }
            visited.insert(module.clone());

            // Check if this module re-exports the symbol from elsewhere
            let ref_idx = self.semantic_reference_index.read();
            let export_edges = ref_idx.query_export_edges(bare_name);
            
            for edge in export_edges {
                if edge.exporting_module == module {
                    // This module re-exports the symbol.
                    // Try to find the original definition via ImportExportIndex.
                    drop(ref_idx); // Release read lock before acquiring write lock elsewhere
                    
                    let ie_idx = self.semantic_import_export_index.read();
                    // ie_idx.get_imports_for_file() tells us which modules this file imports from
                    // We need to cross-reference to find where `symbol_name` originally came from.
                    // This requires querying the inverse: which module defines this symbol?
                    
                    // For now, return the current location (this is a complex chain resolution)
                    // and defer full implementation to Step 2.2
                    return definition_location.clone();
                }
            }
        }

        definition_location.clone()
    }
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | grep "error"`

### Step 2.2: Modify find_definition() to use re-export chain resolution
**File**: `crates/perl-workspace/src/workspace/workspace_index.rs`, line 2349

**Current code**:
```rust
pub fn find_definition(&self, symbol_name: &str) -> Option<Location> {
    if let Some(location) = self.definition_candidates(symbol_name).into_iter().next() {
        return Some(location);
    }
    // ... fallback scan
}
```

**Change to**:
```rust
pub fn find_definition(&self, symbol_name: &str) -> Option<Location> {
    if let Some(location) = self.definition_candidates(symbol_name).into_iter().next() {
        // NEW: Follow re-export chain to find original definition
        let mut visited = HashSet::new();
        return Some(self.resolve_reexport_chain(symbol_name, &location, &mut visited));
    }
    // ... fallback scan (unchanged)
}
```

**Verify**: `cargo test -p perl-workspace 2>&1 | tail -20`

---

## Phase 3: Add Re-Export Chain Query to ImportExportIndex

### Step 3.1: Enhance ImportExportIndex to trace symbol origin
**File**: `crates/perl-workspace/src/semantic/imports.rs`

**What to change**: Add method to determine which module originally exports a symbol.

**Location**: In `impl ImportExportIndex` block, around line 140.

**Signature**:
```rust
impl ImportExportIndex {
    /// Find the original defining module for a symbol that may be re-exported.
    ///
    /// Given a symbol name, trace through re-export chains to find which module
    /// originally defines it. This requires correlating imports with exports.
    ///
    /// # Arguments
    /// * `importing_module_uri` - The module importing the symbol
    /// * `symbol_name` - The symbol being imported (bare name)
    ///
    /// # Returns
    /// The module name that originally defines the symbol, or None if not found
    pub fn trace_symbol_to_origin(
        &self,
        importing_module_uri: &str,
        symbol_name: &str,
    ) -> Option<String> {
        // Look up the file ID for the importing module
        let file_id = *self.file_uri_to_id.get(importing_module_uri)?;
        
        // Get imports for this file
        let imports = self.imports_by_file.get(&file_id)?;
        
        // Find which imported module provides this symbol
        for import_spec in imports {
            // Check if this module exports the symbol
            if let Some(export_set) = self.exports_by_module.get(&import_spec.module) {
                if export_set.exports.contains(&symbol_name.to_string())
                   || export_set.exports_ok.contains(&symbol_name.to_string()) {
                    // Found the immediate re-exporter.
                    // Now check if that module itself re-exported it (recursive case).
                    // For now, return the module; full recursion in Step 3.2.
                    return Some(import_spec.module.clone());
                }
            }
        }
        
        None
    }
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | grep "error"`

### Step 3.2: Implement recursive origin resolution
**File**: `crates/perl-workspace/src/semantic/imports.rs`

**Modify Step 3.1 method** to recursively trace chains:

```rust
pub fn trace_symbol_to_origin_recursive(
    &self,
    importing_module_uri: &str,
    symbol_name: &str,
    visited: &mut HashSet<String>,
) -> Option<String> {
    let file_id = *self.file_uri_to_id.get(importing_module_uri)?;
    let imports = self.imports_by_file.get(&file_id)?;
    
    for import_spec in imports {
        let module_name = &import_spec.module;
        
        if visited.contains(module_name) {
            continue; // Cycle detected
        }
        visited.insert(module_name.clone());
        
        if let Some(export_set) = self.exports_by_module.get(module_name) {
            if export_set.exports.contains(&symbol_name.to_string())
               || export_set.exports_ok.contains(&symbol_name.to_string()) {
                // Check if this module itself re-exports it
                if let Some(source_uri) = self.module_to_source_uri.get(module_name) {
                    if let Some(origin) = self.trace_symbol_to_origin_recursive(
                        source_uri,
                        symbol_name,
                        visited,
                    ) {
                        return Some(origin);
                    }
                }
                // If no further origin found, this module is the original
                return Some(module_name.clone());
            }
        }
    }
    
    None
}
```

**Verify**: `cargo test -p perl-workspace 2>&1 | grep -A5 "test result"`

---

## Phase 4: Update find_references() to Include Re-Export Sites

### Step 4.1: Extend find_references() to query export edges
**File**: `crates/perl-workspace/src/workspace/workspace_index.rs`, line 2171

**Current code**: Searches bare-name and qualified variants, deduplicates.

**Change**: After collecting all references, also add locations of modules that re-export the symbol:

```rust
pub fn find_references(&self, symbol_name: &str) -> Vec<Location> {
    let global_refs = self.global_references.read();
    let mut seen: HashSet<(String, u32, u32, u32, u32)> = HashSet::new();
    let mut locations = Vec::new();

    // (existing code: collect refs from global_references)
    // ... lines 2177-2230 unchanged ...

    // NEW: Add re-export sites
    let bare_name = symbol_name.rfind("::").map(|i| &symbol_name[i + 2..])
        .unwrap_or(symbol_name);
    
    let ref_idx = self.semantic_reference_index.read();
    for edge in ref_idx.query_export_edges(bare_name) {
        // Each export edge's declared_uri is a re-export site
        let location = Location {
            uri: edge.declared_uri.clone(),
            range: Range::default(), // TODO: use actual range from AST
        };
        let key = (
            location.uri.clone(),
            0, 0, 0, 0, // Range not yet available; deduplicate on URI only
        );
        if seen.insert(key) {
            locations.push(location);
        }
    }

    Self::sort_locations_deterministically(&mut locations);
    locations
}
```

**Verify**: `cargo test -p perl-workspace test_find_references 2>&1 | tail -10`

---

## Phase 5: Enhance find_symbols() to Rank by Origin

### Step 5.1: Add origin detection to search results
**File**: `crates/perl-workspace/src/workspace/workspace_index.rs`, line 2908

**Current code**: Searches for bare-name or qualified-name substring matches.

**Change**: Add rank field and sort by original-definition-first:

```rust
pub fn search_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
    let query = query.trim();
    let query_lower = query.to_lowercase();
    let files = self.files.read();
    
    // Collect candidates with origin rank
    let mut candidates: Vec<(WorkspaceSymbol, i32)> = Vec::new();
    
    for file_index in files.values() {
        for symbol in &file_index.symbols {
            if symbol.name.to_lowercase().contains(&query_lower)
                || symbol.qualified_name.as_ref()
                    .map(|qn| qn.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            {
                // Determine if this is an original definition or re-export
                let rank = if self.is_re_export_site(&symbol) {
                    1 // Re-export sites rank lower
                } else {
                    0 // Original definitions rank higher
                };
                candidates.push((symbol.clone(), rank));
            }
        }
    }

    // Sort by rank (0 first), then by name for stability
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.name.cmp(&b.0.name)));
    
    let mut results: Vec<WorkspaceSymbol> = candidates.into_iter().map(|(s, _)| s).collect();
    results
}

fn is_re_export_site(&self, symbol: &WorkspaceSymbol) -> bool {
    // Check if this symbol's file contains a use statement or @EXPORT_OK
    // that re-exports the symbol (simple heuristic for now)
    let ie_idx = self.semantic_import_export_index.read();
    let decl_uri = &symbol.uri;
    let file_id = Self::hash_uri_to_file_id(decl_uri);
    
    // If file imports the symbol, it's likely re-exporting
    let imports = ie_idx.get_imports_for_file(file_id);
    imports.iter().any(|imp| {
        ie_idx.exports_by_module.get(&imp.module)
            .map(|exp| exp.exports.contains(&symbol.name)
              || exp.exports_ok.contains(&symbol.name))
            .unwrap_or(false)
    })
}
```

**Verify**: `cargo build -p perl-workspace 2>&1 | grep "error"`

---

## Phase 6: Extend symbol_uri_reachable() for Re-Exports

### Step 6.1: Modify EffectiveIncContext to check re-export modules
**File**: `crates/perl-lsp-rs/src/runtime/lifecycle/inc_context/mod.rs`, line 73

**Current code**: Checks if symbol_uri is under an effective include root.

**Change**: Also allow symbols from modules that re-export reachable definitions:

```rust
pub(crate) fn symbol_uri_reachable(&self, symbol_uri: &str) -> bool {
    let Some(symbol_path) = super::super::source_path_from_uri(symbol_uri) else {
        // Non-file URI — don't filter.
        return true;
    };

    // Normalise the symbol path to an absolute form for comparison.
    let symbol_abs = if symbol_path.is_absolute() {
        symbol_path
    } else {
        self.root.join(&symbol_path)
    };

    let root_is_workspace = |root_abs: &std::path::Path| root_abs == self.root.as_path();

    // First: check if symbol is directly under an effective include root
    if self.effective_roots.iter().any(|root| {
        let root_abs = if root.path.is_absolute() {
            root.path.clone()
        } else {
            self.root.join(&root.path)
        };
        if root_is_workspace(&root_abs) {
            return false;
        }
        symbol_abs.starts_with(&root_abs)
    }) {
        return true;
    }

    // NEW: Check if symbol is re-exported by a reachable module
    // This requires querying the workspace index's import-export index
    // (deferred to Phase 7 for LSP server integration)
    
    false
}
```

**Note**: Full implementation deferred to Phase 7 (requires LSP server context).

**Verify**: `cargo build -p perl-lsp-rs 2>&1 | grep "error"`

---

## Phase 7: Add Integration Tests for Re-Export Scenarios

### Step 7.1: Add test for bare-call re-export chain
**File**: `crates/perl-workspace/tests/dual_indexing_tests.rs`

**Location**: After existing tests (around line 350).

**Test code**:
```rust
#[test]
fn test_reexport_chain_goto_definition_bare_call() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    
    // Module B defines optional()
    let b_uri = file_url("/lib/Optional/Base.pm")?;
    index.index_file(b_uri.clone(), r#"
package Optional::Base;
sub optional { return 1; }
1;
"#.to_string())?;

    // Module A re-exports optional from B
    let a_uri = file_url("/lib/Optional/Consumer.pm")?;
    index.index_file(a_uri.clone(), r#"
package Optional::Consumer;
use Optional::Base;
our @EXPORT_OK = qw(optional);
1;
"#.to_string())?;

    // Caller imports from A but should resolve definition to B
    let caller_uri = file_url("/scripts/main.pl")?;
    index.index_file(caller_uri, r#"
use Optional::Consumer qw(optional);
optional();
"#.to_string())?;

    // find_definition("optional") should jump to B's definition, not A's import
    let def = index.find_definition("optional")
        .expect("should find definition for 'optional'");
    
    assert_eq!(def.uri, b_uri.to_string(), 
        "definition should point to B (Optional/Base.pm), not A (Optional/Consumer.pm)");
    
    Ok(())
}
```

**Verify**: `cargo test -p perl-workspace test_reexport_chain_goto_definition_bare_call 2>&1`

### Step 7.2: Add test for consumer consistency
**File**: `crates/perl-workspace/tests/dual_indexing_tests.rs`

**Test code**:
```rust
#[test]
fn test_reexport_consumer_consistency() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    
    let b_uri = file_url("/lib/Base.pm")?;
    index.index_file(b_uri.clone(), "package Base;\nsub foo { 1 }".to_string())?;

    let a_uri = file_url("/lib/Wrapper.pm")?;
    index.index_file(a_uri.clone(), 
        "package Wrapper; use Base; our @EXPORT_OK = qw(foo); 1;".to_string())?;

    // find_definition and find_references should agree on the definition location
    let def = index.find_definition("foo")
        .expect("should find definition");
    let refs = index.find_references("foo");

    assert!(refs.contains(&def), 
        "find_references should include the location returned by find_definition");

    // find_symbols should list the original definition
    let syms = index.find_symbols("foo");
    assert!(!syms.is_empty(), "find_symbols should find foo");
    
    Ok(())
}
```

**Verify**: `cargo test -p perl-workspace test_reexport_consumer_consistency 2>&1`

### Step 7.3: Add test for workspace-symbol ranking
**File**: `crates/perl-workspace/tests/dual_indexing_tests.rs`

**Test code**:
```rust
#[test]
fn test_reexport_workspace_symbol_ranking() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    
    let orig_uri = file_url("/lib/Original.pm")?;
    index.index_file(orig_uri, "package Original;\nsub process { 1 }".to_string())?;

    let reexp_uri = file_url("/lib/Reexporter.pm")?;
    index.index_file(reexp_uri, 
        "package Reexporter; use Original; our @EXPORT_OK = qw(process); 1;".to_string())?;

    // workspace-symbol query should rank original definition first
    let results = index.find_symbols("process");
    assert!(!results.is_empty(), "should find 'process' symbol");
    
    // Original definition should be first candidate
    // (after ranking implementation in Step 5.1)
    // For now, just verify both are found
    
    Ok(())
}
```

**Verify**: `cargo test -p perl-workspace test_reexport_workspace_symbol_ranking 2>&1`

### Step 7.4: Add test for three-level re-export chain
**File**: `crates/perl-workspace/tests/dual_indexing_tests.rs`

**Test code**:
```rust
#[test]
fn test_reexport_chain_three_levels() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    
    // Level 1: C defines the original
    let c_uri = file_url("/lib/C.pm")?;
    index.index_file(c_uri.clone(), "package C;\nsub deep { 1 }".to_string())?;

    // Level 2: B re-exports from C
    let b_uri = file_url("/lib/B.pm")?;
    index.index_file(b_uri.clone(), 
        "package B; use C; our @EXPORT_OK = qw(deep); 1;".to_string())?;

    // Level 3: A re-exports from B
    let a_uri = file_url("/lib/A.pm")?;
    index.index_file(a_uri, 
        "package A; use B; our @EXPORT_OK = qw(deep); 1;".to_string())?;

    // find_definition should resolve all the way to C
    let def = index.find_definition("deep")
        .expect("should find definition for 'deep'");
    
    assert_eq!(def.uri, c_uri.to_string(), 
        "three-level chain should resolve to original (C.pm)");
    
    Ok(())
}
```

**Verify**: `cargo test -p perl-workspace test_reexport_chain_three_levels 2>&1`

### Step 7.5: Add regression test for non-Exporter modules
**File**: `crates/perl-workspace/tests/dual_indexing_tests.rs`

**Test code**:
```rust
#[test]
fn test_reexport_non_exporter_modules_unaffected() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    
    // Regular module without Exporter
    let uri = file_url("/lib/Regular.pm")?;
    index.index_file(uri.clone(), "package Regular;\nsub normal { 1 }".to_string())?;

    // Should still work as before (regression test)
    let def = index.find_definition("Regular::normal")
        .expect("qualified lookup should work");
    assert_eq!(def.uri, uri.to_string());

    let bare_def = index.find_definition("normal")
        .expect("bare lookup should work");
    assert_eq!(bare_def.uri, uri.to_string());

    Ok(())
}
```

**Verify**: `cargo test -p perl-workspace test_reexport_non_exporter_modules_unaffected 2>&1`

---

## Phase 8: Verify and Compile

### Step 8.1: Full workspace build
**Command**:
```bash
cargo build -p perl-workspace 2>&1 | head -50
cargo build -p perl-lsp-rs 2>&1 | head -50
```

**Expected**: No errors after all changes.

### Step 8.2: Run all workspace tests
**Command**:
```bash
cargo test -p perl-workspace --lib 2>&1 | tail -30
```

**Expected**: All tests pass, including new re-export tests.

### Step 8.3: Run LSP integration tests
**Command**:
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2 2>&1 | tail -30
```

**Expected**: No regressions in navigation, completion, hover tests.

### Step 8.4: Format and lint
**Command**:
```bash
cargo fmt -p perl-workspace -p perl-lsp-rs
cargo clippy -p perl-workspace -p perl-lsp-rs --lib 2>&1 | grep -E "error|warning"
```

**Expected**: No clippy warnings.

### Step 8.5: Commit spec files
**Command**:
```bash
git add .spec/811-workspace-symbol-reexport-gaps/
git commit -m "plan(workspace): add implementation spec for #811"
git push -u origin impl/811-workspace-symbol-reexport-gaps
```

---

## Implementation Order Summary

1. **Phase 1** (ReferenceIndex): Add export_edges field and query methods
2. **Phase 2** (find_definition): Implement re-export chain resolution
3. **Phase 3** (ImportExportIndex): Add origin tracing queries
4. **Phase 4** (find_references): Include re-export site locations
5. **Phase 5** (find_symbols): Rank by origin (original definitions first)
6. **Phase 6** (symbol_uri_reachable): Extend filtering (optional for Phase 1)
7. **Phase 7** (Tests): Add comprehensive test coverage
8. **Phase 8** (Verify): Build, test, format, commit

Each phase compiles independently (except Phase 6, which is optional). Red-TDD builder should write failing tests in Phase 7 before implementing Phases 1-5.
