//! `MutationStructuredValue.v1` — structured mutation data profile (#11327).
//!
//! Exact optional data/value/target/currentness contract for fresh unblessed
//! array/hash reference replacement. This module defines domain types, the
//! versioned text-envelope discriminator, bounded parsing, fresh-reference
//! semantics, and compatibility/result vocabulary only.
//!
//! It parses no DAP wire traffic, renders no Perl, performs no debugger I/O,
//! publishes no graph, changes no handler or capability, and proves no public
//! behavior.

use perl_source_identity::ContentDigest;
use serde::Serialize;

/// Current schema version for [`MutationStructuredValueV1`].
pub const MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION: u32 = 1;

/// Required byte-exact prefix admitting structured data on the scalar
/// mutation text surface (`MutationValueText.v1`). Bare Perl-looking text is
/// never interpreted as structure.
pub const STRUCTURED_PREFIX: &str = "json:";

/// Structural bytes charged per container node: its opening and closing
/// delimiters count against the aggregate budget like any other decoded
/// data byte.
const CONTAINER_DELIMITER_BYTES: usize = 2;

/// Pinned profile limits (#11327). Adjustable only through a reviewed
/// profile version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredMutationLimits {
    /// Maximum encoded input bytes.
    pub max_input_bytes: usize,
    /// Maximum decoded scalar/string bytes per scalar.
    pub max_scalar_bytes: usize,
    /// Maximum aggregate decoded bytes across the tree.
    pub max_aggregate_bytes: usize,
    /// Maximum nesting depth.
    pub max_depth: usize,
    /// Maximum total nodes.
    pub max_nodes: usize,
    /// Maximum entries per object/array.
    pub max_entries: usize,
    /// Maximum numeric significant digits.
    pub max_significant_digits: usize,
    /// Maximum absolute decimal exponent.
    pub max_absolute_exponent: u32,
}

impl Default for StructuredMutationLimits {
    fn default() -> Self {
        Self::PINNED_V1_MAXIMA
    }
}

impl StructuredMutationLimits {
    /// Pinned profile-v1 maxima (#11327). A caller-supplied profile may
    /// tighten any budget, but a schema-v1 admission can never be emitted
    /// under budgets wider than the reviewed profile: parse-time enforcement
    /// keeps the `schema_version == 1` wire identity meaning the reviewed
    /// bounds.
    pub const PINNED_V1_MAXIMA: Self = Self {
        max_input_bytes: 65_536,
        max_scalar_bytes: 32_768,
        max_aggregate_bytes: 65_536,
        max_depth: 16,
        max_nodes: 1_024,
        max_entries: 512,
        max_significant_digits: 256,
        max_absolute_exponent: 4_096,
    };

    /// Whether every budget stays at or below the pinned v1 maxima.
    fn within_pinned_v1(&self) -> bool {
        let pinned = &Self::PINNED_V1_MAXIMA;
        self.max_input_bytes <= pinned.max_input_bytes
            && self.max_scalar_bytes <= pinned.max_scalar_bytes
            && self.max_aggregate_bytes <= pinned.max_aggregate_bytes
            && self.max_depth <= pinned.max_depth
            && self.max_nodes <= pinned.max_nodes
            && self.max_entries <= pinned.max_entries
            && self.max_significant_digits <= pinned.max_significant_digits
            && self.max_absolute_exponent <= pinned.max_absolute_exponent
    }
}

/// Why a structured mutation value was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StructuredRefusal {
    /// The required `json:` prefix was absent (bare Perl-looking text).
    #[error("missing required json: prefix; bare expression text stays unsupported")]
    MissingStructuredPrefix,
    /// Input exceeded the encoded-byte budget.
    #[error("input exceeds the {limit}-byte input budget")]
    InputTooLarge {
        /// The configured budget.
        limit: usize,
    },
    /// Nesting exceeded the depth budget.
    #[error("nesting exceeds depth {limit}")]
    DepthExceeded {
        /// The configured budget.
        limit: usize,
    },
    /// Tree exceeded the total-node budget.
    #[error("tree exceeds {limit} nodes")]
    TooManyNodes {
        /// The configured budget.
        limit: usize,
    },
    /// Aggregate decoded size exceeded the budget.
    #[error("aggregate size exceeds {limit} bytes")]
    AggregateTooLarge {
        /// The configured budget.
        limit: usize,
    },
    /// A scalar/string exceeded its byte budget.
    #[error("scalar exceeds {limit} bytes")]
    ScalarTooLarge {
        /// The configured budget.
        limit: usize,
    },
    /// An object/array exceeded the entries-per-container budget.
    #[error("container exceeds {limit} entries")]
    TooManyEntries {
        /// The configured budget.
        limit: usize,
    },
    /// An object key appeared twice; last-wins is never silent.
    #[error("duplicate object key {key:?}")]
    DuplicateKey {
        /// The duplicated key.
        key: String,
    },
    /// Numeric literal exceeded the significant-digit budget.
    #[error("numeric literal exceeds {limit} significant digits")]
    TooManyDigits {
        /// The configured budget.
        limit: usize,
    },
    /// Numeric exponent exceeded the absolute-exponent budget.
    #[error("numeric exponent exceeds +/-{limit}")]
    ExponentTooLarge {
        /// The configured budget.
        limit: u32,
    },
    /// Integer literal outside the exact bounded range.
    #[error("integer literal out of the exact bounded range")]
    IntegerOutOfRange,
    /// Text was not valid structured data.
    #[error("invalid structured data at byte {offset}")]
    InvalidSyntax {
        /// Byte offset where parsing stopped.
        offset: usize,
    },
    /// Caller-supplied limits exceeded the pinned profile-v1 maxima; the
    /// schema-v1 identity may not be emitted under widened budgets.
    #[error("caller limits exceed the pinned profile v1 maxima")]
    LimitsExceedPinnedProfile,
}

/// One exact structured value. Numbers never pass through `f64`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum StructuredValue {
    /// Perl `undef`.
    Null,
    /// Defined numeric 1 or empty-string 0 semantics.
    Bool(bool),
    /// Exact bounded integer.
    Integer(i64),
    /// Exact bounded decimal/exponent kept in canonical text form.
    Decimal(ExactDecimal),
    /// Unicode string data.
    String(String),
    /// Finite ordered array of values.
    Array(Vec<StructuredValue>),
    /// Ordered object with unique string keys.
    Object(Vec<(String, StructuredValue)>),
}

/// Exact decimal/exponent number kept as canonical text (no binary float).
///
/// Construction is checked: only canonical JSON-number text is admitted, so
/// invalid forms cannot enter the exact value model outside this parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExactDecimal {
    /// Canonical form: optional `-`, digits with one `.`, optional `e±exp`.
    canonical: String,
}

impl ExactDecimal {
    /// Admit canonical JSON-number text (`-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?`).
    /// Returns `None` for any non-canonical spelling.
    pub fn admitted(canonical: &str) -> Option<Self> {
        if !is_canonical_number(canonical) {
            return None;
        }
        Some(Self { canonical: canonical.to_string() })
    }

    /// Canonical text form of this exact decimal.
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

/// Canonical JSON-number grammar: optional minus, integer part with the
/// leading-zero rule, optional fraction with at least one digit, optional
/// exponent with at least one digit.
fn is_canonical_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    if bytes.first() == Some(&b'-') {
        index += 1;
    }
    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == integer_start {
        return false;
    }
    if bytes[integer_start] == b'0' && index - integer_start > 1 {
        return false;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start || index != bytes.len() {
            return false;
        }
    }
    index == bytes.len()
}

/// Versioned envelope pairing the scalar mutation text with its parsed
/// structured payload.
///
/// Construction is sealed: the only producer is [`parse_structured_mutation`],
/// and the fields are readable through accessors, so a v1 envelope and its
/// digest can only describe admitted content and cannot be forged, widened,
/// or mutated after parsing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MutationStructuredValueV1 {
    /// Pinned schema version.
    schema_version: u32,
    /// The admitted structured value.
    value: StructuredValue,
    /// Deterministic fingerprint over the canonical serialization of
    /// [`Self::value`] (fixed variant/field order, documented entry order).
    fingerprint: ContentDigest,
}

impl MutationStructuredValueV1 {
    /// Pinned schema version of this envelope.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// The admitted structured value (read-only).
    pub const fn value(&self) -> &StructuredValue {
        &self.value
    }

    /// Fingerprint over the canonical serialization of the admitted value.
    pub const fn fingerprint(&self) -> &ContentDigest {
        &self.fingerprint
    }
}

/// Strip-and-check the versioned prefix on the scalar text surface.
pub fn structured_payload(raw: &str) -> Result<&str, StructuredRefusal> {
    raw.strip_prefix(STRUCTURED_PREFIX).ok_or(StructuredRefusal::MissingStructuredPrefix)
}

/// Parse one bounded structured value from prefixed text.
pub fn parse_structured_mutation(
    raw: &str,
    limits: &StructuredMutationLimits,
) -> Result<MutationStructuredValueV1, StructuredRefusal> {
    let payload = structured_payload(raw)?;
    if raw.len() > limits.max_input_bytes {
        return Err(StructuredRefusal::InputTooLarge { limit: limits.max_input_bytes });
    }
    if !limits.within_pinned_v1() {
        return Err(StructuredRefusal::LimitsExceedPinnedProfile);
    }
    let mut parser = Parser::new(payload.as_bytes(), limits);
    let value = parser.parse_value(0)?;
    parser.skip_ws();
    if parser.pos != parser.input.len() {
        return Err(StructuredRefusal::InvalidSyntax { offset: parser.pos });
    }
    let canonical = serde_json::to_string(&value)
        .map_err(|_| StructuredRefusal::InvalidSyntax { offset: 0 })?;
    let envelope = MutationStructuredValueV1 {
        schema_version: MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION,
        fingerprint: ContentDigest::of_bytes(canonical.as_bytes()),
        value,
    };
    Ok(envelope)
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    limits: &'a StructuredMutationLimits,
    nodes: usize,
    aggregate_bytes: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8], limits: &'a StructuredMutationLimits) -> Self {
        Self { input, pos: 0, limits, nodes: 0, aggregate_bytes: 0 }
    }

    fn skip_ws(&mut self) {
        while matches!(self.input.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn bump_node(&mut self, depth: usize) -> Result<(), StructuredRefusal> {
        if depth > self.limits.max_depth {
            return Err(StructuredRefusal::DepthExceeded { limit: self.limits.max_depth });
        }
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(StructuredRefusal::TooManyNodes { limit: self.limits.max_nodes });
        }
        Ok(())
    }

    fn charge_bytes(&mut self, count: usize) -> Result<(), StructuredRefusal> {
        self.aggregate_bytes = self.aggregate_bytes.saturating_add(count);
        if self.aggregate_bytes > self.limits.max_aggregate_bytes {
            return Err(StructuredRefusal::AggregateTooLarge {
                limit: self.limits.max_aggregate_bytes,
            });
        }
        Ok(())
    }

    fn parse_value(&mut self, depth: usize) -> Result<StructuredValue, StructuredRefusal> {
        self.bump_node(depth)?;
        self.skip_ws();
        match self.input.get(self.pos) {
            Some(b'n') => self.parse_literal(b"null", StructuredValue::Null),
            Some(b't') => self.parse_literal(b"true", StructuredValue::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", StructuredValue::Bool(false)),
            Some(b'"') => self.parse_string(),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => Err(StructuredRefusal::InvalidSyntax { offset: self.pos }),
        }
    }

    fn parse_literal(
        &mut self,
        word: &[u8],
        value: StructuredValue,
    ) -> Result<StructuredValue, StructuredRefusal> {
        if self.input[self.pos..].starts_with(word) {
            self.pos += word.len();
            self.charge_bytes(word.len())?;
            Ok(value)
        } else {
            Err(StructuredRefusal::InvalidSyntax { offset: self.pos })
        }
    }

    fn parse_string(&mut self) -> Result<StructuredValue, StructuredRefusal> {
        self.pos += 1; // opening quote
        let mut decoded = String::new();
        while let Some(&b) = self.input.get(self.pos) {
            match b {
                b'"' => {
                    let text = decoded;
                    self.pos += 1;
                    self.charge_bytes(text.len())?;
                    return Ok(StructuredValue::String(text));
                }
                b'\\' => {
                    let escape = *self
                        .input
                        .get(self.pos + 1)
                        .ok_or(StructuredRefusal::InvalidSyntax { offset: self.pos })?;
                    let (replacement, consumed) = match escape {
                        b'"' => ('"', 2),
                        b'\\' => ('\\', 2),
                        b'/' => ('/', 2),
                        b'b' => ('\u{0008}', 2),
                        b'f' => ('\u{000C}', 2),
                        b'n' => ('\n', 2),
                        b'r' => ('\r', 2),
                        b't' => ('\t', 2),
                        b'u' => self.decode_unicode_escape()?,
                        _ => {
                            return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
                        }
                    };
                    decoded.push(replacement);
                    self.pos += consumed;
                }
                _ => {
                    if b.is_ascii() {
                        // Raw control characters (U+0000–U+001F) are not
                        // legal inside a json: string; they must arrive via
                        // the escape branch above.
                        if b < 0x20 {
                            return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
                        }
                        decoded.push(b as char);
                        self.pos += 1;
                    } else {
                        let rest = std::str::from_utf8(&self.input[self.pos..])
                            .map_err(|_| StructuredRefusal::InvalidSyntax { offset: self.pos })?;
                        let c = rest
                            .chars()
                            .next()
                            .ok_or(StructuredRefusal::InvalidSyntax { offset: self.pos })?;
                        decoded.push(c);
                        self.pos += c.len_utf8();
                    }
                }
            }
            if decoded.len() > self.limits.max_scalar_bytes {
                return Err(StructuredRefusal::ScalarTooLarge {
                    limit: self.limits.max_scalar_bytes,
                });
            }
        }
        Err(StructuredRefusal::InvalidSyntax { offset: self.pos })
    }

    /// Decode one `\uXXXX` escape whose backslash sits at `self.pos`, returning
    /// the decoded scalar and the consumed byte count (`6`, or `12` for an
    /// exact surrogate pair). Strict JSON: every non-surrogate unit must be a
    /// valid scalar, a lone low surrogate refuses, and a high surrogate must be
    /// followed by an adjacent `\uDC00..=DFFF` continuation.
    ///
    /// Refusal offsets are payload-relative and name the escape that is
    /// actually malformed: a bad or truncated *continuation* `hex4` reports the
    /// continuation backslash (`backslash + 6`), while a continuation that is
    /// well-formed hex but not a low surrogate is a defect of the pair, so it
    /// reports the leading backslash.
    fn decode_unicode_escape(&self) -> Result<(char, usize), StructuredRefusal> {
        let backslash = self.pos;
        let refused_at = |offset: usize| StructuredRefusal::InvalidSyntax { offset };
        let unit = Self::hex4(self.input, backslash + 2, backslash)?;
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(refused_at(backslash));
        }
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.input.get(backslash + 6) != Some(&b'\\')
                || self.input.get(backslash + 7) != Some(&b'u')
            {
                return Err(refused_at(backslash));
            }
            let low = Self::hex4(self.input, backslash + 8, backslash + 6)?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(refused_at(backslash));
            }
            let combined = 0x1_0000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
            let scalar = char::from_u32(combined).ok_or_else(|| refused_at(backslash))?;
            return Ok((scalar, 12));
        }
        let scalar = char::from_u32(unit).ok_or_else(|| refused_at(backslash))?;
        Ok((scalar, 6))
    }

    /// Read exactly four hex digits starting at `start`, refusing at
    /// `error_offset` on any truncation or non-hex byte.
    fn hex4(input: &[u8], start: usize, error_offset: usize) -> Result<u32, StructuredRefusal> {
        let mut value = 0u32;
        for step in 0..4 {
            let byte = *input
                .get(start + step)
                .ok_or(StructuredRefusal::InvalidSyntax { offset: error_offset })?;
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => return Err(StructuredRefusal::InvalidSyntax { offset: error_offset }),
            };
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn parse_array(&mut self, depth: usize) -> Result<StructuredValue, StructuredRefusal> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.input.get(self.pos) == Some(&b']') {
            self.pos += 1;
            self.charge_bytes(CONTAINER_DELIMITER_BYTES)?;
            return Ok(StructuredValue::Array(items));
        }
        loop {
            if items.len() >= self.limits.max_entries {
                return Err(StructuredRefusal::TooManyEntries { limit: self.limits.max_entries });
            }
            items.push(self.parse_value(depth + 1)?);
            self.skip_ws();
            match self.input.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.charge_bytes(CONTAINER_DELIMITER_BYTES)?;
                    return Ok(StructuredValue::Array(items));
                }
                _ => return Err(StructuredRefusal::InvalidSyntax { offset: self.pos }),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<StructuredValue, StructuredRefusal> {
        self.pos += 1;
        let mut entries: Vec<(String, StructuredValue)> = Vec::new();
        self.skip_ws();
        if self.input.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            self.charge_bytes(CONTAINER_DELIMITER_BYTES)?;
            return Ok(StructuredValue::Object(entries));
        }
        loop {
            if entries.len() >= self.limits.max_entries {
                return Err(StructuredRefusal::TooManyEntries { limit: self.limits.max_entries });
            }
            self.skip_ws();
            let key = match self.parse_string()? {
                StructuredValue::String(key) => key,
                _ => return Err(StructuredRefusal::InvalidSyntax { offset: self.pos }),
            };
            if entries.iter().any(|(existing, _)| existing == &key) {
                return Err(StructuredRefusal::DuplicateKey { key });
            }
            self.skip_ws();
            if self.input.get(self.pos) != Some(&b':') {
                return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
            }
            self.pos += 1;
            let value = self.parse_value(depth + 1)?;
            entries.push((key, value));
            self.skip_ws();
            match self.input.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.charge_bytes(CONTAINER_DELIMITER_BYTES)?;
                    return Ok(StructuredValue::Object(entries));
                }
                _ => return Err(StructuredRefusal::InvalidSyntax { offset: self.pos }),
            }
        }
    }

    fn parse_number(&mut self) -> Result<StructuredValue, StructuredRefusal> {
        let start = self.pos;
        if self.input.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        // Integer component: at least one digit, and a leading zero may not
        // be followed by further integer digits (JSON grammar).
        let integer_start = self.pos;
        if matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
            if self.input[integer_start] == b'0' {
                if matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
                    return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
                }
            } else {
                while matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        } else {
            return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
        }
        let mut is_decimal = false;
        if self.input.get(self.pos) == Some(&b'.') {
            is_decimal = true;
            self.pos += 1;
            if !matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
                return Err(StructuredRefusal::InvalidSyntax { offset: self.pos });
            }
            while matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let exponent;
        if matches!(self.input.get(self.pos), Some(b'e' | b'E')) {
            is_decimal = true;
            self.pos += 1;
            let negative = if matches!(self.input.get(self.pos), Some(b'-')) {
                self.pos += 1;
                true
            } else {
                if self.input.get(self.pos) == Some(&b'+') {
                    self.pos += 1;
                }
                false
            };
            let exp_start = self.pos;
            while matches!(self.input.get(self.pos), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
            let digits = &self.input[exp_start..self.pos];
            if digits.is_empty() {
                return Err(StructuredRefusal::InvalidSyntax { offset: exp_start });
            }
            // Checked accumulation: an oversized exponent must refuse as
            // ExponentTooLarge, never overflow-panic or wrap past the limit.
            let mut magnitude = Some(0i64);
            for &d in digits {
                magnitude = magnitude
                    .and_then(|acc| acc.checked_mul(10))
                    .and_then(|acc| acc.checked_add(i64::from(d - b'0')));
            }
            let magnitude = magnitude.ok_or(StructuredRefusal::ExponentTooLarge {
                limit: self.limits.max_absolute_exponent,
            })?;
            exponent = if negative { -magnitude } else { magnitude };
            if exponent.unsigned_abs() > u64::from(self.limits.max_absolute_exponent) {
                return Err(StructuredRefusal::ExponentTooLarge {
                    limit: self.limits.max_absolute_exponent,
                });
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| StructuredRefusal::InvalidSyntax { offset: start })?;
        if !is_decimal {
            if let Ok(integer) = text.parse::<i64>() {
                self.charge_bytes(text.len())?;
                return Ok(StructuredValue::Integer(integer));
            }
            return Err(StructuredRefusal::IntegerOutOfRange);
        }
        let significant = text.chars().filter(|c| c.is_ascii_digit()).count();
        if significant > self.limits.max_significant_digits {
            return Err(StructuredRefusal::TooManyDigits {
                limit: self.limits.max_significant_digits,
            });
        }
        self.charge_bytes(text.len())?;
        Ok(StructuredValue::Decimal(ExactDecimal { canonical: text.to_string() }))
    }
}

/// Fresh referent kind created by an accepted structured value (#11327).
///
/// `StructuredArray` creates one fresh unblessed ARRAY reference and
/// `StructuredObject` one fresh unblessed HASH reference; scalars admit no
/// fresh referent of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FreshReferentKind {
    /// Fresh unblessed ARRAY reference.
    Array,
    /// Fresh unblessed HASH reference.
    Hash,
}

/// Which fresh referent an accepted value would create, if any.
#[must_use]
pub fn fresh_referent_kind(value: &StructuredValue) -> Option<FreshReferentKind> {
    match value {
        StructuredValue::Array(_) => Some(FreshReferentKind::Array),
        StructuredValue::Object(_) => Some(FreshReferentKind::Hash),
        _ => None,
    }
}
