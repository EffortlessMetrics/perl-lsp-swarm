//! Minimal structural JSON Schema validator shared by the harness contract
//! suites.
//!
//! This is the single proof instrument used whenever a produced receipt is
//! checked against its registered schema. It covers exactly the JSON Schema
//! keywords the registered harness schemas use; any unknown keyword or pattern
//! grammar fails closed rather than silently passing.
//!
//! "Fails closed" is enforced by [`KNOWN_KEYWORDS`], not merely intended. An
//! unimplemented keyword that were simply skipped would fail *open* — the
//! schema would appear to pass while the constraint it declares went
//! unchecked — so a keyword outside that list is a hard error, and a schema
//! that grows one fails here until this validator implements it.
//!
//! One shared instrument matters for the closed-world claim (#7729): a suite
//! that carried its own copy could drift into accepting a shape another suite
//! rejects, and "Rust and the registered schema agree" would no longer be a
//! statement about one checkable rule. Test-only; never compiled into the
//! production surface.

use serde_json::Value;

pub fn validate(root: &Value, instance: &Value) -> Result<(), String> {
    check(root, root, instance)
}

/// Validates `instance` against one subschema `node`, resolving any `$ref` it
/// contains against `root`. Callers that need to ask which branch of a `oneOf`
/// an instance actually satisfies use this rather than re-implementing the
/// keyword walk.
pub fn validate_node(root: &Value, node: &Value, instance: &Value) -> Result<(), String> {
    check(node, root, instance)
}

/// Every schema keyword this validator implements, plus the annotation
/// keywords it may ignore safely.
///
/// A keyword outside this list is rejected rather than skipped. Silently
/// ignoring an unimplemented keyword fails *open*: the schema would appear to
/// pass while a constraint it declares went unchecked. Adding a keyword to a
/// registered schema therefore fails here until it is implemented above.
const KNOWN_KEYWORDS: &[&str] = &[
    // Annotations, carrying no assertion.
    "$schema",
    "$id",
    "$comment",
    "$defs",
    "title",
    "description",
    "examples",
    "default",
    "deprecated",
    // Applicators.
    "$ref",
    "allOf",
    "anyOf",
    "oneOf",
    "if",
    "then",
    "else",
    "items",
    "properties",
    "additionalProperties",
    // Assertions.
    "type",
    "const",
    "enum",
    "pattern",
    "required",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minProperties",
    "maxProperties",
];

fn check(schema: &Value, root: &Value, instance: &Value) -> Result<(), String> {
    // A boolean schema accepts or rejects everything.
    if let Some(accepted) = schema.as_bool() {
        return match accepted {
            true => Ok(()),
            false => Err("boolean schema false rejects every instance".to_string()),
        };
    }
    if let Some(fields) = schema.as_object() {
        for key in fields.keys() {
            if !KNOWN_KEYWORDS.contains(&key.as_str()) {
                return Err(format!("unsupported schema keyword {key}"));
            }
        }
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference.strip_prefix('#').unwrap_or(reference);
        let target =
            root.pointer(pointer).ok_or_else(|| format!("schema $ref {reference} unresolved"))?;
        return check(target, root, instance);
    }
    if let Some(expected) = schema.get("type") {
        let satisfied = match expected {
            Value::String(name) => type_matches(name, instance)?,
            Value::Array(names) => {
                let mut matched = false;
                for name in names.iter().filter_map(Value::as_str) {
                    matched = matched || type_matches(name, instance)?;
                }
                matched
            }
            other => return Err(format!("unsupported schema type shape {other}")),
        };
        if !satisfied {
            return Err(format!("instance violates type constraint {expected}"));
        }
    }
    if let Some(expected) = schema.get("const")
        && instance != expected
    {
        return Err(format!("instance violates const {expected}"));
    }
    if let Some(expected) = schema.get("enum").and_then(Value::as_array)
        && !expected.contains(instance)
    {
        return Err(format!("instance is outside enum {expected:?}"));
    }
    // Pattern/numeric keywords constrain their matching instance types;
    // other types are governed solely by the checked `type` keyword.
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
        && let Some(text) = instance.as_str()
    {
        anchored_pattern_matches(pattern, text)?;
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64)
        && let Some(number) = instance.as_i64()
        && number < minimum
    {
        return Err(format!("instance {number} is below minimum {minimum}"));
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_i64)
        && let Some(number) = instance.as_i64()
        && number > maximum
    {
        return Err(format!("instance {number} is above maximum {maximum}"));
    }
    match instance {
        Value::String(text) => {
            if let Some(min) = schema.get("minLength").and_then(Value::as_u64)
                && (text.chars().count() as u64) < min
            {
                return Err(format!("string shorter than minLength {min}"));
            }
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64)
                && (text.chars().count() as u64) > max
            {
                return Err(format!("string longer than maxLength {max}"));
            }
        }
        Value::Array(items) => {
            if let Some(min) = schema.get("minItems").and_then(Value::as_u64)
                && (items.len() as u64) < min
            {
                return Err(format!("array shorter than minItems {min}"));
            }
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64)
                && (items.len() as u64) > max
            {
                return Err(format!("array longer than maxItems {max}"));
            }
            if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
                let duplicated = items
                    .iter()
                    .enumerate()
                    .any(|(index, item)| items[index + 1..].iter().any(|later| later == item));
                if duplicated {
                    return Err("array items are not unique".to_string());
                }
            }
            if let Some(item_schema) = schema.get("items") {
                for item in items {
                    check(item_schema, root, item)?;
                }
            }
        }
        Value::Object(object) => {
            if let Some(min) = schema.get("minProperties").and_then(Value::as_u64)
                && (object.len() as u64) < min
            {
                return Err(format!("object has fewer than minProperties {min}"));
            }
            if let Some(max) = schema.get("maxProperties").and_then(Value::as_u64)
                && (object.len() as u64) > max
            {
                return Err(format!("object has more than maxProperties {max}"));
            }
            for key in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
                let key = key.as_str().ok_or("required entries must be strings")?;
                if !object.contains_key(key) {
                    return Err(format!("object is missing required key {key}"));
                }
            }
            let properties = schema.get("properties").and_then(Value::as_object);
            let additional = schema.get("additionalProperties");
            for (key, value) in object {
                match properties.and_then(|properties| properties.get(key)) {
                    Some(key_schema) => check(key_schema, root, value)?,
                    None => match additional {
                        Some(&Value::Bool(false)) => {
                            return Err(format!("object carries unknown property {key}"));
                        }
                        Some(additional_schema) => {
                            check(additional_schema, root, value)?;
                        }
                        _ => {}
                    },
                }
            }
        }
        _ => {}
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            check(branch, root, instance)?;
        }
    }
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array)
        && !branches.iter().any(|branch| check(branch, root, instance).is_ok())
    {
        let details = branches
            .iter()
            .filter_map(|branch| check(branch, root, instance).err())
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!("instance satisfies no anyOf branch; branch errors: {details}"));
    }
    // `if`/`then`/`else`: the drift contract states its status-conditional
    // invariants this way, so ignoring them would let a `not_proven` receipt
    // carry a fingerprint and populated drift arrays unchallenged.
    if let Some(condition) = schema.get("if") {
        let matched = check(condition, root, instance).is_ok();
        let branch = match matched {
            true => schema.get("then"),
            false => schema.get("else"),
        };
        if let Some(branch) = branch {
            check(branch, root, instance).map_err(|error| {
                let arm = match matched {
                    true => "then",
                    false => "else",
                };
                format!("instance violates conditional {arm}: {error}")
            })?;
        }
    }
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        let passing =
            branches.iter().filter(|branch| check(branch, root, instance).is_ok()).count();
        if passing != 1 {
            let details = branches
                .iter()
                .filter_map(|branch| check(branch, root, instance).err())
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(format!(
                "instance satisfies {passing} oneOf branches, expected 1; branch errors: {details}"
            ));
        }
    }
    Ok(())
}

fn type_matches(name: &str, instance: &Value) -> Result<bool, String> {
    Ok(match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "integer" => instance.is_i64() || instance.is_u64(),
        "number" => instance.is_number(),
        other => return Err(format!("unsupported schema type name {other}")),
    })
}

/// Anchored pattern matcher for the single-piece character-class shapes
/// this schema uses (`^[class]{n,m}$`, `^[class]+$`, `^([class]{w})*$`).
/// Any other grammar fails closed instead of passing.
fn anchored_pattern_matches(pattern: &str, text: &str) -> Result<(), String> {
    let unsupported = || format!("unsupported pattern grammar {pattern}");
    let after_caret = pattern.strip_prefix('^').ok_or_else(unsupported)?;
    let Some(body) = after_caret.strip_suffix('$') else {
        // Start-anchored with no end anchor: the only grammar the registered
        // schemas use in this shape is a literal prefix such as `^\.\./`.
        // Anything carrying a real metacharacter fails closed rather than
        // matching loosely.
        let literal = literal_prefix(after_caret).ok_or_else(unsupported)?;
        return match text.starts_with(&literal) {
            true => Ok(()),
            false => Err(format!("text {text:?} does not start with {literal:?}")),
        };
    };
    // A trailing literal (`^[^/\\]+\.json$`) is split off before the class is
    // matched; the class governs only the part of the text it actually covers.
    let (body, tail) = split_class_and_literal_tail(body)?;
    let text = match text.strip_suffix(tail.as_str()) {
        Some(head) => head,
        None => return Err(format!("text {text:?} does not end with {tail:?}")),
    };
    let body = body.as_str();
    let (unit_width, min_units, max_units, class_body) =
        if let Some(inner) = body.strip_prefix('(').and_then(|rest| rest.strip_suffix(")*")) {
            let (class_body, width) = split_bracket_and_exact_repeat(inner)?;
            (width, 0, None, class_body)
        } else {
            let (class_body, quantifier) = split_bracket_and_quantifier(body)?;
            match quantifier {
                Quantifier::OneOrMore | Quantifier::Plain => (1, 1, None, class_body),
                Quantifier::Exact(units) => (1, units, Some(units), class_body),
                Quantifier::Bounded(low, high) => (1, low, Some(high), class_body),
            }
        };
    let class = parse_char_class(class_body)?;
    let bytes = text.as_bytes();
    if !bytes.iter().all(|byte| class.contains(*byte)) {
        return Err(format!("text {text:?} contains characters outside {pattern}"));
    }
    if bytes.len() % unit_width != 0 {
        return Err(format!("text {text:?} length does not fit pattern {pattern}"));
    }
    let units = (bytes.len() / unit_width) as u64;
    if units < min_units || max_units.is_some_and(|max| units > max) {
        return Err(format!("text {text:?} length does not satisfy pattern {pattern}"));
    }
    Ok(())
}

/// Splits a bracket-class body from an optional trailing literal, as in
/// `[^/\\]+\.json`. Patterns with no bracket group are returned unchanged with
/// an empty tail. A tail carrying a metacharacter fails closed.
fn split_class_and_literal_tail(body: &str) -> Result<(String, String), String> {
    if !body.starts_with('[') {
        return Ok((body.to_string(), String::new()));
    }
    let close = body.find(']').ok_or_else(|| format!("bad class in {body}"))?;
    // The quantifier immediately follows the class; the literal tail is
    // whatever remains after it.
    let rest = &body[close + 1..];
    let quantifier_end = match rest.strip_prefix('{') {
        Some(after) => {
            close + 2 + after.find('}').ok_or_else(|| format!("bad quantifier in {body}"))? + 1
        }
        None => match rest.starts_with('+') {
            true => close + 2,
            false => close + 1,
        },
    };
    let (head, tail) = body.split_at(quantifier_end);
    if tail.is_empty() {
        return Ok((head.to_string(), String::new()));
    }
    let literal =
        literal_prefix(tail).ok_or_else(|| format!("unsupported pattern tail in {body}"))?;
    Ok((head.to_string(), literal))
}

/// Unescapes a pattern body that is a plain literal (`\.` becomes `.`).
/// Returns `None` as soon as an unescaped regex metacharacter appears, so an
/// unsupported grammar fails closed instead of being matched approximately.
fn literal_prefix(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => out.push(chars.next()?),
            '[' | ']' | '(' | ')' | '{' | '}' | '*' | '+' | '?' | '|' | '.' | '^' | '$' => {
                return None;
            }
            other => out.push(other),
        }
    }
    Some(out)
}

enum Quantifier {
    Plain,
    OneOrMore,
    Exact(u64),
    Bounded(u64, u64),
}

/// Splits `[class]` plus an optional `{n}`, `{n,m}`, or `+` suffix.
fn split_bracket_and_quantifier(body: &str) -> Result<(&str, Quantifier), String> {
    if let Some(rest) = body.strip_suffix('+') {
        let class = strip_brackets(rest).ok_or_else(|| format!("bad class in {body}"))?;
        return Ok((class, Quantifier::OneOrMore));
    }
    let Some((core, suffix)) = body.split_once('{') else {
        let class = strip_brackets(body).ok_or_else(|| format!("bad class in {body}"))?;
        return Ok((class, Quantifier::Plain));
    };
    let numbers = suffix.strip_suffix('}').ok_or_else(|| format!("bad quantifier in {body}"))?;
    let parse_number =
        |value: &str| value.parse::<u64>().map_err(|_| format!("bad quantifier in {body}"));
    let bounds = if let Some((low, high)) = numbers.split_once(',') {
        let max = match high.is_empty() {
            true => None,
            false => Some(parse_number(high)?),
        };
        (parse_number(low)?, max)
    } else {
        let exact = parse_number(numbers)?;
        (exact, Some(exact))
    };
    let class = strip_brackets(core).ok_or_else(|| format!("bad class in {body}"))?;
    Ok((
        class,
        match bounds {
            (low, Some(high)) if low == high => Quantifier::Exact(low),
            (low, Some(high)) => Quantifier::Bounded(low, high),
            (low, None) => Quantifier::Bounded(low, u64::MAX),
        },
    ))
}

/// Splits `[class]{w}` where the group-star unit repeats `w` bytes each.
fn split_bracket_and_exact_repeat(body: &str) -> Result<(&str, usize), String> {
    let (class, quantifier) = split_bracket_and_quantifier(body)?;
    let Quantifier::Exact(units) = quantifier else {
        return Err(format!("group star requires an exact byte width, got {body}"));
    };
    Ok((class, units as usize))
}

fn strip_brackets(body: &str) -> Option<&str> {
    body.strip_prefix('[').and_then(|rest| rest.strip_suffix(']'))
}

fn parse_char_class(body: &str) -> Result<CharClass, String> {
    let mut class = CharClass::default();
    // A leading `^` negates the class, as in `[^/\\]`.
    let body = match body.strip_prefix('^') {
        Some(rest) => {
            class.negated = true;
            rest
        }
        None => body,
    };
    let mut chars = body.chars().peekable();
    while let Some(first) = chars.next() {
        // Inside a class, `\` escapes the next character (`[^/\\]`).
        let first = match first {
            '\\' => chars.next().ok_or_else(|| format!("dangling escape in class {body}"))?,
            other => other,
        };
        if chars.peek() == Some(&'-') {
            chars.next();
            let last = chars.next().ok_or_else(|| format!("dangling range in class {body}"))?;
            class.ranges.push((first as u8, last as u8));
        } else {
            class.ranges.push((first as u8, first as u8));
        }
    }
    Ok(class)
}

#[derive(Default)]
struct CharClass {
    ranges: Vec<(u8, u8)>,
    negated: bool,
}

impl CharClass {
    fn contains(&self, byte: u8) -> bool {
        let listed = self.ranges.iter().any(|(low, high)| *low <= byte && byte <= *high);
        listed != self.negated
    }
}
