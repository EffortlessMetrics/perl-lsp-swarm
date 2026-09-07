//! Compatibility decoding for `field` trait spellings.
//!
//! The parser retains a `field` declaration's attributes as raw source
//! spellings such as `param`, `reader`, or `writer(write_name)`. Downstream
//! compatibility consumers previously compared those spellings against exact
//! bare strings, which silently discarded every explicitly named form.
//!
//! This module decodes one admitted spelling once and preserves its identity:
//! the trait family, the raw spelling, and the argument disposition. It
//! deliberately assigns no profile semantics — whether a language or module
//! profile admits a decoded form remains the caller's decision, so decoding
//! alone cannot widen a core or `Object::Pad` claim.
//!
//! # Terminal disposition
//!
//! This is a bounded compatibility path, not a canonical fact producer. It
//! consumes the string attributes the parser retains today and is expected to
//! be replaced by the typed declaration-attribute syntax owned by
//! `perl_ast::DeclarationAttributeSyntax` once that value reaches the `field`
//! declaration node, and then removed after canonical generated-member facts
//! are promoted. It must not grow into a second long-lived attribute parser.

/// The trait family a decoded `field` attribute belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldTraitKind {
    /// `:param` — the field participates in constructor parameters.
    Param,
    /// `:reader` — a read accessor is generated.
    Reader,
    /// `:writer` — a write accessor is generated.
    Writer,
    /// `:accessor` — a combined read/write accessor is generated.
    Accessor,
    /// `:mutator` — a combined read/write accessor is generated.
    Mutator,
    /// Any other attribute, including user or extension attributes.
    ///
    /// An unknown family never generates a member and never widens a known
    /// trait's meaning, no matter what argument it carries.
    Unknown,
}

/// The disposition of a decoded trait's parenthesized argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldTraitArgument {
    /// The bare spelling, as in `:reader`. The family's default applies.
    None,
    /// An exact static name, as in `:reader(read_name)`.
    StaticName(String),
    /// Delimiters were present but the argument body was empty, as in
    /// `:reader()`.
    Empty,
    /// The argument was unclosed, or its body is not a static name — for
    /// example `:reader(` or `:reader($dyn)`.
    ///
    /// This state stays explicit rather than degrading into either the bare
    /// default or a trimmed static name.
    MalformedOrDynamic,
}

/// One decoded `field` trait spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedFieldTrait {
    /// The trait family this spelling belongs to.
    pub(crate) kind: FieldTraitKind,
    /// The attribute spelling exactly as the parser retained it.
    ///
    /// Preserved so compatibility consumers and later diagnostics do not have
    /// to re-derive source identity from a normalized value.
    pub(crate) raw_spelling: String,
    /// The decoded argument disposition.
    pub(crate) argument: FieldTraitArgument,
}

impl DecodedFieldTrait {
    /// Decode one raw attribute spelling.
    ///
    /// Decoding never evaluates the argument and never infers a name that the
    /// source did not spell.
    pub(crate) fn decode(raw_spelling: &str) -> Self {
        let spelling = raw_spelling.trim();
        let (name, argument) = match spelling.find('(') {
            None => (spelling, FieldTraitArgument::None),
            Some(open) => {
                let (name, rest) = spelling.split_at(open);
                // `rest` starts at the opening delimiter.
                let argument = match rest.strip_prefix('(').and_then(|body| body.strip_suffix(')'))
                {
                    // Unclosed argument: the source proposition is incomplete.
                    None => FieldTraitArgument::MalformedOrDynamic,
                    Some(body) => decode_argument_body(body),
                };
                (name, argument)
            }
        };

        Self {
            kind: FieldTraitKind::from_name(name.trim()),
            raw_spelling: raw_spelling.to_owned(),
            argument,
        }
    }

    /// Return true when this is the bare spelling of its family.
    pub(crate) fn is_bare(&self) -> bool {
        matches!(self.argument, FieldTraitArgument::None)
    }
}

impl FieldTraitKind {
    fn from_name(name: &str) -> Self {
        match name {
            "param" => Self::Param,
            "reader" => Self::Reader,
            "writer" => Self::Writer,
            "accessor" => Self::Accessor,
            "mutator" => Self::Mutator,
            _ => Self::Unknown,
        }
    }
}

fn decode_argument_body(body: &str) -> FieldTraitArgument {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return FieldTraitArgument::Empty;
    }
    if is_static_name(trimmed) {
        FieldTraitArgument::StaticName(trimmed.to_owned())
    } else {
        FieldTraitArgument::MalformedOrDynamic
    }
}

/// Return true when `name` is a static Perl method-name identifier.
///
/// Anything else — a sigil, a call, an interpolation, punctuation — is not a
/// name this compatibility path is willing to generate a member from.
fn is_static_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// Decode the first trait of `kind` in source order.
///
/// The first trait of a family owns that family's result. A malformed or
/// unsupported first spelling fails closed rather than falling through to a
/// later duplicate, so a broken spelling can never be repaired into a
/// generated member by an unrelated attribute.
pub(crate) fn first_trait_of_kind(
    attributes: &[String],
    kind: FieldTraitKind,
) -> Option<DecodedFieldTrait> {
    attributes
        .iter()
        .map(|attribute| DecodedFieldTrait::decode(attribute))
        .find(|decoded| decoded.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(spelling: &str) -> (FieldTraitKind, FieldTraitArgument) {
        let decoded = DecodedFieldTrait::decode(spelling);
        assert_eq!(
            decoded.raw_spelling, spelling,
            "the raw spelling must survive decoding unchanged"
        );
        (decoded.kind, decoded.argument)
    }

    #[test]
    fn decodes_every_admitted_spelling_shape() {
        let cases: &[(&str, FieldTraitKind, FieldTraitArgument)] = &[
            // Bare forms keep their family default.
            ("param", FieldTraitKind::Param, FieldTraitArgument::None),
            ("reader", FieldTraitKind::Reader, FieldTraitArgument::None),
            ("writer", FieldTraitKind::Writer, FieldTraitArgument::None),
            ("accessor", FieldTraitKind::Accessor, FieldTraitArgument::None),
            ("mutator", FieldTraitKind::Mutator, FieldTraitArgument::None),
            // Explicit static names are retained exactly.
            (
                "param(ext_name)",
                FieldTraitKind::Param,
                FieldTraitArgument::StaticName("ext_name".to_owned()),
            ),
            (
                "reader(read_name)",
                FieldTraitKind::Reader,
                FieldTraitArgument::StaticName("read_name".to_owned()),
            ),
            (
                "writer(write_name)",
                FieldTraitKind::Writer,
                FieldTraitArgument::StaticName("write_name".to_owned()),
            ),
            (
                "accessor(access_name)",
                FieldTraitKind::Accessor,
                FieldTraitArgument::StaticName("access_name".to_owned()),
            ),
            (
                "mutator(mutate_name)",
                FieldTraitKind::Mutator,
                FieldTraitArgument::StaticName("mutate_name".to_owned()),
            ),
            // A leading underscore is a valid identifier, not a private marker
            // to be stripped at decode time.
            (
                "reader(_hidden)",
                FieldTraitKind::Reader,
                FieldTraitArgument::StaticName("_hidden".to_owned()),
            ),
            // Empty arguments stay an explicit bounded state.
            ("reader()", FieldTraitKind::Reader, FieldTraitArgument::Empty),
            ("writer(   )", FieldTraitKind::Writer, FieldTraitArgument::Empty),
            // Malformed and dynamic arguments never become static names.
            ("reader(", FieldTraitKind::Reader, FieldTraitArgument::MalformedOrDynamic),
            ("writer(write_name", FieldTraitKind::Writer, FieldTraitArgument::MalformedOrDynamic),
            ("reader($dyn)", FieldTraitKind::Reader, FieldTraitArgument::MalformedOrDynamic),
            ("reader(1bad)", FieldTraitKind::Reader, FieldTraitArgument::MalformedOrDynamic),
            ("reader(a-b)", FieldTraitKind::Reader, FieldTraitArgument::MalformedOrDynamic),
            ("reader(get())", FieldTraitKind::Reader, FieldTraitArgument::MalformedOrDynamic),
            (
                "accessor(Foo::bar)",
                FieldTraitKind::Accessor,
                FieldTraitArgument::MalformedOrDynamic,
            ),
            // Unknown families never borrow a known trait's meaning.
            ("Custom", FieldTraitKind::Unknown, FieldTraitArgument::None),
            (
                "Custom(read_name)",
                FieldTraitKind::Unknown,
                FieldTraitArgument::StaticName("read_name".to_owned()),
            ),
            ("Reader", FieldTraitKind::Unknown, FieldTraitArgument::None),
            ("readers", FieldTraitKind::Unknown, FieldTraitArgument::None),
            ("myreader", FieldTraitKind::Unknown, FieldTraitArgument::None),
        ];

        for (spelling, kind, argument) in cases {
            assert_eq!(
                decoded(spelling),
                (*kind, argument.clone()),
                "decoding `{spelling}` did not preserve its source identity"
            );
        }
    }

    #[test]
    fn a_static_name_is_exposed_only_for_exact_names() {
        assert_eq!(
            DecodedFieldTrait::decode("reader(read_name)").argument,
            FieldTraitArgument::StaticName("read_name".to_owned())
        );
        for spelling in ["reader", "reader()", "reader(", "reader($dyn)"] {
            assert!(
                !matches!(
                    DecodedFieldTrait::decode(spelling).argument,
                    FieldTraitArgument::StaticName(_)
                ),
                "`{spelling}` must not expose a static name"
            );
        }
    }

    #[test]
    fn only_the_bare_spelling_reports_as_bare() {
        assert!(DecodedFieldTrait::decode("reader").is_bare());
        for spelling in ["reader(read_name)", "reader()", "reader("] {
            assert!(
                !DecodedFieldTrait::decode(spelling).is_bare(),
                "`{spelling}` must not report as the bare trait"
            );
        }
    }

    #[test]
    fn first_trait_of_a_family_owns_the_result() {
        let attributes = vec![
            "param".to_owned(),
            "reader(".to_owned(),
            "reader".to_owned(),
            "writer(write_name)".to_owned(),
        ];

        let reader = first_trait_of_kind(&attributes, FieldTraitKind::Reader)
            .expect("a reader trait is present");
        assert_eq!(
            reader.argument,
            FieldTraitArgument::MalformedOrDynamic,
            "a malformed first spelling must not be repaired by a later duplicate"
        );

        let writer = first_trait_of_kind(&attributes, FieldTraitKind::Writer)
            .expect("a writer trait is present");
        assert_eq!(writer.argument, FieldTraitArgument::StaticName("write_name".to_owned()));

        assert_eq!(first_trait_of_kind(&attributes, FieldTraitKind::Mutator), None);
    }
}
