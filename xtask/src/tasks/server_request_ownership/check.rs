//! Composition rules for the server-request ownership matrix (#13223).
//!
//! Every rule fails closed. A cell whose owner or proof is absent is recorded
//! as `missing` and can never satisfy a requirement, and an empty discovery or
//! an empty matrix is an instrument failure rather than a pass.

use super::model::{Discovered, Matrix, RegistryKind, RequestRow, Violation};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Registry module whose non-test source must name a method before that method
/// may claim a per-method response decoder.
const DECODER_SOURCE: &str = "crates/perl-lsp-rs/src/runtime/client_requests/registry.rs";

/// The two dynamic-registration methods must never collapse into one row.
const REGISTRATION_PAIR: [&str; 2] = ["client/registerCapability", "client/unregisterCapability"];

/// Dispositions that assert the method is carrying real product weight.
const CREDIT_BEARING: [&str; 2] = ["supported", "advertised_not_proven"];

fn missing(cell: &str) -> bool {
    RequestRow::is_missing(cell)
}

/// Apply every composition rule and return the findings in a deterministic order.
pub(super) fn check(
    repo_root: &Path,
    matrix: &Matrix,
    discovered: &Discovered,
    mut violations: Vec<Violation>,
) -> Vec<Violation> {
    let meta = &matrix.meta;

    if meta.schema != "server_request_ownership.v1" {
        violations.push(Violation::new(
            "matrix-schema",
            "<matrix>",
            format!("unexpected schema `{}`", meta.schema),
        ));
    }

    // ── Instrument guards ────────────────────────────────────────────────
    // Zero discovered methods or zero rows is NOT_PROVEN, never success.
    let registry_requests: BTreeSet<&String> = discovered
        .registry
        .iter()
        .filter(|(_, kind)| **kind == RegistryKind::ServerToClientRequest)
        .map(|(method, _)| method)
        .collect();

    if registry_requests.is_empty() {
        violations.push(Violation::new(
            "registry-empty",
            "<matrix>",
            format!(
                "no server-to-client requests were parsed from `{}`; the discovery instrument \
                 failed and its silence must not read as an empty surface",
                meta.direction_registry
            ),
        ));
    }
    if matrix.request.is_empty() {
        violations.push(Violation::new(
            "matrix-empty",
            "<matrix>",
            "the matrix declares no rows; an empty matrix is NOT_PROVEN, not a pass",
        ));
    }
    if discovered.catalog_rows.is_empty() {
        violations.push(Violation::new(
            "catalog-empty",
            "<matrix>",
            format!(
                "no server-to-client rows were parsed from `{}`; the catalog instrument failed",
                meta.feature_catalog
            ),
        ));
    }

    // ── Row identity ─────────────────────────────────────────────────────
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let mut by_method: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &matrix.request {
        if !seen_ids.insert(row.id.as_str()) {
            violations.push(Violation::new("duplicate-id", &row.id, "row id is declared twice"));
        }
        *by_method.entry(row.method.as_str()).or_default() += 1;
    }
    for (method, count) in &by_method {
        if *count > 1 {
            violations.push(Violation::new(
                "duplicate-method",
                *method,
                format!("{count} rows claim this method; one method has one owning row"),
            ));
        }
    }

    // ── Coverage in both directions ──────────────────────────────────────
    for method in &registry_requests {
        if !by_method.contains_key(method.as_str()) {
            violations.push(Violation::new(
                "missing-row",
                method.as_str(),
                "the direction registry classifies this as a server-to-client request but the \
                 matrix has no row for it",
            ));
        }
    }
    for pair_member in REGISTRATION_PAIR {
        if registry_requests.contains(&pair_member.to_string())
            && !by_method.contains_key(pair_member)
        {
            violations.push(Violation::new(
                "registration-pair-split",
                pair_member,
                "client/registerCapability and client/unregisterCapability each need their own \
                 row; they may not share one outcome",
            ));
        }
    }

    for row in &matrix.request {
        check_row(repo_root, matrix, discovered, row, &mut violations);
    }

    violations.sort();
    violations.dedup();
    violations
}

fn check_row(
    repo_root: &Path,
    matrix: &Matrix,
    discovered: &Discovered,
    row: &RequestRow,
    violations: &mut Vec<Violation>,
) {
    let meta = &matrix.meta;

    // ── Closed vocabulary ────────────────────────────────────────────────
    // An unlisted value is a hard failure so an invented state cannot pass.
    for (field, value, allowed) in [
        ("protocol_baseline", &row.protocol_baseline, &meta.allowed_protocol_baselines),
        ("emission", &row.emission, &meta.allowed_emission_states),
        ("response_decoder", &row.response_decoder, &meta.allowed_response_decoders),
        ("disposition", &row.disposition, &meta.allowed_dispositions),
    ] {
        if !allowed.contains(value) {
            violations.push(Violation::new(
                "value-not-allowed",
                &row.id,
                format!("`{field}` value `{value}` is outside the declared vocabulary"),
            ));
        }
    }

    if row.limitations.trim().is_empty() {
        violations.push(Violation::new(
            "empty-limitations",
            &row.id,
            "a row must state its limitations; an empty cell is not a clean bill of health",
        ));
    }

    // ── Direction and envelope ───────────────────────────────────────────
    match discovered.registry.get(&row.method) {
        None => violations.push(Violation::new(
            "unknown-method",
            &row.id,
            format!("`{}` is not classified by the direction registry", row.method),
        )),
        Some(RegistryKind::ClientToServer) => violations.push(Violation::new(
            "wrong-direction",
            &row.id,
            format!(
                "`{}` is registered client-to-server; an opposite-direction method can never \
                 satisfy server-request coverage",
                row.method
            ),
        )),
        Some(RegistryKind::ServerToClientNotification) => violations.push(Violation::new(
            "wrong-envelope",
            &row.id,
            format!(
                "`{}` is registered as a notification; a notification expects no client response \
                 and is not a server request",
                row.method
            ),
        )),
        Some(RegistryKind::ServerToClientRequest) => {}
    }

    // ── Emission ─────────────────────────────────────────────────────────
    let discovered_paths = discovered.emitted.get(&row.method);
    match row.emission.as_str() {
        "emitted" => {
            if discovered_paths.is_none() {
                violations.push(Violation::new(
                    "emission-mismatch",
                    &row.id,
                    format!(
                        "the row claims `{}` is emitted but no production call site emits it",
                        row.method
                    ),
                ));
            }
            if row.emitters.is_empty() {
                violations.push(Violation::new(
                    "emitter-missing",
                    &row.id,
                    "an emitted request must cite at least one emitter",
                ));
            }
        }
        "not_emitted" => {
            if let Some(paths) = discovered_paths {
                violations.push(Violation::new(
                    "emission-mismatch",
                    &row.id,
                    format!(
                        "the row claims `{}` is not emitted but it is emitted from {}",
                        row.method,
                        paths.join(", ")
                    ),
                ));
            }
            if !row.emitters.is_empty() {
                violations.push(Violation::new(
                    "emitter-contradiction",
                    &row.id,
                    "a row marked not_emitted must cite no emitters",
                ));
            }
        }
        _ => {}
    }

    // ── Emitter citations must be current ────────────────────────────────
    for emitter in &row.emitters {
        let Some((path, symbol)) = RequestRow::split_emitter(emitter) else {
            violations.push(Violation::new(
                "emitter-shape",
                &row.id,
                format!("emitter `{emitter}` is not `path#symbol`"),
            ));
            continue;
        };
        let absolute = repo_root.join(path);
        let Ok(source) = std::fs::read_to_string(&absolute) else {
            violations.push(Violation::new(
                "emitter-path-stale",
                &row.id,
                format!("emitter path `{path}` does not exist"),
            ));
            continue;
        };
        if !source.contains(symbol) {
            violations.push(Violation::new(
                "emitter-symbol-stale",
                &row.id,
                format!("`{path}` no longer defines `{symbol}`"),
            ));
        }
    }

    // The cited emitter set must equal the discovered one. Citing a real symbol
    // that does not emit this method, or leaving a second emitting path
    // uncited, both leave the row's ownership claim untrue.
    let discovered_refs: Vec<&String> =
        discovered_paths.map(Vec::as_slice).unwrap_or(&[]).iter().collect();
    for cited in &row.emitters {
        if !discovered_refs.iter().any(|found| *found == cited) {
            violations.push(Violation::new(
                "emitter-not-discovered",
                &row.id,
                format!(
                    "`{cited}` is cited as an emitter of `{}` but the emission scan attributes no \
                     such call site to that symbol",
                    row.method
                ),
            ));
        }
    }
    for found in &discovered_refs {
        if !row.emitters.iter().any(|cited| &cited == found) {
            violations.push(Violation::new(
                "emitter-uncited",
                &row.id,
                format!("`{found}` emits `{}` but the row does not cite it", row.method),
            ));
        }
    }

    // ── Feature catalog join ─────────────────────────────────────────────
    // The catalog's own spec is consumed, not merely its key: a row may not
    // restate a version the catalog contradicts.
    let catalog_spec = discovered.catalog_rows.get(&row.feature_catalog_row);
    if row.feature_catalog_row != "none" {
        match catalog_spec {
            None => violations.push(Violation::new(
                "catalog-row-unknown",
                &row.id,
                format!(
                    "`{}` is not a server-to-client row in `{}`",
                    row.feature_catalog_row, meta.feature_catalog
                ),
            )),
            Some(spec) if !spec.is_empty() && spec != &row.spec => {
                violations.push(Violation::new(
                    "catalog-spec-mismatch",
                    &row.id,
                    format!(
                        "the row claims spec `{}` but `{}` records `{spec}` for `{}`",
                        row.spec, meta.feature_catalog, row.feature_catalog_row
                    ),
                ));
            }
            Some(_) => {}
        }
    }

    // ── Protocol baseline ────────────────────────────────────────────────
    // A selected 3.18 surface may never inherit stable-3.17 status. The
    // catalog's spec is authoritative where it exists, so editing only the
    // matrix side cannot demote a 3.18 surface.
    let authoritative_spec =
        catalog_spec.filter(|spec| !spec.is_empty()).unwrap_or(&row.spec).clone();
    let spec_is_318 =
        authoritative_spec.contains("3.18") || authoritative_spec.contains("@proposed");
    if spec_is_318 && row.protocol_baseline != "selected_3_18" {
        violations.push(Violation::new(
            "baseline-understated",
            &row.id,
            format!(
                "spec `{authoritative_spec}` is a selected 3.18 surface but the row claims \
                 baseline `{}`",
                row.protocol_baseline
            ),
        ));
    }
    if !spec_is_318 && row.protocol_baseline == "selected_3_18" {
        violations.push(Violation::new(
            "baseline-overstated",
            &row.id,
            format!("baseline `selected_3_18` does not match spec `{}`", row.spec),
        ));
    }

    // ── Decoder claims ───────────────────────────────────────────────────
    // A per-method decoder must actually name its method in the decoding
    // module's production source; otherwise the claim is refuted.
    if row.response_decoder == "per_method" {
        let named = std::fs::read_to_string(repo_root.join(DECODER_SOURCE))
            .is_ok_and(|source| source.contains(&format!("\"{}\"", row.method)));
        if !named {
            violations.push(Violation::new(
                "decoder-overclaim",
                &row.id,
                format!(
                    "the row claims a per-method decoder but `{DECODER_SOURCE}` does not name \
                     `{}`; decoding is generic shape-only",
                    row.method
                ),
            ));
        }
    }

    // ── Support credit ───────────────────────────────────────────────────
    if row.disposition == "supported" {
        for (field, value) in [
            ("terminal_state_owner", &row.terminal_state_owner),
            ("exact_process_proof", &row.exact_process_proof),
        ] {
            if missing(value) {
                violations.push(Violation::new(
                    "support-without-proof",
                    &row.id,
                    format!(
                        "disposition `supported` requires `{field}`, which is `{value}`; missing \
                         evidence is never support credit"
                    ),
                ));
            }
        }
    }

    // A method nothing emits cannot carry credit for being carried.
    if row.emission == "not_emitted" && CREDIT_BEARING.contains(&row.disposition.as_str()) {
        violations.push(Violation::new(
            "dormant-credit",
            &row.id,
            format!(
                "`{}` has no production emitter, so disposition `{}` overstates it; a dormant or \
                 helper-only method earns no credit",
                row.method, row.disposition
            ),
        ));
    }
}
