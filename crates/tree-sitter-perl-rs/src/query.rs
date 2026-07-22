//! Structural query matching for the native AST.
//!
//! This module intentionally implements a bounded query subset only: node kinds,
//! wildcards, nested children, named fields, captures, predicates, multiple
//! top-level patterns, and byte-range restriction. Other tree-sitter query
//! language features return QueryError instead of being silently ignored.

use crate::Node;
use regex::Regex;
use std::error::Error;
use std::fmt;
use std::ops::Range;

/// A compiled structural query pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Query {
    source: String,
    patterns: Vec<NodePattern>,
}

impl Query {
    /// Compile a supported structural query string.
    pub fn new(source: &str) -> Result<Self, QueryError> {
        let tokens = lex(source)?;
        if tokens.is_empty() {
            return Err(QueryError::Empty);
        }

        let mut position = 0;
        let mut patterns = Vec::new();
        while position < tokens.len() {
            patterns.push(parse_top_level_pattern(&tokens, &mut position)?);
        }
        validate_predicates(&patterns)?;

        Ok(Self { source: source.to_owned(), patterns })
    }

    /// Returns the original query source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the number of top-level patterns in this query.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

/// A typed error for unsupported or malformed query syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryError {
    /// The query contained no patterns.
    Empty,
    /// The query ended before a complete pattern was read.
    UnexpectedEnd,
    /// A token did not fit the supported structural grammar.
    UnexpectedToken {
        /// Expected token description.
        expected: String,
        /// Token that was encountered.
        found: String,
    },
    /// A query language feature outside Phase 2a was requested.
    UnsupportedSyntax {
        /// Unsupported source fragment.
        syntax: String,
    },
    /// A capture name was empty or not a valid capture identifier.
    InvalidCaptureName {
        /// Invalid capture name.
        name: String,
    },
    /// A supported predicate had the wrong number or kind of arguments.
    InvalidPredicateArguments {
        /// Predicate name.
        name: String,
    },
    /// A supported regex predicate contained an invalid pattern.
    InvalidPredicatePattern {
        /// Predicate name.
        name: String,
        /// Invalid regex source.
        pattern: String,
    },
    /// A predicate referenced a capture not present in its pattern.
    MissingPredicateCapture {
        /// Predicate name.
        name: String,
        /// Referenced capture name.
        capture: String,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("query contains no patterns"),
            Self::UnexpectedEnd => formatter.write_str("query ended before a complete pattern"),
            Self::UnexpectedToken { expected, found } => {
                write!(formatter, "expected {expected}, found {found:?}")
            }
            Self::UnsupportedSyntax { syntax } => {
                write!(formatter, "unsupported query syntax: {syntax:?}")
            }
            Self::InvalidCaptureName { name } => {
                write!(formatter, "invalid query capture name: {name:?}")
            }
            Self::InvalidPredicateArguments { name } => {
                write!(formatter, "invalid arguments for query predicate: {name:?}")
            }
            Self::InvalidPredicatePattern { name, pattern } => {
                write!(formatter, "invalid regex for query predicate {name:?}: {pattern:?}")
            }
            Self::MissingPredicateCapture { name, capture } => {
                write!(formatter, "query predicate {name:?} references missing capture {capture:?}")
            }
        }
    }
}

impl Error for QueryError {}

/// A cursor that executes compiled queries over a native syntax tree.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct QueryCursor {
    byte_range: Option<Range<usize>>,
}

impl QueryCursor {
    /// Create a cursor with no byte-range restriction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict matches to nodes overlapping the supplied range.
    pub fn set_byte_range(&mut self, range: Range<usize>) {
        self.byte_range = Some(range);
    }

    /// Remove any byte-range restriction.
    pub fn clear_byte_range(&mut self) {
        self.byte_range = None;
    }

    /// Return the active byte-range restriction, if any.
    pub fn byte_range(&self) -> Option<Range<usize>> {
        self.byte_range.clone()
    }

    /// Execute a query in pre-order and return every matching pattern.
    pub fn matches<'tree>(&mut self, query: &Query, root: Node<'tree>) -> QueryMatches<'tree> {
        let mut matches = Vec::new();
        collect_matches(root, query, self.byte_range.as_ref(), &mut matches);
        QueryMatches { inner: matches.into_iter() }
    }
}

/// Iterator returned by QueryCursor::matches.
#[non_exhaustive]
pub struct QueryMatches<'tree> {
    inner: std::vec::IntoIter<QueryMatch<'tree>>,
}

impl<'tree> Iterator for QueryMatches<'tree> {
    type Item = QueryMatch<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// One successful top-level query pattern match.
#[non_exhaustive]
pub struct QueryMatch<'tree> {
    pattern_index: usize,
    captures: Vec<QueryCapture<'tree>>,
    settings: Vec<QuerySetting>,
}

impl<'tree> QueryMatch<'tree> {
    /// Return the zero-based top-level pattern index that matched.
    pub fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    /// Return captures in structural traversal order.
    pub fn captures(&self) -> &[QueryCapture<'tree>] {
        &self.captures
    }

    /// Return `#set!` metadata attached to this match.
    pub fn settings(&self) -> &[QuerySetting] {
        &self.settings
    }
}

/// Metadata emitted by a `#set! key "value"` query directive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct QuerySetting {
    /// Setting key, such as `injection.language`.
    pub key: String,
    /// Setting value, such as `comment` or `perl`.
    pub value: String,
}

/// A named node captured by a query match.
#[non_exhaustive]
pub struct QueryCapture<'tree> {
    name: String,
    node: Node<'tree>,
}

impl<'tree> QueryCapture<'tree> {
    /// Return the capture name without its leading at-sign.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the captured node.
    pub fn node(&self) -> &Node<'tree> {
        &self.node
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Atom(String),
    Capture(String),
    Predicate(String),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodePattern {
    kind: NodeKindPattern,
    children: Vec<ChildPattern>,
    capture: Option<String>,
    predicates: Vec<PredicatePattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildPattern {
    field: Option<String>,
    pattern: NodePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeKindPattern {
    Any,
    Named(String),
}

#[derive(Debug, Clone)]
enum PredicatePattern {
    Eq { capture: String, value: String, negated: bool },
    Match { capture: String, pattern: String, regex: Regex, negated: bool },
    Set { key: String, value: String },
}

impl PartialEq for PredicatePattern {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Eq { capture: left_capture, value: left_value, negated: left_negated },
                Self::Eq { capture: right_capture, value: right_value, negated: right_negated },
            ) => {
                left_capture == right_capture
                    && left_value == right_value
                    && left_negated == right_negated
            }
            (
                Self::Match {
                    capture: left_capture,
                    pattern: left_pattern,
                    negated: left_negated,
                    ..
                },
                Self::Match {
                    capture: right_capture,
                    pattern: right_pattern,
                    negated: right_negated,
                    ..
                },
            ) => {
                left_capture == right_capture
                    && left_pattern == right_pattern
                    && left_negated == right_negated
            }
            (
                Self::Set { key: left_key, value: left_value },
                Self::Set { key: right_key, value: right_value },
            ) => left_key == right_key && left_value == right_value,
            _ => false,
        }
    }
}

impl Eq for PredicatePattern {}

fn lex(source: &str) -> Result<Vec<Token>, QueryError> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();

    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        match ch {
            ';' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '@' => {
                chars.next();
                let mut name = String::new();
                while let Some(next) = chars.peek().copied() {
                    if next.is_whitespace() || matches!(next, '(' | ')') {
                        break;
                    }
                    name.push(next);
                    chars.next();
                }
                if !is_valid_capture_name(&name) {
                    return Err(QueryError::InvalidCaptureName { name });
                }
                tokens.push(Token::Capture(name));
            }
            '#' => {
                let mut predicate = String::new();
                while let Some(next) = chars.peek().copied() {
                    if next.is_whitespace() || matches!(next, '(' | ')') {
                        break;
                    }
                    predicate.push(next);
                    chars.next();
                }
                tokens.push(Token::Predicate(predicate));
            }
            '"' => {
                chars.next();
                let mut value = String::new();
                let mut closed = false;
                while let Some(next) = chars.next() {
                    match next {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            let Some(escaped) = chars.next() else {
                                return Err(QueryError::UnexpectedEnd);
                            };
                            value.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                other => other,
                            });
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    return Err(QueryError::UnexpectedEnd);
                }
                tokens.push(Token::String(value));
            }
            _ => {
                let mut atom = String::new();
                while let Some(next) = chars.peek().copied() {
                    if next.is_whitespace() || matches!(next, '(' | ')') {
                        break;
                    }
                    atom.push(next);
                    chars.next();
                }
                if atom.is_empty() {
                    return Err(QueryError::UnexpectedToken {
                        expected: "query token".to_string(),
                        found: ch.to_string(),
                    });
                }
                tokens.push(Token::Atom(atom));
            }
        }
    }

    Ok(tokens)
}

fn is_supported_atom(atom: &str) -> bool {
    if atom.is_empty() || atom.starts_with('.') {
        return false;
    }

    let is_operator_kind = atom.starts_with("binary_") || atom.starts_with("unary_");
    is_operator_kind
        || !atom.chars().any(|ch| {
            matches!(
                ch,
                '#' | '"' | '.' | '*' | '+' | '?' | '|' | '!' | '&' | '[' | ']' | '{' | '}' | '='
            )
        })
}

fn parse_top_level_pattern(
    tokens: &[Token],
    position: &mut usize,
) -> Result<NodePattern, QueryError> {
    if matches!(tokens.get(*position), Some(Token::Open))
        && matches!(tokens.get(position.saturating_add(1)), Some(Token::Open))
    {
        *position += 1;
        let pattern = parse_node(tokens, position)?;
        expect_token(tokens, position, Token::Close)?;
        Ok(pattern)
    } else {
        parse_node(tokens, position)
    }
}

fn parse_node(tokens: &[Token], position: &mut usize) -> Result<NodePattern, QueryError> {
    expect_token(tokens, position, Token::Open)?;
    let kind = match tokens.get(*position) {
        Some(Token::Atom(name)) if name == "_" => {
            *position += 1;
            NodeKindPattern::Any
        }
        Some(Token::Atom(name)) => {
            if !is_supported_atom(name) {
                return Err(QueryError::UnsupportedSyntax { syntax: name.clone() });
            }
            let name = name.clone();
            *position += 1;
            NodeKindPattern::Named(name)
        }
        Some(token) => {
            return Err(QueryError::UnexpectedToken {
                expected: "node kind or _".to_string(),
                found: format!("{token:?}"),
            });
        }
        None => return Err(QueryError::UnexpectedEnd),
    };

    let mut children = Vec::new();
    while !matches!(tokens.get(*position), Some(Token::Close)) {
        if *position >= tokens.len() {
            return Err(QueryError::UnexpectedEnd);
        }

        let field = match tokens.get(*position) {
            Some(Token::Atom(name)) if name.ends_with(':') => {
                let field = name.trim_end_matches(':');
                if !is_supported_atom(field) {
                    return Err(QueryError::UnsupportedSyntax { syntax: name.clone() });
                }
                *position += 1;
                Some(field.to_string())
            }
            Some(Token::Open) => None,
            Some(Token::Atom(_)) => None,
            Some(Token::Capture(_)) => {
                return Err(QueryError::UnexpectedToken {
                    expected: "child pattern or closing parenthesis".to_string(),
                    found: "capture".to_string(),
                });
            }
            Some(Token::Predicate(name)) => {
                return Err(QueryError::UnsupportedSyntax { syntax: name.clone() });
            }
            Some(Token::String(_)) => {
                return Err(QueryError::UnexpectedToken {
                    expected: "child pattern or closing parenthesis".to_string(),
                    found: "string".to_string(),
                });
            }
            Some(Token::Close) | None => None,
        };

        let pattern = match tokens.get(*position) {
            Some(Token::Open) => parse_node(tokens, position)?,
            Some(Token::Atom(_)) => parse_atom_pattern(tokens, position)?,
            Some(token) => {
                return Err(QueryError::UnexpectedToken {
                    expected: "child pattern".to_string(),
                    found: format!("{token:?}"),
                });
            }
            None => return Err(QueryError::UnexpectedEnd),
        };
        children.push(ChildPattern { field, pattern });
    }

    expect_token(tokens, position, Token::Close)?;
    let capture = take_capture(tokens, position)?;
    let predicates = take_predicates(tokens, position)?;
    Ok(NodePattern { kind, children, capture, predicates })
}

fn parse_atom_pattern(tokens: &[Token], position: &mut usize) -> Result<NodePattern, QueryError> {
    let kind = match tokens.get(*position) {
        Some(Token::Atom(name)) if name == "_" => {
            *position += 1;
            NodeKindPattern::Any
        }
        Some(Token::Atom(name)) => {
            if !is_supported_atom(name) {
                return Err(QueryError::UnsupportedSyntax { syntax: name.clone() });
            }
            let name = name.clone();
            *position += 1;
            NodeKindPattern::Named(name)
        }
        Some(token) => {
            return Err(QueryError::UnexpectedToken {
                expected: "node kind or _".to_string(),
                found: format!("{token:?}"),
            });
        }
        None => return Err(QueryError::UnexpectedEnd),
    };
    let capture = take_capture(tokens, position)?;
    let predicates = take_predicates(tokens, position)?;
    Ok(NodePattern { kind, children: Vec::new(), capture, predicates })
}

fn take_predicates(
    tokens: &[Token],
    position: &mut usize,
) -> Result<Vec<PredicatePattern>, QueryError> {
    let mut predicates = Vec::new();
    while matches!(tokens.get(*position), Some(Token::Open))
        && matches!(tokens.get(position.saturating_add(1)), Some(Token::Predicate(_)))
    {
        *position += 1;
        let Some(Token::Predicate(name)) = tokens.get(*position) else {
            return Err(QueryError::UnexpectedEnd);
        };
        let name = name.clone();
        *position += 1;
        if !matches!(name.as_str(), "#eq?" | "#not-eq?" | "#match?" | "#not-match?" | "#set!") {
            return Err(QueryError::UnsupportedSyntax { syntax: name });
        }

        if name == "#set!" {
            let key = match tokens.get(*position) {
                Some(Token::Atom(key)) if !key.is_empty() => {
                    *position += 1;
                    key.clone()
                }
                _ => return Err(QueryError::InvalidPredicateArguments { name }),
            };
            let value = match tokens.get(*position) {
                Some(Token::String(value)) => {
                    *position += 1;
                    value.clone()
                }
                _ => return Err(QueryError::InvalidPredicateArguments { name }),
            };
            expect_token(tokens, position, Token::Close)?;
            predicates.push(PredicatePattern::Set { key, value });
            continue;
        }

        let capture = match tokens.get(*position) {
            Some(Token::Capture(capture)) => {
                *position += 1;
                capture.clone()
            }
            _ => return Err(QueryError::InvalidPredicateArguments { name }),
        };
        let value = match tokens.get(*position) {
            Some(Token::String(value)) => {
                *position += 1;
                value.clone()
            }
            _ => return Err(QueryError::InvalidPredicateArguments { name }),
        };
        expect_token(tokens, position, Token::Close)?;
        let predicate = match name.as_str() {
            "#eq?" => PredicatePattern::Eq { capture, value, negated: false },
            "#not-eq?" => PredicatePattern::Eq { capture, value, negated: true },
            "#match?" | "#not-match?" => {
                let regex =
                    Regex::new(&value).map_err(|_| QueryError::InvalidPredicatePattern {
                        name: name.clone(),
                        pattern: value.clone(),
                    })?;
                PredicatePattern::Match {
                    capture,
                    pattern: value,
                    regex,
                    negated: name == "#not-match?",
                }
            }
            _ => return Err(QueryError::UnsupportedSyntax { syntax: name }),
        };
        predicates.push(predicate);
    }
    Ok(predicates)
}

fn validate_predicates(patterns: &[NodePattern]) -> Result<(), QueryError> {
    for pattern in patterns {
        for predicate in &pattern.predicates {
            let (name, capture) = match predicate {
                PredicatePattern::Eq { capture, negated, .. } => {
                    (if *negated { "#not-eq?" } else { "#eq?" }, capture)
                }
                PredicatePattern::Match { capture, negated, .. } => {
                    (if *negated { "#not-match?" } else { "#match?" }, capture)
                }
                PredicatePattern::Set { .. } => continue,
            };
            if !pattern_has_capture(pattern, capture) {
                return Err(QueryError::MissingPredicateCapture {
                    name: name.to_string(),
                    capture: capture.clone(),
                });
            }
        }
        for child in &pattern.children {
            validate_predicates(std::slice::from_ref(&child.pattern))?;
        }
    }
    Ok(())
}

fn pattern_has_capture(pattern: &NodePattern, capture: &str) -> bool {
    pattern.capture.as_deref() == Some(capture)
        || pattern.children.iter().any(|child| pattern_has_capture(&child.pattern, capture))
}

fn take_capture(tokens: &[Token], position: &mut usize) -> Result<Option<String>, QueryError> {
    match tokens.get(*position) {
        Some(Token::Capture(name)) => {
            let name = name.clone();
            *position += 1;
            Ok(Some(name))
        }
        _ => Ok(None),
    }
}

fn expect_token(tokens: &[Token], position: &mut usize, expected: Token) -> Result<(), QueryError> {
    match tokens.get(*position) {
        Some(actual) if *actual == expected => {
            *position += 1;
            Ok(())
        }
        Some(actual) => Err(QueryError::UnexpectedToken {
            expected: format!("{expected:?}"),
            found: format!("{actual:?}"),
        }),
        None => Err(QueryError::UnexpectedEnd),
    }
}

fn is_valid_capture_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch == '-' || ch == '.' || ch.is_ascii_alphanumeric())
}

fn collect_matches<'tree>(
    node: Node<'tree>,
    query: &Query,
    byte_range: Option<&Range<usize>>,
    output: &mut Vec<QueryMatch<'tree>>,
) {
    let overlaps_range = byte_range
        .is_none_or(|range| node.start_byte() < range.end && node.end_byte() > range.start);
    if !overlaps_range {
        return;
    }

    for (pattern_index, pattern) in query.patterns.iter().enumerate() {
        let mut captures = Vec::new();
        let mut settings = Vec::new();
        if matches_pattern(pattern, node, &mut captures, &mut settings) {
            output.push(QueryMatch { pattern_index, captures, settings });
        }
    }

    for child in node.children() {
        collect_matches(child, query, byte_range, output);
    }
}

fn matches_pattern<'tree>(
    pattern: &NodePattern,
    node: Node<'tree>,
    captures: &mut Vec<QueryCapture<'tree>>,
    settings: &mut Vec<QuerySetting>,
) -> bool {
    let kind_matches = match &pattern.kind {
        NodeKindPattern::Any => true,
        NodeKindPattern::Named(expected) => node.kind() == *expected,
    };
    if !kind_matches {
        return false;
    }

    let mut last_child_index: Option<usize> = None;
    for child_pattern in &pattern.children {
        let mut child_match = false;
        let first_index = last_child_index.map_or(0, |index| index.saturating_add(1));
        for index in first_index..node.child_count() {
            let Some(child) = node.child(index) else {
                continue;
            };
            if let Some(expected_field) = child_pattern.field.as_deref() {
                if node.field_name_for_child(index) != Some(expected_field) {
                    continue;
                }
            }

            let mut nested_captures = Vec::new();
            let mut nested_settings = Vec::new();
            if matches_pattern(
                &child_pattern.pattern,
                child,
                &mut nested_captures,
                &mut nested_settings,
            ) {
                captures.extend(nested_captures);
                settings.extend(nested_settings);
                child_match = true;
                last_child_index = Some(index);
                break;
            }
        }
        if !child_match {
            return false;
        }
    }

    if let Some(name) = &pattern.capture {
        captures.push(QueryCapture { name: name.clone(), node });
    }

    pattern.predicates.iter().all(|predicate| predicate_matches(predicate, captures, settings))
}

fn predicate_matches(
    predicate: &PredicatePattern,
    captures: &[QueryCapture<'_>],
    settings: &mut Vec<QuerySetting>,
) -> bool {
    if let PredicatePattern::Set { key, value } = predicate {
        settings.push(QuerySetting { key: key.clone(), value: value.clone() });
        return true;
    }

    let (capture_name, negated) = match predicate {
        PredicatePattern::Eq { capture, negated, .. }
        | PredicatePattern::Match { capture, negated, .. } => (capture, *negated),
        PredicatePattern::Set { .. } => return true,
    };

    let matches_capture = |capture: &QueryCapture<'_>| {
        let Ok(text) = capture.node.utf8_text(capture.node.tree_source().as_bytes()) else {
            return false;
        };
        match predicate {
            PredicatePattern::Eq { value, .. } => text == value,
            PredicatePattern::Match { regex, .. } => regex.is_match(text),
            PredicatePattern::Set { .. } => true,
        }
    };
    let mut matching =
        captures.iter().filter(|capture| capture.name == *capture_name).map(matches_capture);
    if negated { matching.all(|matches| !matches) } else { matching.any(|matches| matches) }
}
