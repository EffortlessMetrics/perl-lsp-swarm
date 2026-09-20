# Prototype and signature semicolon boundary

Issue #2142 listed semicolon separation as a missing signature feature. That premise is incorrect for core Perl.

- sub proto($;@) { } uses `;` as valid prototype syntax.
- sub f($required; $optional) { } is invalid core Perl signature syntax; optional signature parameters use an explicit default such as `$optional = undef`.
- The parser must keep these productions distinct and preserve the recovered diagnostic for the invalid signature form.

This is a characterization record, not a new parser feature. The remaining #2142 items are separate work: user-defined block prototypes, ampersand-call provenance, signature completion, signature hover metadata, and underscore prototype semantics.
