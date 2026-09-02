//! Owner-neutral source syntax for declaration attributes.
//!
//! These values preserve the source proposition without assigning meaning to
//! an attribute. Class, field, and other declaration families may interpret
//! the same syntax downstream.

use crate::SourceLocation;
use std::fmt;

/// The source separator that introduced a declaration attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationAttributeSeparator {
    /// An explicit colon, as in `:reader`.
    Colon {
        /// The byte range occupied by the colon.
        range: SourceLocation,
    },
    /// A reviewed continuation form whose separator is whitespace rather than
    /// a colon. The range covers the source separator retained by the parser.
    WhitespaceContinuation {
        /// The byte range occupied by the separator.
        range: SourceLocation,
    },
}

impl DeclarationAttributeSeparator {
    /// Return the source range occupied by this separator.
    #[must_use]
    pub const fn range(self) -> SourceLocation {
        match self {
            Self::Colon { range } | Self::WhitespaceContinuation { range } => range,
        }
    }
}

/// The disposition of an attribute argument's source payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationAttributeArgumentDisposition {
    /// The argument and its delimiters were recovered exactly.
    Exact,
    /// Delimiters were present but the argument body was empty.
    Empty,
    /// The parser retained a partial argument during recovery.
    Recovered,
    /// An argument was syntactically indicated but its source payload was not
    /// available to this value contract.
    Unavailable,
}

/// Delimiter identity retained for a declaration attribute argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationAttributeDelimiter {
    /// Parenthesized argument, such as `:foo(bar)`.
    Parentheses {
        /// Range of the opening delimiter.
        opening: SourceLocation,
        /// Range of the closing delimiter.
        closing: SourceLocation,
    },
    /// Bracketed argument, such as `:foo[bar]`.
    Brackets {
        /// Range of the opening delimiter.
        opening: SourceLocation,
        /// Range of the closing delimiter.
        closing: SourceLocation,
    },
    /// Braced argument, such as `:foo{bar}`.
    Braces {
        /// Range of the opening delimiter.
        opening: SourceLocation,
        /// Range of the closing delimiter.
        closing: SourceLocation,
    },
}

impl DeclarationAttributeDelimiter {
    fn opening(self) -> SourceLocation {
        match self {
            Self::Parentheses { opening, .. }
            | Self::Brackets { opening, .. }
            | Self::Braces { opening, .. } => opening,
        }
    }

    fn closing(self) -> SourceLocation {
        match self {
            Self::Parentheses { closing, .. }
            | Self::Brackets { closing, .. }
            | Self::Braces { closing, .. } => closing,
        }
    }
}

/// Source-preserving argument information for one declaration attribute.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarationAttributeArgumentSyntax {
    /// Full source range of the argument, including delimiters when present.
    pub range: SourceLocation,
    /// Source range of the argument body, excluding delimiters.
    pub body_range: SourceLocation,
    /// Delimiter identity, when the parser retained it.
    pub delimiters: Option<DeclarationAttributeDelimiter>,
    /// Whether the retained argument is exact, empty, recovered, or unavailable.
    pub disposition: DeclarationAttributeArgumentDisposition,
}

/// Completeness of the attribute's source representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationAttributeCompleteness {
    /// Every represented source component is exact.
    Exact,
    /// At least one represented source component was recovered.
    Recovered,
}

/// One owner-neutral declaration-attribute source proposition.
///
/// This type preserves source order, duplicate entries, spelling, geometry,
/// and local recovery. It deliberately does not interpret the attribute name
/// or evaluate its argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarationAttributeSyntax {
    /// The source separator/introducer identity.
    separator: DeclarationAttributeSeparator,
    /// Attribute name exactly as represented by the source parser.
    name: String,
    /// Byte range of the attribute name.
    name_range: SourceLocation,
    /// Optional argument; `None` means that no argument was present.
    argument: Option<DeclarationAttributeArgumentSyntax>,
    /// Full source range of the attribute.
    range: SourceLocation,
    /// Whether this attribute is exact or recovery-derived.
    completeness: DeclarationAttributeCompleteness,
}

/// Construction failures for [`DeclarationAttributeSyntax`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationAttributeSyntaxError {
    /// A range has its end before its start.
    InvalidRange,
    /// A child range is not contained by its parent range.
    RangeOutsideParent,
    /// A required source name is empty.
    EmptyName,
    /// Exactness and recovery dispositions contradict one another.
    ContradictoryCompleteness,
    /// An exact or empty argument lacks exact delimiters.
    MissingExactDelimiters,
    /// Delimiters are not ordered or contained by the argument range.
    InvalidDelimiters,
    /// A source separator or exact attribute range has invalid geometry.
    InvalidAttributeGeometry,
}

impl fmt::Display for DeclarationAttributeSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRange => "source range has end before start",
            Self::RangeOutsideParent => "source range is outside its parent",
            Self::EmptyName => "attribute name is empty",
            Self::ContradictoryCompleteness => "exact attribute contains recovered syntax",
            Self::MissingExactDelimiters => "exact argument lacks exact delimiters",
            Self::InvalidDelimiters => "argument delimiters are invalid",
            Self::InvalidAttributeGeometry => "attribute source geometry is invalid",
        };
        f.write_str(message)
    }
}

impl std::error::Error for DeclarationAttributeSyntaxError {}

impl DeclarationAttributeSyntax {
    /// Return the source separator/introducer identity.
    #[must_use]
    pub const fn separator(&self) -> DeclarationAttributeSeparator {
        self.separator
    }

    /// Return the attribute name exactly as represented by the parser.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the byte range of the attribute name.
    #[must_use]
    pub const fn name_range(&self) -> SourceLocation {
        self.name_range
    }

    /// Return the optional source argument.
    #[must_use]
    pub const fn argument(&self) -> Option<&DeclarationAttributeArgumentSyntax> {
        self.argument.as_ref()
    }

    /// Return the full source range of the attribute.
    #[must_use]
    pub const fn range(&self) -> SourceLocation {
        self.range
    }

    /// Return whether this attribute is exact or recovery-derived.
    #[must_use]
    pub const fn completeness(&self) -> DeclarationAttributeCompleteness {
        self.completeness
    }

    /// Construct and validate an owner-neutral source attribute.
    ///
    /// For [`DeclarationAttributeCompleteness::Exact`] the outer `range` is
    /// pinned tight to the source components: it must start at the separator
    /// start and end at the argument end (or the name end when no argument is
    /// present). [`DeclarationAttributeCompleteness::Recovered`] permits the
    /// outer range to extend past the last component to cover recovered
    /// source.
    pub fn new(
        separator: DeclarationAttributeSeparator,
        name: String,
        name_range: SourceLocation,
        argument: Option<DeclarationAttributeArgumentSyntax>,
        range: SourceLocation,
        completeness: DeclarationAttributeCompleteness,
    ) -> Result<Self, DeclarationAttributeSyntaxError> {
        validate_range(range)?;
        validate_range(name_range)?;
        if name.is_empty() {
            return Err(DeclarationAttributeSyntaxError::EmptyName);
        }
        if name_range.end() - name_range.start() != name.len() {
            return Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry);
        }
        validate_range(separator.range())?;
        if matches!(separator, DeclarationAttributeSeparator::Colon { .. })
            && separator.range().end() - separator.range().start() != 1
        {
            return Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry);
        }
        if matches!(separator, DeclarationAttributeSeparator::WhitespaceContinuation { .. })
            && separator.range().is_empty()
        {
            return Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry);
        }
        if !contains(range, separator.range())
            || !contains(range, name_range)
            || separator.range().end() > name_range.start()
        {
            return Err(DeclarationAttributeSyntaxError::RangeOutsideParent);
        }

        if let Some(argument) = &argument {
            validate_argument(argument, range, completeness)?;
            if argument.range.start() < name_range.end() {
                return Err(DeclarationAttributeSyntaxError::RangeOutsideParent);
            }
            if completeness == DeclarationAttributeCompleteness::Exact
                && (range.start() != separator.range().start()
                    || range.end() != argument.range.end())
            {
                return Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry);
            }
        } else if completeness == DeclarationAttributeCompleteness::Exact
            && (range.start() != separator.range().start() || range.end() != name_range.end())
        {
            return Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry);
        }

        Ok(Self { separator, name, name_range, argument, range, completeness })
    }
}

fn validate_argument(
    argument: &DeclarationAttributeArgumentSyntax,
    attribute_range: SourceLocation,
    completeness: DeclarationAttributeCompleteness,
) -> Result<(), DeclarationAttributeSyntaxError> {
    validate_range(argument.range)?;
    validate_range(argument.body_range)?;
    if !contains(attribute_range, argument.range) || !contains(argument.range, argument.body_range)
    {
        return Err(DeclarationAttributeSyntaxError::RangeOutsideParent);
    }

    let has_recovery = matches!(
        argument.disposition,
        DeclarationAttributeArgumentDisposition::Recovered
            | DeclarationAttributeArgumentDisposition::Unavailable
    );
    if completeness == DeclarationAttributeCompleteness::Exact && has_recovery {
        return Err(DeclarationAttributeSyntaxError::ContradictoryCompleteness);
    }

    let delimiters = argument.delimiters;
    if let Some(delimiters) = delimiters {
        let opening = delimiters.opening();
        let closing = delimiters.closing();
        validate_range(opening)?;
        validate_range(closing)?;
        if opening.is_empty()
            || closing.is_empty()
            || opening.end() != opening.start() + 1
            || closing.end() != closing.start() + 1
            || opening.end() > closing.start()
            || !contains(argument.range, opening)
            || !contains(argument.range, closing)
            || argument.body_range.start() < opening.end()
            || argument.body_range.end() > closing.start()
        {
            return Err(DeclarationAttributeSyntaxError::InvalidDelimiters);
        }
    }

    match argument.disposition {
        DeclarationAttributeArgumentDisposition::Exact
        | DeclarationAttributeArgumentDisposition::Empty => {
            let Some(delimiters) = delimiters else {
                return Err(DeclarationAttributeSyntaxError::MissingExactDelimiters);
            };
            let opening = delimiters.opening();
            let closing = delimiters.closing();
            if argument.range.start() != opening.start()
                || argument.range.end() != closing.end()
                || argument.body_range.start() != opening.end()
                || argument.body_range.end() != closing.start()
                || (argument.disposition == DeclarationAttributeArgumentDisposition::Empty
                    && !argument.body_range.is_empty())
                || (argument.disposition == DeclarationAttributeArgumentDisposition::Exact
                    && argument.body_range.is_empty())
            {
                return Err(DeclarationAttributeSyntaxError::InvalidDelimiters);
            }
        }
        DeclarationAttributeArgumentDisposition::Recovered
        | DeclarationAttributeArgumentDisposition::Unavailable => {}
    }
    Ok(())
}

fn validate_range(range: SourceLocation) -> Result<(), DeclarationAttributeSyntaxError> {
    if range.start() > range.end() {
        Err(DeclarationAttributeSyntaxError::InvalidRange)
    } else {
        Ok(())
    }
}

const fn contains(parent: SourceLocation, child: SourceLocation) -> bool {
    parent.start() <= child.start() && child.end() <= parent.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize) -> SourceLocation {
        SourceLocation::new(start, end)
    }

    fn exact_argument() -> DeclarationAttributeArgumentSyntax {
        DeclarationAttributeArgumentSyntax {
            range: span(7, 12),
            body_range: span(8, 11),
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(7, 8),
                closing: span(11, 12),
            }),
            disposition: DeclarationAttributeArgumentDisposition::Exact,
        }
    }

    fn exact_attribute(
        name: &str,
    ) -> Result<DeclarationAttributeSyntax, DeclarationAttributeSyntaxError> {
        DeclarationAttributeSyntax::new(
            DeclarationAttributeSeparator::Colon { range: span(0, 1) },
            name.to_owned(),
            span(1, 1 + name.len()),
            Some(exact_argument()),
            span(0, 12),
            DeclarationAttributeCompleteness::Exact,
        )
    }

    #[test]
    fn preserves_order_duplicates_and_spelling_without_interpretation()
    -> Result<(), DeclarationAttributeSyntaxError> {
        let attributes =
            [exact_attribute("reader")?, exact_attribute("reader")?, exact_attribute("custom")?];
        assert_eq!(attributes[0].name(), attributes[1].name());
        assert_eq!(
            attributes.iter().map(|a| a.name()).collect::<Vec<_>>(),
            ["reader", "reader", "custom"]
        );
        assert_ne!(attributes[0], attributes[2]);
        Ok(())
    }

    #[test]
    fn preserves_separator_geometry_and_whitespace_continuation()
    -> Result<(), DeclarationAttributeSyntaxError> {
        let attribute = DeclarationAttributeSyntax::new(
            DeclarationAttributeSeparator::WhitespaceContinuation { range: span(0, 2) },
            "does".into(),
            span(2, 6),
            None,
            span(0, 6),
            DeclarationAttributeCompleteness::Exact,
        )?;
        assert_eq!(attribute.separator().range(), span(0, 2));
        Ok(())
    }

    #[test]
    fn rejects_zero_width_whitespace_continuation() {
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::WhitespaceContinuation { range: span(0, 0) },
                "a".into(),
                span(0, 1),
                None,
                span(0, 1),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry)
        );
    }

    #[test]
    fn rejects_maximum_offset_colon_without_overflowing() {
        // A zero-width colon at `usize::MAX` must return a validation error
        // instead of overflowing the width arithmetic in debug builds.
        let max = span(usize::MAX, usize::MAX);
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: max },
                "a".into(),
                span(0, 1),
                None,
                span(0, 1),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry)
        );
    }

    #[test]
    fn distinguishes_absent_empty_exact_recovered_and_unavailable_arguments()
    -> Result<(), DeclarationAttributeSyntaxError> {
        let absent = DeclarationAttributeSyntax::new(
            DeclarationAttributeSeparator::Colon { range: span(0, 1) },
            "a".into(),
            span(1, 2),
            None,
            span(0, 2),
            DeclarationAttributeCompleteness::Exact,
        )?;
        assert!(absent.argument().is_none());

        for disposition in [
            DeclarationAttributeArgumentDisposition::Empty,
            DeclarationAttributeArgumentDisposition::Exact,
        ] {
            let argument = if disposition == DeclarationAttributeArgumentDisposition::Empty {
                DeclarationAttributeArgumentSyntax {
                    disposition,
                    range: span(7, 9),
                    body_range: span(8, 8),
                    delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                        opening: span(7, 8),
                        closing: span(8, 9),
                    }),
                }
            } else {
                DeclarationAttributeArgumentSyntax { disposition, ..exact_argument() }
            };
            let attribute_range = span(0, argument.range.end());
            assert!(
                DeclarationAttributeSyntax::new(
                    DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                    "a".into(),
                    span(1, 2),
                    Some(argument),
                    attribute_range,
                    DeclarationAttributeCompleteness::Exact,
                )
                .is_ok()
            );
        }

        for disposition in [
            DeclarationAttributeArgumentDisposition::Recovered,
            DeclarationAttributeArgumentDisposition::Unavailable,
        ] {
            let argument = DeclarationAttributeArgumentSyntax {
                disposition,
                delimiters: None,
                range: span(7, 12),
                body_range: span(7, 12),
            };
            assert!(
                DeclarationAttributeSyntax::new(
                    DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                    "a".into(),
                    span(1, 2),
                    Some(argument),
                    span(0, 12),
                    DeclarationAttributeCompleteness::Recovered,
                )
                .is_ok()
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_recovered_syntax_marked_exact() {
        let argument = DeclarationAttributeArgumentSyntax {
            disposition: DeclarationAttributeArgumentDisposition::Recovered,
            delimiters: None,
            range: span(7, 12),
            body_range: span(7, 12),
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(argument),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::ContradictoryCompleteness)
        );
    }

    #[test]
    fn rejects_missing_delimiters_for_exact_argument() {
        let argument = DeclarationAttributeArgumentSyntax { delimiters: None, ..exact_argument() };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(argument),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::MissingExactDelimiters)
        );
    }

    #[test]
    fn rejects_invalid_range_and_delimiter_order() {
        // A reversed span like (4, 3) is unrepresentable through public
        // constructors since #8740: `ByteSpan` ordering is enforced at
        // construction, so the former `InvalidRange` rejection for
        // `range.start > range.end` is unreachable by construction.
        let argument = DeclarationAttributeArgumentSyntax {
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(10, 11),
                closing: span(8, 9),
            }),
            ..exact_argument()
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(argument),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        let wide_delimiters = DeclarationAttributeArgumentSyntax {
            range: span(7, 12),
            body_range: span(9, 11),
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(7, 9),
                closing: span(11, 12),
            }),
            disposition: DeclarationAttributeArgumentDisposition::Exact,
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(wide_delimiters),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 2) },
                "a".into(),
                span(2, 3),
                None,
                span(0, 3),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry)
        );

        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                None,
                span(0, 4),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry)
        );

        let zero_width_delimiters = DeclarationAttributeArgumentSyntax {
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(7, 7),
                closing: span(11, 12),
            }),
            ..exact_argument()
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(zero_width_delimiters),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        let body_outside_delimiters =
            DeclarationAttributeArgumentSyntax { body_range: span(8, 12), ..exact_argument() };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(body_outside_delimiters),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        let non_empty_empty = DeclarationAttributeArgumentSyntax {
            disposition: DeclarationAttributeArgumentDisposition::Empty,
            body_range: span(8, 10),
            ..exact_argument()
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(non_empty_empty),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        let recovered_invalid_delimiters = DeclarationAttributeArgumentSyntax {
            disposition: DeclarationAttributeArgumentDisposition::Recovered,
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(10, 11),
                closing: span(8, 9),
            }),
            ..exact_argument()
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(recovered_invalid_delimiters),
                span(0, 12),
                DeclarationAttributeCompleteness::Recovered,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );

        let recovered_body_contains_delimiters = DeclarationAttributeArgumentSyntax {
            disposition: DeclarationAttributeArgumentDisposition::Recovered,
            delimiters: Some(DeclarationAttributeDelimiter::Parentheses {
                opening: span(7, 8),
                closing: span(11, 12),
            }),
            range: span(7, 12),
            body_range: span(7, 12),
        };
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(recovered_body_contains_delimiters),
                span(0, 12),
                DeclarationAttributeCompleteness::Recovered,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidDelimiters)
        );
    }

    #[test]
    fn rejects_empty_names_and_out_of_range_children() {
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                String::new(),
                span(1, 1),
                None,
                span(0, 1),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::EmptyName)
        );
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(2, 3),
                None,
                span(0, 2),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::RangeOutsideParent)
        );

        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "reader".into(),
                span(1, 4),
                None,
                span(0, 7),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::InvalidAttributeGeometry)
        );

        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(3, 4) },
                "a".into(),
                span(1, 2),
                None,
                span(0, 4),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::RangeOutsideParent)
        );

        let argument = exact_argument();
        assert_eq!(
            DeclarationAttributeSyntax::new(
                DeclarationAttributeSeparator::Colon { range: span(0, 1) },
                "a".into(),
                span(1, 2),
                Some(DeclarationAttributeArgumentSyntax { range: span(1, 6), ..argument }),
                span(0, 12),
                DeclarationAttributeCompleteness::Exact,
            ),
            Err(DeclarationAttributeSyntaxError::RangeOutsideParent)
        );
    }
}
