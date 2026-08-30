//! Proof that the receipt describes the probe the plan sanctions, and that a
//! conforming observation has nowhere to put a secret.
//!
//! The second claim is deliberately structural. A boolean field asserting "no
//! credentials were retained" would be a self-declaration with no independent
//! evidence path. Instead the observation contract is closed at every level and
//! its property vocabulary is fixed, so the absence of any body, header,
//! credential or local-path field is a checkable property of the schema itself.

use super::model::RECEIPT_SCHEMA_VERSION;
use super::plan::{Cell, probe_plan};
use super::test_support::{
    AVAILABLE_EXACT, DIGEST_MISMATCH, INCIDENT, INSTRUMENT_INCOMPLETE, LISTING_MISSING,
    NAMESPACE_ABSENT, RATE_LIMITED, observation, receipt,
};
use color_eyre::eyre::{Result, bail, eyre};
use serde_json::Value;

const OBSERVATION_SCHEMA: &str =
    include_str!("../../../schemas/open_vsx_public_state_observation.v1.schema.json");

/// Every property name the observation contract is allowed to define.
///
/// Adding a name here is the moment to ask whether the new field can carry
/// response content, request headers, credentials, or a maintainer-local path.
const PERMITTED_OBSERVATION_PROPERTIES: &[&str] = &[
    "schema_version",
    "observed_at",
    "registry",
    "identity",
    "namespace",
    "extension",
    "instrument",
    "name",
    "version",
    "source_ref",
    "expected",
    "versions",
    "publication_refs",
    "authority",
    "source",
    "vsix_sha256",
    "cells",
    "listing",
    "search",
    "namespace_metadata",
    "extension_metadata",
    "version_rows",
    "versioned_file",
    "transport",
    "url",
    "method",
    "outcome",
    "status",
    "redirects",
    "response_bytes",
    "truncated",
    "error_kind",
    "matched_identity",
    "namespace_present",
    "identity_matches",
    "sha256",
    "byte_length",
];

/// Substrings that would signal a field able to carry secret or local material.
const FORBIDDEN_PROPERTY_MARKERS: &[&str] = &[
    "auth",
    "token",
    "pat",
    "secret",
    "credential",
    "cookie",
    "header",
    "body",
    "content",
    "path",
    "home",
    "env",
    "password",
    "key",
];

#[test]
fn every_fixture_addresses_exactly_the_planned_request_set() -> Result<()> {
    let fixtures = [
        ("incident", INCIDENT),
        ("available_exact", AVAILABLE_EXACT),
        ("listing_missing", LISTING_MISSING),
        ("rate_limited", RATE_LIMITED),
        ("namespace_absent", NAMESPACE_ABSENT),
        ("digest_mismatch", DIGEST_MISMATCH),
        ("instrument_incomplete", INSTRUMENT_INCOMPLETE),
    ];

    for (label, raw) in fixtures {
        let observation = observation(raw)?;
        let subject = observation
            .expected
            .versions
            .first()
            .ok_or_else(|| eyre!("{label}: fixture must declare an expected version"))?;
        let plan = probe_plan(
            &observation.identity.namespace,
            &observation.identity.extension,
            &subject.version,
        )
        .ok_or_else(|| eyre!("{label}: fixture identity must produce a plan"))?;

        let document: Value = serde_json::from_str(raw)?;
        for cell in Cell::ALL {
            let planned =
                plan.request(cell).ok_or_else(|| eyre!("{label}: {} is unplanned", cell.key()))?;
            let observed = document["cells"][cell.key()]["transport"]["url"]
                .as_str()
                .ok_or_else(|| eyre!("{label}: {} has no URL", cell.key()))?;
            if observed != planned.url {
                bail!("{label}: {} addressed {observed}, planned {}", cell.key(), planned.url);
            }
        }
    }
    Ok(())
}

#[test]
fn the_receipt_is_bound_to_the_plan_digest_for_its_subject() -> Result<()> {
    let receipt = receipt(AVAILABLE_EXACT)?;
    let Some(subject) = receipt.subject_version.as_deref() else {
        bail!("a classifiable observation must record its subject version");
    };
    let plan = probe_plan(&receipt.identity.namespace, &receipt.identity.extension, subject)
        .ok_or_else(|| eyre!("receipt identity must produce a plan"))?;
    if receipt.probe_plan_digest.as_deref() != Some(plan.digest()?.as_str()) {
        bail!("receipt plan digest does not match the plan for its own subject");
    }
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION {
        bail!("receipt schema version drifted: {}", receipt.schema_version);
    }
    Ok(())
}

/// Property names that contain a forbidden marker substring but have been
/// reviewed and found safe.
///
/// The marker check is a deliberately blunt tripwire, so a name it catches is
/// not automatically a defect — it is a prompt to look. Each entry here records
/// that the look happened and what keeps the field safe.
///
/// - `authority`: names the source binding the expected byte identity. It holds
///   a repository-relative path or release reference plus a SHA-256, both value
///   validated by `classify::unsafe_reference` and `is_sha256`; it carries no
///   credential and reaches nothing outside the repository.
const MARKER_EXEMPT_PROPERTIES: &[&str] = &["authority"];

#[test]
fn the_observation_contract_has_no_field_able_to_carry_a_secret() -> Result<()> {
    let schema: Value = serde_json::from_str(OBSERVATION_SCHEMA)?;
    let mut names = Vec::new();
    collect_property_names(&schema, &mut names);
    if names.is_empty() {
        bail!("no properties were collected; the walk is not exercising the schema");
    }

    for name in &names {
        if !PERMITTED_OBSERVATION_PROPERTIES.contains(&name.as_str()) {
            bail!(
                "observation schema defines an unvetted property {name:?}; confirm it cannot \
                 carry response content, headers, credentials or local paths, then add it to \
                 PERMITTED_OBSERVATION_PROPERTIES"
            );
        }
        if MARKER_EXEMPT_PROPERTIES.contains(&name.as_str()) {
            continue;
        }
        let lowered = name.to_ascii_lowercase();
        for marker in FORBIDDEN_PROPERTY_MARKERS {
            if lowered.contains(marker) {
                bail!(
                    "observation schema property {name:?} looks like it can carry {marker}; if it \
                     genuinely cannot, record why in MARKER_EXEMPT_PROPERTIES"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn the_marker_tripwire_still_bites_for_an_unvetted_name() -> Result<()> {
    // An exemption list is only safe while the check it exempts from still
    // works. These names are not in the schema; the point is that the rule
    // would reject them if they appeared.
    for name in ["authorization", "auth_token", "ovsx_pat", "response_body", "home_path"] {
        let exempt = MARKER_EXEMPT_PROPERTIES.contains(&name);
        let flagged = FORBIDDEN_PROPERTY_MARKERS
            .iter()
            .any(|marker| name.to_ascii_lowercase().contains(marker));
        if exempt || !flagged {
            bail!("{name:?} would pass the credential-shape tripwire");
        }
    }
    Ok(())
}

#[test]
fn the_closure_walk_catches_members_declared_outside_properties() -> Result<()> {
    // The walk keyed only on `properties`, so a node using `patternProperties`
    // slipped past it entirely. This pins the widened rule against a shape the
    // contract does not currently use.
    for keyword in ["patternProperties", "propertyNames", "unevaluatedProperties"] {
        let schema: Value = serde_json::from_str(&format!(
            r#"{{"type":"object","additionalProperties":false,
                 "{keyword}":{{"^x$":{{"type":"string"}}}}}}"#
        ))?;
        let mut open = Vec::new();
        collect_open_objects(&schema, "#", &mut open);
        if open.is_empty() {
            bail!("a schema declaring members via {keyword} was not flagged as open");
        }
    }
    Ok(())
}

#[test]
fn the_observation_contract_is_closed_at_every_level() -> Result<()> {
    let schema: Value = serde_json::from_str(OBSERVATION_SCHEMA)?;
    let mut open = Vec::new();
    collect_open_objects(&schema, "#", &mut open);
    if !open.is_empty() {
        bail!(
            "these object schemas accept undeclared fields, so a secret could be smuggled \
             through one: {open:?}"
        );
    }
    Ok(())
}

/// Collect every declared property name anywhere in the schema.
fn collect_property_names(node: &Value, found: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "properties"
                    && let Value::Object(properties) = value
                {
                    for name in properties.keys() {
                        found.push(name.clone());
                    }
                }
                collect_property_names(value, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_property_names(item, found);
            }
        }
        _ => {}
    }
}

/// Keywords whose subschemas refine an already-declared shape rather than
/// declaring one of their own.
///
/// Skipping these is not a weakening: a conditional subschema cannot admit a
/// field, because `additionalProperties: false` on the shape it refines still
/// applies to the same instance. Requiring closure here would instead force
/// every narrowing clause to restate the full property set, which is how a
/// contract drifts.
const APPLICATOR_KEYWORDS: &[&str] = &["if", "then", "else", "not"];

/// Keywords that declare object members without going through `properties`.
///
/// The closure walk keyed only on `properties`, so a node using one of these
/// instead was invisible to it — neither flagged as open nor harvested for
/// name vetting. Nothing in the shipped contract uses them; treating their
/// presence as "open" keeps that true for future edits instead of relying on it.
const UNVETTABLE_MEMBER_KEYWORDS: &[&str] =
    &["patternProperties", "propertyNames", "unevaluatedProperties"];

/// Collect the pointer of every object schema that does not close itself.
fn collect_open_objects(node: &Value, pointer: &str, found: &mut Vec<String>) {
    if let Value::Object(map) = node {
        let declares_members = map.contains_key("properties")
            || UNVETTABLE_MEMBER_KEYWORDS.iter().any(|keyword| map.contains_key(*keyword));
        let opens_by_keyword =
            UNVETTABLE_MEMBER_KEYWORDS.iter().any(|keyword| map.contains_key(*keyword));
        if declares_members
            && (opens_by_keyword || map.get("additionalProperties") != Some(&Value::Bool(false)))
            && !pointer.split('/').any(|segment| APPLICATOR_KEYWORDS.contains(&segment))
        {
            found.push(pointer.to_owned());
        }
        for (key, value) in map {
            // Property names are data, not schema nodes; descend through the
            // container so a property literally named "properties" cannot hide
            // an open object.
            if key == "properties"
                && let Value::Object(properties) = value
            {
                for (name, child) in properties {
                    collect_open_objects(child, &format!("{pointer}/properties/{name}"), found);
                }
            } else {
                collect_open_objects(value, &format!("{pointer}/{key}"), found);
            }
        }
    } else if let Value::Array(items) = node {
        for (index, item) in items.iter().enumerate() {
            collect_open_objects(item, &format!("{pointer}/{index}"), found);
        }
    }
}
