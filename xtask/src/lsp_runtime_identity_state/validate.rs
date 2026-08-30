use std::collections::BTreeSet;

use color_eyre::eyre::{Result, bail, eyre};
use serde_json::Value;

use super::*;

impl Vocabulary {
    pub(super) fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA_NAME || self.version != SCHEMA_VERSION {
            bail!("unknown vocabulary schema/version: {:?}/{}", self.schema, self.version);
        }
        if (self.authority.issue, self.authority.architecture, self.authority.train)
            != (11045, 7384, 10360)
        {
            bail!("authority must remain #11045 under #7384 and checked train #10360");
        }
        exact_u64s("authority.consumers", &self.authority.consumers, &[10512, 11051, 11053])?;
        nonempty("authority.claim", &self.authority.claim)?;
        if self.fragments.identities != "identities.v1.json"
            || self.fragments.states != "states.v1.json"
            || self.fragments.relations != "relations.v1.json"
            || self.fragments.journeys != "journeys.v1.json"
        {
            bail!("contract fragment identities must remain exact and versioned");
        }

        if self.request_state.kind != "orthogonal_axes"
            || !self.request_state.linear_phase_forbidden
        {
            bail!("request state must remain orthogonal_axes and forbid a total linear phase");
        }
        exact_strings(
            "request_state.axes",
            self.request_state.axes.iter().map(String::as_str),
            &[
                "cleanup",
                "control",
                "execution",
                "output_delivery",
                "protocol_admission",
                "publication",
                "terminal",
                "tracking_registration",
            ],
        )?;
        nonempty("request_state.law", &self.request_state.law)?;

        let one = &self.generic_boundary.one_authority;
        if one.single_object || one.single_actor || one.global_lock || one.single_store {
            bail!(
                "one authority must not require one object, actor, global lock, or mutable store"
            );
        }
        nonempty("generic_boundary.one_authority.law", &one.law)?;
        if self.generic_boundary.client_consumption_claimable {
            bail!("client consumption is outside reusable-runtime claim authority");
        }
        nonempty("generic_boundary.currentness_law", &self.generic_boundary.currentness_law)?;
        exact_strings(
            "generic_boundary.forbidden_terms",
            self.generic_boundary.forbidden_terms.iter().map(String::as_str),
            &["document", "lexer", "parser", "perl", "provider", "workspace"],
        )?;

        validate_named_rows("axes", &self.axes, |row| (&row.id, row.source))?;
        validate_named_rows("identities", &self.identities, |row| (&row.id, row.source))?;
        validate_named_rows("boundary_terms", &self.boundary_terms, |row| (&row.id, row.source))?;
        validate_named_rows("states", &self.states, |row| (&row.id, row.source))?;
        validate_named_rows("relations", &self.relations, |row| (&row.id, row.source))?;
        validate_named_rows("journeys", &self.journeys, |row| (&row.id, row.source))?;

        exact_strings("axes", self.axes.iter().map(|row| row.id.as_str()), REQUIRED_AXES)?;
        exact_strings(
            "identities",
            self.identities.iter().map(|row| row.id.as_str()),
            REQUIRED_IDENTITIES,
        )?;
        exact_strings(
            "boundary_terms",
            self.boundary_terms.iter().map(|row| row.id.as_str()),
            REQUIRED_BOUNDARY_TERMS,
        )?;
        exact_strings("states", self.states.iter().map(|row| row.id.as_str()), REQUIRED_STATES)?;
        exact_strings(
            "ambiguous_terms",
            self.ambiguous_terms.iter().map(|row| row.term.as_str()),
            REQUIRED_AMBIGUOUS_TERMS,
        )?;
        exact_strings(
            "journeys",
            self.journeys.iter().map(|row| row.id.as_str()),
            REQUIRED_JOURNEYS,
        )?;

        let axis_ids: BTreeSet<&str> = self.axes.iter().map(|row| row.id.as_str()).collect();
        let identity_ids: BTreeSet<&str> =
            self.identities.iter().map(|row| row.id.as_str()).collect();
        let concepts = concept_id_set(self);
        let relation_ids: BTreeSet<&str> =
            self.relations.iter().map(|row| row.id.as_str()).collect();

        for identity in &self.identities {
            nonempty("identity.name", &identity.name)?;
            nonempty("identity.proposition", &identity.proposition)?;
            nonempty("identity.scope", &identity.scope)?;
            nonempty("identity.owner", &identity.owner)?;
            nonempty("identity.equality", &identity.equality)?;
            nonempty("identity.lifetime", &identity.lifetime)?;
            known_refs(
                &format!("identity {} scopes", identity.id),
                &identity.scoped_by,
                &identity_ids,
            )?;
        }
        for term in &self.boundary_terms {
            nonempty("boundary term name", &term.name)?;
            nonempty("boundary term proposition", &term.proposition)?;
            nonempty("boundary term owner", &term.owner)?;
        }
        for state in &self.states {
            nonempty("state name", &state.name)?;
            nonempty("state proposition", &state.proposition)?;
            if !axis_ids.contains(state.axis.as_str()) {
                bail!("state {} references unknown axis {}", state.id, state.axis);
            }
        }

        validate_request_key(self, "request_key")?;
        validate_request_key(self, "reverse_request_key")?;
        let currentness = identity(self, "currentness_token")?;
        if !currentness.opaque || !currentness.owner_validated {
            bail!("CurrentnessToken must remain opaque and owner-validated");
        }
        let client = state(self, "client_consumed")?;
        if client.runtime_claimable {
            bail!("client_consumed must remain external-only");
        }
        if self
            .states
            .iter()
            .filter(|row| row.id != "client_consumed")
            .any(|row| !row.runtime_claimable)
        {
            bail!("client_consumed is the only external-only normative state");
        }

        let mut relation_keys = BTreeSet::new();
        for relation in &self.relations {
            if !concepts.contains(relation.from_id.as_str())
                || !concepts.contains(relation.to.as_str())
            {
                bail!(
                    "relationship {} references unknown concepts {} -> {}",
                    relation.id,
                    relation.from_id,
                    relation.to
                );
            }
            nonempty("relationship.reason", &relation.reason)?;
            let key = format!("{}|{}|{}", relation.from_id, relation.kind.as_str(), relation.to);
            if !relation_keys.insert(key) {
                bail!("duplicate relationship semantic in {}", relation.id);
            }
        }
        for required in REQUIRED_RELATION_KEYS {
            if !relation_keys.contains(*required) {
                bail!("missing required relationship {required}");
            }
        }

        for term in &self.ambiguous_terms {
            if term.replacements.len() < 2 {
                bail!("ambiguous term {} needs at least two exact replacements", term.term);
            }
            nonempty("ambiguous term reason", &term.reason)?;
            known_refs(
                &format!("ambiguous term {} replacements", term.term),
                &term.replacements,
                &concepts,
            )?;
        }
        for journey in &self.journeys {
            nonempty("journey.title", &journey.title)?;
            nonempty("journey.proposition", &journey.proposition)?;
            if journey.facts.is_empty()
                || journey.relations.is_empty()
                || journey.rejected.is_empty()
            {
                bail!(
                    "journey {} must carry facts, legal relations, and rejected inferences",
                    journey.id
                );
            }
            known_refs(&format!("journey {} facts", journey.id), &journey.facts, &concepts)?;
            known_refs(
                &format!("journey {} relations", journey.id),
                &journey.relations,
                &relation_ids,
            )?;
            known_refs(
                &format!("journey {} rejected", journey.id),
                &journey.rejected,
                &relation_ids,
            )?;
            for rejected in &journey.rejected {
                let relation = self
                    .relations
                    .iter()
                    .find(|row| row.id == *rejected)
                    .ok_or_else(|| eyre!("missing rejected relation {rejected}"))?;
                if !matches!(
                    relation.kind,
                    RelationKind::ForbidsInference | RelationKind::IndependentOf
                ) {
                    bail!(
                        "journey {} rejected relation {} is neither forbids_inference nor independent_of",
                        journey.id,
                        rejected
                    );
                }
            }
        }

        reject_product_terms(self)?;
        Ok(())
    }
}

fn validate_named_rows<T, F>(label: &str, rows: &[T], mut key: F) -> Result<()>
where
    F: for<'a> FnMut(&'a T) -> (&'a str, u64),
{
    let mut ids = BTreeSet::new();
    for row in rows {
        let (id, source) = key(row);
        machine_id(label, id)?;
        if !ids.insert(id) {
            bail!("duplicate {label} id {id}");
        }
        if source == 0 {
            bail!("{label} {id} has no source issue");
        }
    }
    Ok(())
}

fn validate_request_key(vocabulary: &Vocabulary, id: &str) -> Result<()> {
    let key = identity(vocabulary, id)?;
    exact_strings(
        &format!("{id}.variants"),
        key.variants.iter().map(String::as_str),
        &["numeric", "string"],
    )?;
    exact_strings(
        &format!("{id}.scoped_by"),
        key.scoped_by.iter().map(String::as_str),
        &["connection_id", "session_id"],
    )?;
    Ok(())
}

fn identity<'a>(vocabulary: &'a Vocabulary, id: &str) -> Result<&'a Identity> {
    vocabulary
        .identities
        .iter()
        .find(|row| row.id == id)
        .ok_or_else(|| eyre!("missing identity {id}"))
}

fn state<'a>(vocabulary: &'a Vocabulary, id: &str) -> Result<&'a StateTerm> {
    vocabulary.states.iter().find(|row| row.id == id).ok_or_else(|| eyre!("missing state {id}"))
}

fn known_refs(label: &str, refs: &[String], known: &BTreeSet<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for reference in refs {
        if !seen.insert(reference.as_str()) {
            bail!("{label} contains duplicate reference {reference}");
        }
        if !known.contains(reference.as_str()) {
            bail!("{label} references unknown id {reference}");
        }
    }
    Ok(())
}

fn exact_strings<'a>(
    label: &str,
    actual: impl Iterator<Item = &'a str>,
    required: &[&str],
) -> Result<()> {
    let actual_values: Vec<&str> = actual.collect();
    let actual: BTreeSet<&str> = actual_values.iter().copied().collect();
    let required: BTreeSet<&str> = required.iter().copied().collect();
    if actual != required || actual_values.len() != actual.len() {
        bail!("{label} denominator mismatch: actual={actual:?} required={required:?}");
    }
    Ok(())
}

fn exact_u64s(label: &str, actual: &[u64], required: &[u64]) -> Result<()> {
    let actual_values = actual;
    let actual: BTreeSet<u64> = actual_values.iter().copied().collect();
    let required: BTreeSet<u64> = required.iter().copied().collect();
    if actual != required || actual_values.len() != actual.len() {
        bail!("{label} denominator mismatch");
    }
    Ok(())
}

fn machine_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("invalid {label} id {value:?}");
    }
    Ok(())
}

fn nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn reject_product_terms(vocabulary: &Vocabulary) -> Result<()> {
    let forbidden = &vocabulary.generic_boundary.forbidden_terms;
    for (label, value) in [
        ("authority", serde_json::to_value(&vocabulary.authority)?),
        ("request_state", serde_json::to_value(&vocabulary.request_state)?),
        ("axes", serde_json::to_value(&vocabulary.axes)?),
        ("identities", serde_json::to_value(&vocabulary.identities)?),
        ("boundary_terms", serde_json::to_value(&vocabulary.boundary_terms)?),
        ("states", serde_json::to_value(&vocabulary.states)?),
        ("relations", serde_json::to_value(&vocabulary.relations)?),
        ("ambiguous_terms", serde_json::to_value(&vocabulary.ambiguous_terms)?),
        ("journeys", serde_json::to_value(&vocabulary.journeys)?),
    ] {
        scan_strings(label, &value, forbidden)?;
    }
    Ok(())
}

fn scan_strings(label: &str, value: &Value, forbidden: &[String]) -> Result<()> {
    match value {
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            for term in forbidden {
                if lower.contains(term) {
                    bail!("{label} contains forbidden generic-domain term {term:?}");
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                scan_strings(label, value, forbidden)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                scan_strings(label, value, forbidden)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}
