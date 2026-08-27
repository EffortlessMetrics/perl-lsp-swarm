//! Fail-closed evaluation of the pinned manifest JSON Schema
//! (`analysis_reachability_fixture_manifest.v1.schema.json`) against one
//! parsed manifest document (#10998 / PR #12706 review adoption).
//!
//! The schema artifact is version-controlled next to the code it constrains,
//! so the evaluator refuses to guess: any assertion or applicator keyword the
//! schema uses that this evaluator does not implement deterministically makes
//! the whole evaluation a violation instead of silently degrading into an
//! acceptance. Annotation keywords ([`ANNOTATION_KEYWORDS`]) carry no
//! acceptance effect anywhere they appear; every other object key inside the
//! schema document must be an implemented assertion or applicator.

use serde_json::Value;
use std::collections::BTreeSet;

/// Keywords evaluated purely as annotations (or pure containers such as
/// `$defs`) with no acceptance effect.
const ANNOTATION_KEYWORDS: &[&str] =
    &["$schema", "$id", "$defs", "title", "description", "default", "examples"];

/// Maximum `$ref`-resolution depth before failing closed.
const REF_DEPTH_LIMIT: usize = 32;

/// Context-aware keyword census mirroring [`eval`]'s traversal exactly: keys
/// collected by name-bearing applicators (`properties`, `patternProperties`,
/// `$defs`) name INSTANCE members and must not be mistaken for schema
/// keywords; every other object key inside a schema document is one.
fn audit_schema_keywords(schema: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    audit_node(schema, &mut violations);
    violations
}

fn audit_node(node: &Value, out: &mut Vec<String>) {
    let Value::Object(map) = node else {
        audit_container_children(node, out);
        return;
    };
    for (key, value) in map {
        if annotation_keyword(key) || implemented_assertion_keyword(key) {
            audit_applicator(key, value, out);
        } else {
            out.push(format!(
                "{key:?} is not a keyword the fail-closed schema evaluator implements; extend the evaluator or revise the schema"
            ));
            // Keep auditing deeper structure so one unknown keyword does not
            // hide several more.
            audit_applicator(key, value, out);
        }
    }
}

fn audit_applicator(keyword: &str, value: &Value, out: &mut Vec<String>) {
    match keyword {
        // Name→schema maps: keys are instance member names.
        "properties" | "patternProperties" | "$defs" => {
            if let Value::Object(named) = value {
                for sub in named.values() {
                    audit_node(sub, out);
                }
            }
        }
        _ => audit_node(value, out),
    }
}

fn audit_container_children(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Array(items) => {
            for item in items {
                audit_node(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                audit_node(value, out);
            }
        }
        _ => {}
    }
}

fn annotation_keyword(keyword: &str) -> bool {
    ANNOTATION_KEYWORDS.contains(&keyword)
}

/// Every keyword this evaluator's [`eval`] match has a real enforcement arm
/// for. Keep in lockstep with the match below; the defensive fallthrough
/// converts any drift here into a violation rather than silent acceptance.
fn implemented_assertion_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "$ref"
            | "type"
            | "const"
            | "enum"
            | "required"
            | "properties"
            | "additionalProperties"
            | "patternProperties"
            | "propertyNames"
            | "items"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "minProperties"
            | "maxProperties"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
    )
}

/// Evaluates `instance` against `schema`. Every violation names the instance
/// location so a diff against the wire contract stays reviewable.
pub fn evaluate(schema: &Value, instance: &Value) -> Vec<String> {
    let keyword_violations = audit_schema_keywords(schema);
    if !keyword_violations.is_empty() {
        return keyword_violations;
    }
    let root = schema.clone();
    let mut violations = Vec::new();
    eval(schema, instance, &root, "$", REF_DEPTH_LIMIT, &mut violations);
    violations
}

fn eval(
    schema: &Value,
    instance: &Value,
    root: &Value,
    at: &str,
    depth_left: usize,
    out: &mut Vec<String>,
) {
    if depth_left == 0 {
        out.push(format!("{at}: $ref resolution exceeded depth limit"));
        return;
    }
    let Some(map) = schema.as_object() else { return };

    for (keyword, subschema) in map {
        match keyword.as_str() {
            "$schema" | "$id" | "$defs" | "title" | "description" | "default" | "examples" => {}
            "$ref" => {
                let Some(reference) = subschema.as_str() else {
                    out.push(format!("{at}: $ref must be a string"));
                    continue;
                };
                match resolve_pointer(root, reference) {
                    Ok(resolved) => eval(resolved, instance, root, at, depth_left - 1, out),
                    Err(error) => {
                        out.push(format!("{at}: unresolvable $ref {reference:?}: {error}"))
                    }
                }
            }
            "type" => check_type(subschema, instance, at, out),
            "const" => {
                if subschema != instance {
                    out.push(format!(
                        "{at}: violates schema const {subschema}; document carries {instance}"
                    ));
                }
            }
            "enum" => {
                let Some(allowed) = subschema.as_array() else {
                    out.push(format!("{at}: enum must be an array"));
                    continue;
                };
                if !allowed.contains(instance) {
                    out.push(format!("{at}: value {instance} is outside the schema enum"));
                }
            }
            "required" => {
                let Some(required_keys) = subschema.as_array() else {
                    out.push(format!("{at}: required must be an array of names"));
                    continue;
                };
                if let Some(object) = instance.as_object() {
                    for key in required_keys {
                        match key.as_str() {
                            Some(name) if !object.contains_key(name) => {
                                out.push(format!("{at}: missing required property {name:?}"));
                            }
                            None => out.push(format!("{at}: required entries must be strings")),
                            _ => {}
                        }
                    }
                }
            }
            "properties" => {
                let Some(properties) = subschema.as_object() else {
                    out.push(format!("{at}: properties must be an object"));
                    continue;
                };
                if let Some(object) = instance.as_object() {
                    for (name, property_schema) in properties {
                        if let Some(value) = object.get(name) {
                            eval(
                                property_schema,
                                value,
                                root,
                                &format!("{at}.{name}"),
                                depth_left - 1,
                                out,
                            );
                        }
                    }
                }
            }
            "patternProperties" => {
                let Some(patterns) = subschema.as_object() else {
                    out.push(format!("{at}: patternProperties must be an object"));
                    continue;
                };
                if let Some(object) = instance.as_object() {
                    for (source, property_schema) in patterns {
                        match compile(source) {
                            Ok(matcher) => {
                                for (key, value) in object {
                                    if matcher.is_match(key) {
                                        eval(
                                            property_schema,
                                            value,
                                            root,
                                            &format!("{at}.{key}"),
                                            depth_left - 1,
                                            out,
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                out.push(format!("{at}: invalid patternProperty regex: {error}"));
                            }
                        }
                    }
                }
            }
            "propertyNames" => {
                if let Some(object) = instance.as_object() {
                    match string_schema(subschema) {
                        Ok((matcher, min_length, max_length)) => {
                            for key in object.keys() {
                                check_string_subject(
                                    &key_value(key),
                                    matcher.as_ref(),
                                    min_length,
                                    max_length,
                                    subschema.get("enum").and_then(Value::as_array),
                                    at,
                                    out,
                                );
                            }
                        }
                        Err(error) => out.push(format!("{at}: propertyNames {error}")),
                    }
                }
            }
            "items" => match subschema {
                item_schema @ Value::Object(_) => {
                    if let Some(elements) = instance.as_array() {
                        for (index, element) in elements.iter().enumerate() {
                            eval(
                                item_schema,
                                element,
                                root,
                                &format!("{at}[{index}]"),
                                depth_left - 1,
                                out,
                            );
                        }
                    }
                }
                // Positional tuple form is deliberately unsupported: the
                // pinned schema never uses it and silently mis-evaluating
                // tuples would be worse than refusing.
                Value::Array(_) => out.push(format!(
                    "{at}: array-form items (tuple validation) is unsupported by the fail-closed evaluator"
                )),
                _ => out.push(format!("{at}: items must be a schema object")),
            },
            "additionalProperties" => match subschema {
                Value::Bool(false) => {
                    enforce_closed_object(map, instance, at, out);
                }
                Value::Bool(true) => {}
                nested @ Value::Object(_) => {
                    if let Some(object) = instance.as_object() {
                        let declared = map
                            .get("properties")
                            .and_then(Value::as_object)
                            .map(|properties| properties.keys().cloned().collect::<BTreeSet<_>>())
                            .unwrap_or_default();
                        for (key, value) in object {
                            if !declared.contains(key) {
                                eval(nested, value, root, &format!("{at}.{key}"), depth_left - 1, out);
                            }
                        }
                    }
                }
                _ => out.push(format!("{at}: additionalProperties must be boolean or schema")),
            },
            "pattern" => {
                let Some(source) = subschema.as_str() else {
                    out.push(format!("{at}: pattern must be a string"));
                    continue;
                };
                match compile(source) {
                    Ok(matcher) => {
                        if instance.as_str().map(|text| !matcher.is_match(text)).unwrap_or(false) {
                            out.push(format!(
                                "{at}: value {instance} violates schema pattern {source:?}"
                            ));
                        }
                    }
                    Err(error) => out.push(format!("{at}: invalid schema pattern: {error}")),
                }
            }
            "minLength" | "maxLength" => {
                if let Some(limit) = subschema.as_u64() {
                    if let Some(text) = instance.as_str() {
                        let length = text.chars().count() as u64;
                        let violated =
                            length < limit || (keyword == "maxLength" && length > limit);
                        if violated {
                            out.push(format!("{at}: length {length} violates {keyword} {limit}"));
                        }
                    }
                } else {
                    out.push(format!("{at}: {keyword} must be a non-negative integer"));
                }
            }
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                if let Some(limit) = subschema.as_f64() {
                    if let Some(number) = instance.as_f64() {
                        let violated = match keyword.as_str() {
                            "minimum" => number < limit,
                            "maximum" => number > limit,
                            "exclusiveMinimum" => number <= limit,
                            _ => number >= limit,
                        };
                        if violated {
                            out.push(format!("{at}: value {number} violates {keyword} {limit}"));
                        }
                    }
                } else {
                    out.push(format!("{at}: {keyword} must be numeric"));
                }
            }
            "multipleOf" => {
                if let Some(divisor) = subschema.as_f64() {
                    if divisor <= 0.0 {
                        out.push(format!("{at}: multipleOf must be positive"));
                    } else if let Some(number) = instance.as_f64() {
                        let quotient = number / divisor;
                        let remainder_distance = (quotient - quotient.round()).abs();
                        if remainder_distance > f64::EPSILON {
                            out.push(format!("{at}: value {number} is not a multiple of {divisor}"));
                        }
                    }
                } else {
                    out.push(format!("{at}: multipleOf must be numeric"));
                }
            }
            "minItems" | "maxItems" | "minProperties" | "maxProperties" => {
                if let Some(limit) = subschema.as_u64() {
                    let count = match instance {
                        Value::Array(elements) => elements.len() as u64,
                        Value::Object(object) => object.len() as u64,
                        _ => return,
                    };
                    let violated = keyword.starts_with("min") && count < limit
                        || keyword.starts_with("max") && count > limit;
                    if violated {
                        out.push(format!("{at}: size {count} violates {keyword} {limit}"));
                    }
                } else {
                    out.push(format!("{at}: {keyword} must be a non-negative integer"));
                }
            }
            "uniqueItems" => {
                if subschema.as_bool() == Some(true)
                    && let Some(elements) = instance.as_array()
                {
                    for (index, element) in elements.iter().enumerate() {
                        if elements.iter().take(index).any(|earlier| earlier == element) {
                            out.push(format!("{at}[{index}]: duplicate array entry"));
                        }
                    }
                }
            }
            other => {
                // Audit and match have drifted apart; fail closed.
                out.push(format!(
                    "{other:?} reached evaluation without an implemented arm; refusing to accept the document"
                ));
            }
        }
    }
}

fn enforce_closed_object(
    schema_map: &serde_json::Map<String, Value>,
    instance: &Value,
    at: &str,
    out: &mut Vec<String>,
) {
    let Some(object) = instance.as_object() else { return };
    let declared_properties = schema_map.get("properties").and_then(Value::as_object);
    let declared_patterns: Vec<_> = schema_map
        .get("patternProperties")
        .and_then(Value::as_object)
        .map(|patterns| {
            patterns.keys().filter_map(|source| compile(source).ok()).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for key in object.keys() {
        let covered_by_pattern = declared_patterns.iter().any(|matcher| matcher.is_match(key));
        if !declared_properties.map(|properties| properties.contains_key(key)).unwrap_or(false)
            && !covered_by_pattern
        {
            out.push(format!(
                "{at}: additional property {key:?} is not allowed by the closed schema"
            ));
        }
    }
}

fn compile(source: &str) -> Result<regex::Regex, regex::Error> {
    regex::Regex::new(source)
}

/// Interprets a propertyNames subschema as a string-only constraint bundle.
type StringConstraints = (Option<regex::Regex>, Option<u64>, Option<u64>);

fn string_schema(subschema: &Value) -> Result<StringConstraints, String> {
    let type_ok = matches!(subschema.get("type").and_then(Value::as_str), None | Some("string"));
    if !type_ok {
        return Err("must constrain strings".to_string());
    }
    let pattern = match subschema.get("pattern").and_then(Value::as_str) {
        Some(source) => Some(compile(source).map_err(|error| format!("invalid pattern: {error}"))?),
        None => None,
    };
    Ok((
        pattern,
        subschema.get("minLength").and_then(Value::as_u64),
        subschema.get("maxLength").and_then(Value::as_u64),
    ))
}

fn key_value(text: &str) -> Value {
    Value::String(text.to_string())
}

/// Evaluates one bare-string subject (a property name or key) against string
/// assertions compiled earlier.
fn check_string_subject(
    subject: &Value,
    matcher: Option<&regex::Regex>,
    min_length: Option<u64>,
    max_length: Option<u64>,
    allowed_enum: Option<&Vec<Value>>,
    at: &str,
    out: &mut Vec<String>,
) {
    let text = subject.as_str().unwrap_or_default();
    if let Some(matcher) = matcher
        && !matcher.is_match(text)
    {
        out.push(format!("{at}: name {text:?} violates its schema pattern"));
    }
    if let Some(limit) = min_length {
        let length = text.chars().count() as u64;
        if length < limit {
            out.push(format!("{at}: name {text:?} violates minLength {limit}"));
        }
    }
    if let Some(limit) = max_length {
        let length = text.chars().count() as u64;
        if length > limit {
            out.push(format!("{at}: name {text:?} violates maxLength {limit}"));
        }
    }
    if let Some(allowed) = allowed_enum
        && !allowed.contains(subject)
    {
        out.push(format!("{at}: name {text:?} is outside the schema enum"));
    }
}

fn check_type(expected: &Value, instance: &Value, at: &str, out: &mut Vec<String>) {
    let Some(expected_name) = expected.as_str() else {
        out.push(format!("{at}: type must be a string"));
        return;
    };
    let satisfied = match expected_name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "integer" => instance.as_i64().is_some() || instance.as_u64().is_some(),
        "number" => instance.is_number(),
        other => {
            out.push(format!("{at}: unknown schema type {other:?}"));
            return;
        }
    };
    if !satisfied {
        out.push(format!("{at}: expected type {expected_name}, found {instance}"));
    }
}

fn resolve_pointer<'a>(root: &'a Value, reference: &str) -> Result<&'a Value, String> {
    let pointer = reference
        .strip_prefix('#')
        .ok_or_else(|| "only local #/... references are supported".to_string())?;
    if pointer.is_empty() {
        return Ok(root);
    }
    root.pointer(pointer).ok_or_else(|| format!("pointer {pointer:?} does not exist"))
}
