# Feature Governance Architecture

This document explains how the Perl LSP manages its LSP feature catalog through the
feature-governance modules in `perl-lsp-rs-core`. It covers why the system exists,
how catalog data flows from declaration to runtime capability, and how to add new
features.

> Architectural note: the build-time `features.toml` → generated Rust contract pipeline is now formalized in [ADR-0040](../adr/0040-generated-feature-catalog-contracts.md).

## Why Feature Governance Exists

The Perl LSP advertises 80+ features spanning LSP text document operations,
workspace services, window notifications, notebook support, debug adapter
protocol (DAP) capabilities, and protocol lifecycle methods. Each feature
carries metadata: its LSP spec version, functional area, maturity level,
whether it is advertised to clients, associated test files, and whether it is
included in the catalog's historical declaration grouping. These fields are
not behavior evidence and do not establish a compliance percentage.

Managing this catalog by hand -- scattered across server initialization code,
test assertions, documentation, and CI gates -- would inevitably lead to drift.
A feature could be implemented but never advertised, or advertised but lacking
tests, or tracked for compliance in one place but not another.

Feature governance solves this with two core principles:

1. **Single source of truth.** `features.toml` at the workspace root declares
   every feature once. Build scripts compile this into Rust constants at build
   time, so the runtime catalog is always derived from the same file.

2. **Separation of concerns within the core crate.** Catalog parsing and
   build-time rendering live in `feature_catalog.rs` and the inlined
   `build_catalog.rs`; runtime profiles, policy, capability mapping, and grid
   reporting live under `src/features/`. The `governance` module provides the
   stable façade used by the server and tooling.

## Architecture Overview

The data flow from declaration to runtime looks like this:

```
features.toml
    |
    +--> perl-lsp-rs-core/build.rs
    |       includes build_catalog.rs, resolves and validates the source,
    |       and writes OUT_DIR/feature_contracts.rs
    |
    +--> perl-lsp-rs-core/src/feature_catalog.rs
            parses, validates, and renders the shared catalog model

perl-lsp-rs-core/src/features/
    contracts  -> generated catalog plus BDD rows and compatibility accessors
    flags      -> profile feature flags
    policy     -> runtime profile and capability selection
    grid       -> profile-specific feature-grid payloads
    ids        -> stable feature identifiers
    profile    -> profile parsing and aliases
    |
    v
perl-lsp-rs-core/src/governance  -> stable façade
    |
    v
perl-lsp / perl-lsp-launcher    -> server and launcher consumers
```

### `features.toml` -- the Single Authority

`features.toml` at the workspace root is the ONE authority for feature claims:
IDs, spec versions, directions, capability gates, registration routes,
ownership, claim strength (maturity), limitations, and evidence policy. The
crate-local `crates/*/features_sot.toml` files are byte-exact GENERATED
projections of it (fallbacks for standalone/packaged builds); regenerate with
`cargo xtask feature-sot` and verify with `--check`. Hand-editing a projection
is projection drift and fails CI (#7029).

Every feature is declared as a `[[feature]]` entry with these fields:

| Field | Purpose |
|-------|---------|
| `id` | Canonical identifier (e.g. `lsp.completion`, `dap.core`) |
| `spec` | LSP/DAP spec version where the feature was introduced |
| `area` | Functional grouping: `text_document`, `workspace`, `window`, `notebook`, `debug`, `protocol` |
| `maturity` | Claim strength: `proven`, `preview`, `planned`, `unsupported`, `not_proven` |
| `advertised` | Whether the server announces this capability to clients (binary surface; never an evidence claim) |
| `direction` | Request direction: `client_to_server`, `server_to_client`, `both`, or `missing` |
| `capability_gate` | Client-capability key that gates the feature, `none`, or `missing` |
| `registration` | Registration route: `static`, `dynamic`, `none`, or `missing` |
| `feature_class` | Promotion-policy class (`request_response`, `server_request`, `document_workspace`, `cancellation_progress`, `editor_dependent`, `debug_adapter`) |
| `impl_owner` | Implementation owner module path or `missing` |
| `state_owner` | Retained-state owner or `missing`/`none` |
| `limitations` | Known limitations or `missing` |
| `claim_boundary` | Claim boundary relative to the upstream spec or `missing` |
| `counts_in_coverage` | Historical declaration grouping selector; not behavior evidence |
| `tests` | Paths to test receipts exercising the feature |
| `evidence` | Classified `[[feature.evidence]]` receipts (`class` + `path`); required for `proven` |
| `description` | Human-readable summary |

The `[meta]` section records the catalog version and target LSP spec version.
It must not carry a computed compliance percentage; aggregate declaration
counts are navigation context only (#6731).

The `[policy]` section defines the recognized evidence classes, which classes
qualify for `proven`, and the minimum evidence per `feature_class`
(`[policy.promotion.*]`). A row may claim `maturity = "proven"` only with
classified evidence receipts that qualify for its class and exist on disk;
`advertised = true`, an implementation path, or a named test can never promote
a row by itself.

## Module Responsibilities

| Module | Path | Role |
|-------|------|------|
| `feature_catalog` | `crates/perl-lsp-rs-core/src/feature_catalog.rs` | Parses and validates `features.toml`, renders the generated LSP module, and provides catalog compatibility APIs. |
| `features::contracts` | `crates/perl-lsp-rs-core/src/features/contracts.rs` | Exposes generated feature rows, profile contracts, and grid counts. |
| `capability_map` / `features::ids` | `crates/perl-lsp-rs-core/src/capability_map.rs`, `src/features/ids.rs` | Owns capability translation and stable feature identifiers. |
| `features::flags` / `features::profile` | `crates/perl-lsp-rs-core/src/features/flags.rs`, `src/features/profile.rs` | Defines profile flags and canonical profile parsing. |
| `features::policy` | `crates/perl-lsp-rs-core/src/features/policy.rs` | Resolves profiles into advertised runtime feature IDs. |
| `features::grid` | `crates/perl-lsp-rs-core/src/features/grid.rs` | Assembles BDD/grid JSON and profile summaries. Retained aggregate helpers are compatibility-only, not behavior evidence or authoritative reporting. |
| `features::profile_cli` / `governance` | `crates/perl-lsp-rs-core/src/features/profile_cli.rs`, `src/governance/mod.rs` | Parses CLI profile arguments and re-exports the stable governance surface. |

## Feature Profiles

Profiles control which capabilities the server advertises. Three canonical
profiles are defined:

| Profile | Constructor | Description |
|---------|------------|-------------|
| `ga-lock` | `BuildFlags::ga_lock()` | Conservative set. Excludes `inline_values`. Includes formatting. Intended for stable API guarantees. |
| `production` | `BuildFlags::production()` | Standard runtime set. Includes native full-document and range formatting. All other GA features enabled. |
| `all` | `BuildFlags::all()` | Every in-tree feature enabled. Used for testing, snapshots, and CI matrices. |

Profile selection happens through:

1. **Compile-time default:** The `lsp-ga-lock` Cargo feature propagates through
   the crate chain. When enabled, `FeatureProfileKind::current()` returns
   `GaLock`; otherwise it returns `Production`.

2. **CLI override:** The `--feature-profile` flag accepts tokens like `ga`,
   `ga-lock`, `ga_lock`, `prod`, `production`, `all`, or `auto` (which falls
   back to the compiled default).

3. **Runtime adaptation:** `FeatureProfile::runtime_flags()` preserves the
   selected profile's native formatting flags. External tool detection still
   exists for compatibility adapters, but it no longer gates whether formatting
   capabilities are advertised.

## Contracts and Compliance

The core crate acts as the bridge between the TOML catalog and the Rust type
system. At build time, `build.rs` includes the local `build_catalog.rs` module to:

1. Locate `features.toml` (checking `FEATURES_TOML_OVERRIDE` env var, then the
   workspace root, then the generated `features_sot.toml` projection).
2. Parse and validate the catalog (no empty IDs, no duplicates; `planned`/
   `unsupported` rows unadvertised; `proven` requires evidence receipts).
3. Render a generated Rust module with:
   - `ALL_FEATURES: &[Feature]` -- every feature row as a const array.
   - `ADVERTISED_LSP_FEATURES: &[&str]` -- IDs of rows with
     `advertised = true` (the binary surface, independent of claim strength).
   - `has_feature()` and `advertised_features()` functions.

This generated module is included via `include!(concat!(env!("OUT_DIR"), "/feature_contracts.rs"))`,
giving the core crate compile-time access to the complete feature catalog without
runtime TOML parsing. `governance` and the LSP facade re-export the supported API.

Generated status reports the evidence-backed proven share:

```text
proven share % = proven_trackable_features / trackable_features * 100
```

Where "trackable" means maturity is neither `planned` nor `unsupported` and
`counts_in_coverage == true`. Advertisement never feeds this number (#7029).
Features like protocol lifecycle methods (`lsp.initialize`, `lsp.shutdown`) and
window notifications set `counts_in_coverage = false` because they are
infrastructure, not user-facing language features.

## Feature Lifecycle

A feature progresses through claim-strength stages (#7029 vocabulary):

### `planned`

The feature is declared in `features.toml` with `maturity = "planned"` and
`advertised = false`. It carries no implementation claim and stays out of
trackable denominators. This is the starting point for tracking work items.

### `unsupported`

Explicitly not implemented and not planned (for example, impossible on the
host platform, such as DAP `restartFrame` under perl5db). Always
`advertised = false`.

### `not_proven`

An implementation and/or advertisement exists, but recorded evidence does not
yet support even preview strength. This is the honest baseline for most rows
after #7029; runtime PRs earn promotion from here.

### `preview`

The implementation is functional and exercised by cited suites, but promotion
awaits validated classified-evidence receipts. May be advertised when the
binary surface includes it.

### `proven`

Recorded evidence satisfies the row's `[policy.promotion.<feature_class>]`
minimum: the row carries `[[feature.evidence]]` receipts whose class qualifies
(`[policy].classes_qualifying_for_proven`) and whose paths exist. Advertisement
and test-file names alone never reach this state.

## How to Add a New LSP Feature

### Step 1: Declare in `features.toml`

Add a `[[feature]]` entry with the full #7029 schema (direction, capability
gate, registration, feature class, ownership, limitations, claim boundary —
use the explicit value `"missing"` where genuinely unrecorded):

```toml
[[feature]]
id = "lsp.new_feature"
spec = "LSP 3.18"
area = "text_document"
maturity = "not_proven"
advertised = false
direction = "client_to_server"
capability_gate = "textDocument.newFeature"
registration = "static"
feature_class = "request_response"
impl_owner = "perl-lsp-rs-core::providers::new_feature"
state_owner = "perl-lsp-rs::state::document"
limitations = "missing"
claim_boundary = "method-scoped"
tests = ["crates/perl-lsp-rs/tests/lsp_new_feature_tests.rs"]
description = "Description of the new feature"
```

### Step 2: Add a feature ID constant

In `crates/perl-lsp-rs-core/src/features/ids.rs`, add:

```rust
/// New feature identifier.
pub const LSP_NEW_FEATURE: &str = "lsp.new_feature";
```

### Step 3: Register in BuildFlags

In `crates/perl-lsp-rs-core/src/features/flags.rs`:

1. Add a `pub new_feature: bool` field to `BuildFlags`.
2. Add a corresponding field to `AdvertisedFeatures` if it will be client-visible.
3. Wire the flag in `to_advertised_features()` and `to_feature_ids()`.
4. Set it to `true` in the appropriate profile constructors (`all()` first,
   then `production()` and `ga_lock()` when it reaches GA).

### Step 4: Register in capability map

In `crates/perl-lsp-rs-core/src/capability_map.rs`:

1. Add a match arm in `caps_from_feature_ids()` to set the relevant
   `ServerCapabilities` field.
2. Add a check in `feature_ids_from_caps()` to extract the ID from capabilities.

### Step 5: Implement the LSP provider

Create or update the provider module (under `crates/perl-lsp-rs-core/src/providers/`) and
register it in the server's request routing.

### Step 6: Write tests

Add test files referenced in the `tests` array in `features.toml`. The BDD grid
will track whether the feature has associated test coverage.

### Step 7: Promote maturity

Promotion is evidence-earned (#7029). To promote a row to `proven`, add
classified receipts and flip the label in `features.toml`:

```toml
maturity = "proven"
advertised = true

[[feature.evidence]]
class = "wire_e2e"        # or "integration" — must qualify for feature_class
path = "crates/perl-lsp-rs/tests/lsp_new_feature_e2e.rs"
```

The validator (`cargo xtask feature-sot --check`) fails closed when a row
claims proven without qualifying, existing receipts, so advertisement alone
can never promote a row. Regenerate the crate projections after any catalog
edit: `cargo xtask feature-sot`.

## Dependency Graph

The governance surface is now a module graph inside `perl-lsp-rs-core`: catalog
parsing and generated contracts feed identifiers, capability mapping, profiles,
policy, and grid reporting; `governance` is the public re-export facade. The
LSP crate consumes that facade. The former feature-governance crates are retired
and must not be used as current architecture or dependency examples.

## Related Documentation

- [features.toml](../../features.toml) -- the canonical feature catalog
- [CURRENT_STATUS.md](CURRENT_STATUS.md) -- current status and evidence boundaries
- [STABILITY.md](../reference/STABILITY.md) -- API stability policy
- [LSP_IMPLEMENTATION_GUIDE.md](../reference/LSP_IMPLEMENTATION_GUIDE.md) -- server architecture
