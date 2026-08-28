#!/usr/bin/env python3
"""Apply the bounded Perl 5.40/5.42 logical-XOR patch for issue #13228."""

from __future__ import annotations

import re
from pathlib import Path


def replace_exact(
    path: str | Path,
    old: str,
    new: str,
    *,
    expected: int = 1,
) -> int:
    file_path = Path(path)
    text = file_path.read_text()
    actual = text.count(old)
    if actual != expected:
        raise SystemExit(
            f"{file_path}: expected {expected} occurrences, found {actual}: {old!r}"
        )
    file_path.write_text(text.replace(old, new))
    return actual


def replace_regex(
    path: str | Path,
    pattern: str,
    replacement: str,
    *,
    expected: int,
) -> int:
    file_path = Path(path)
    text = file_path.read_text()
    updated, actual = re.subn(pattern, replacement, text, flags=re.MULTILINE)
    if actual != expected:
        raise SystemExit(
            f"{file_path}: expected {expected} regex replacements, found {actual}: {pattern!r}"
        )
    file_path.write_text(updated)
    return actual


def insert_after_all(
    root: str | Path,
    glob: str,
    anchor: str,
    addition: str,
    *,
    minimum: int,
) -> int:
    total = 0
    for file_path in Path(root).rglob(glob):
        text = file_path.read_text()
        count = text.count(anchor)
        if count:
            file_path.write_text(text.replace(anchor, anchor + addition))
            total += count
    if total < minimum:
        raise SystemExit(
            f"{root}: expected at least {minimum} insertions after {anchor!r}, found {total}"
        )
    return total


# ---------------------------------------------------------------------------
# Canonical token contract
# ---------------------------------------------------------------------------

KIND = Path("crates/perl-token/src/kind.rs")

replace_exact(
    KIND,
    "    /// Logical OR and assign: `||=`\n    LogicalOrAssign,\n"
    "    /// Defined-or and assign: `//=`\n",
    "    /// Logical OR and assign: `||=`\n    LogicalOrAssign,\n"
    "    /// Logical XOR and assign (Perl 5.42+): `^^=`\n    LogicalXorAssign,\n"
    "    /// Defined-or and assign: `//=`\n",
)
replace_exact(
    KIND,
    "    /// Logical OR: `||`\n    Or,\n    /// Logical NOT: `!`\n",
    "    /// Logical OR: `||`\n    Or,\n"
    "    /// Logical XOR (Perl 5.40+): `^^`\n    LogicalXor,\n"
    "    /// Logical NOT: `!`\n",
)
replace_exact(
    KIND,
    '    ("||=", TokenKind::LogicalOrAssign),\n'
    '    ("//=", TokenKind::DefinedOrAssign),\n',
    '    ("||=", TokenKind::LogicalOrAssign),\n'
    '    ("^^=", TokenKind::LogicalXorAssign),\n'
    '    ("//=", TokenKind::DefinedOrAssign),\n',
)
replace_exact(
    KIND,
    '    ("||", TokenKind::Or),\n    ("!", TokenKind::Not),\n',
    '    ("||", TokenKind::Or),\n'
    '    ("^^", TokenKind::LogicalXor),\n'
    '    ("!", TokenKind::Not),\n',
)
replace_exact(
    KIND,
    "            | TokenKind::LogicalAndAssign\n"
    "            | TokenKind::LogicalOrAssign\n"
    "            | TokenKind::DefinedOrAssign\n",
    "            | TokenKind::LogicalAndAssign\n"
    "            | TokenKind::LogicalOrAssign\n"
    "            | TokenKind::LogicalXorAssign\n"
    "            | TokenKind::DefinedOrAssign\n",
    expected=2,
)
replace_exact(
    KIND,
    "            | TokenKind::And\n"
    "            | TokenKind::Or\n"
    "            | TokenKind::Not\n",
    "            | TokenKind::And\n"
    "            | TokenKind::Or\n"
    "            | TokenKind::LogicalXor\n"
    "            | TokenKind::Not\n",
    expected=2,
)
replace_exact(
    KIND,
    '            "||=" => Some(TokenKind::LogicalOrAssign),\n'
    '            "//=" => Some(TokenKind::DefinedOrAssign),\n',
    '            "||=" => Some(TokenKind::LogicalOrAssign),\n'
    '            "^^=" => Some(TokenKind::LogicalXorAssign),\n'
    '            "//=" => Some(TokenKind::DefinedOrAssign),\n',
)
replace_exact(
    KIND,
    '            "||" => Some(TokenKind::Or),\n'
    '            "!" => Some(TokenKind::Not),\n',
    '            "||" => Some(TokenKind::Or),\n'
    '            "^^" => Some(TokenKind::LogicalXor),\n'
    '            "!" => Some(TokenKind::Not),\n',
)
replace_exact(
    KIND,
    '            TokenKind::LogicalOrAssign => "\'||=\'",\n'
    '            TokenKind::DefinedOrAssign => "\'//=\'",\n',
    '            TokenKind::LogicalOrAssign => "\'||=\'",\n'
    '            TokenKind::LogicalXorAssign => "\'^^=\'",\n'
    '            TokenKind::DefinedOrAssign => "\'//=\'",\n',
)
replace_exact(
    KIND,
    '            TokenKind::Or => "\'||\'",\n'
    '            TokenKind::Not => "\'!\'",\n',
    '            TokenKind::Or => "\'||\'",\n'
    '            TokenKind::LogicalXor => "\'^^\'",\n'
    '            TokenKind::Not => "\'!\'",\n',
)
replace_exact(
    KIND,
    "const TOKEN_KIND_ALL: [TokenKind; 132] = [",
    "const TOKEN_KIND_ALL: [TokenKind; 134] = [",
)
replace_exact(
    KIND,
    "    TokenKind::LogicalAndAssign,\n"
    "    TokenKind::LogicalOrAssign,\n"
    "    TokenKind::DefinedOrAssign,\n",
    "    TokenKind::LogicalAndAssign,\n"
    "    TokenKind::LogicalOrAssign,\n"
    "    TokenKind::LogicalXorAssign,\n"
    "    TokenKind::DefinedOrAssign,\n",
)
replace_exact(
    KIND,
    "    TokenKind::And,\n    TokenKind::Or,\n    TokenKind::Not,\n",
    "    TokenKind::And,\n"
    "    TokenKind::Or,\n"
    "    TokenKind::LogicalXor,\n"
    "    TokenKind::Not,\n",
)

# Keep exhaustive/manual token test tables in declaration order. Compilation
# remains the backstop for any differently-shaped exhaustive match.
insert_after_all(
    "crates/perl-token/tests",
    "*.rs",
    "TokenKind::LogicalOrAssign,\n",
    "        TokenKind::LogicalXorAssign,\n",
    minimum=2,
)
insert_after_all(
    "crates/perl-token/tests",
    "*.rs",
    "TokenKind::Or,\n",
    "        TokenKind::LogicalXor,\n",
    minimum=2,
)

# ---------------------------------------------------------------------------
# Longest-match lexer recognition
# ---------------------------------------------------------------------------

CLASSIFICATION = Path(
    "crates/perl-lexer/src/lexer/helpers/operator_classification.rs"
)
replace_exact(
    CLASSIFICATION,
    'const COMPOUND_SECOND_CHARS: &[u8] = b"=<>&|+->.~*:";',
    'const COMPOUND_SECOND_CHARS: &[u8] = b"=<>&|^+->.~*:";',
)
replace_exact(
    CLASSIFICATION,
    "            (b'&', b'&') | (b'|', b'|') => true,",
    "            (b'&', b'&') | (b'|', b'|') | (b'^', b'^') => true,",
)
replace_exact(
    CLASSIFICATION,
    "                | ('|', '|')\n",
    "                | ('|', '|')\n                | ('^', '^')\n",
)

LEXER = Path("crates/perl-lexer/src/lib.rs")
replace_exact(
    LEXER,
    "Check for three-character operators like **=, <<=, >>=",
    "Check for three-character operators like **=, <<=, >>=, ^^=",
    expected=2,
)
replace_regex(
    LEXER,
    r"^(\s*)\| \('>', '>', Some\('='\)\)\s*$",
    r"\1| ('>', '>', Some('='))\n\1| ('^', '^', Some('='))",
    expected=2,
)

# ---------------------------------------------------------------------------
# Recursive-descent parser: same precedence tier as || and //, plus every
# assignment gateway that still spells operators explicitly.
# ---------------------------------------------------------------------------

HELPERS = Path("crates/perl-parser-core/src/engine/parser/helpers.rs")
replace_exact(
    HELPERS,
    "        kind.is_some_and(|token| matches!(token, TokenKind::Or | TokenKind::DefinedOr))",
    "        kind.is_some_and(|token| {\n"
    "            matches!(\n"
    "                token,\n"
    "                TokenKind::Or | TokenKind::LogicalXor | TokenKind::DefinedOr\n"
    "            )\n"
    "        })",
)

parser_map_count = 0
parser_optional_map_count = 0
parser_list_count = 0
for parser_path in Path("crates/perl-parser-core/src/engine/parser").rglob("*.rs"):
    text = parser_path.read_text()

    anchor = 'TokenKind::LogicalOrAssign => Some("||="),\n'
    addition = '                TokenKind::LogicalXorAssign => Some("^^="),\n'
    count = text.count(anchor)
    if count:
        text = text.replace(anchor, anchor + addition)
        parser_map_count += count

    optional_anchor = 'Some(TokenKind::LogicalOrAssign) => Some("||="),\n'
    optional_addition = '                Some(TokenKind::LogicalXorAssign) => Some("^^="),\n'
    count = text.count(optional_anchor)
    if count:
        text = text.replace(optional_anchor, optional_anchor + optional_addition)
        parser_optional_map_count += count

    text, count = re.subn(
        r"^(\s*)\| TokenKind::LogicalOrAssign\n(\s*)\| TokenKind::DefinedOrAssign$",
        r"\1| TokenKind::LogicalOrAssign\n"
        r"\1| TokenKind::LogicalXorAssign\n"
        r"\2| TokenKind::DefinedOrAssign",
        text,
        flags=re.MULTILINE,
    )
    parser_list_count += count
    parser_path.write_text(text)

if parser_map_count < 2:
    raise SystemExit(
        f"expected at least two parser assignment maps, updated {parser_map_count}"
    )
if parser_optional_map_count < 1:
    raise SystemExit(
        "expected at least one optional parser assignment map, "
        f"updated {parser_optional_map_count}"
    )
if parser_list_count < 1:
    raise SystemExit(
        f"expected at least one parser assignment classification list, updated {parser_list_count}"
    )

# ---------------------------------------------------------------------------
# Secondary parser grammars
# ---------------------------------------------------------------------------

for pest_path in (
    Path("crates/perl-parser-pest/src/grammar.pest"),
    Path("archive/crates/tree-sitter-perl-rs/src/grammar.pest"),
):
    replace_exact(
        pest_path,
        'logical_xor_op = { "xor" }',
        'logical_xor_op = { "^^" | "xor" }',
    )
    replace_exact(
        pest_path,
        '    "&&=" | "||=" | "//=" |',
        '    "&&=" | "||=" | "^^=" | "//=" |',
    )

TREE_SITTER_GRAMMAR = Path("tree-sitter-perl/grammar.js")
replace_exact(
    TREE_SITTER_GRAMMAR,
    "        choice('||', '//'),",
    "        choice('||', '^^', '//'),",
)
replace_exact(
    TREE_SITTER_GRAMMAR,
    "        '&&=', '||=', '//=',",
    "        '&&=', '||=', '^^=', '//=',",
)

# ---------------------------------------------------------------------------
# Focused executable contracts
# ---------------------------------------------------------------------------

Path("crates/perl-token/tests/logical_xor_contract.rs").write_text(
    '''//! Perl 5.40/5.42 logical-XOR token contract.\n\n'''
    '''use perl_token::TokenKind;\n\n'''
    '''#[test]\n'''
    '''fn logical_xor_spellings_and_roles_are_canonical() {\n'''
    '''    assert_eq!(TokenKind::from_operator("^^"), Some(TokenKind::LogicalXor));\n'''
    '''    assert_eq!(TokenKind::from_operator("^^="), Some(TokenKind::LogicalXorAssign));\n'''
    '''    assert_eq!(TokenKind::LogicalXor.canonical_spelling(), Some("^^"));\n'''
    '''    assert_eq!(TokenKind::LogicalXorAssign.canonical_spelling(), Some("^^="));\n'''
    '''    assert!(TokenKind::LogicalXor.is_logical_operator());\n'''
    '''    assert!(TokenKind::LogicalXorAssign.is_assignment_operator());\n'''
    '''}\n'''
)

Path("crates/perl-lexer/tests/perl_5_40_42_logical_xor.rs").write_text(
    '''//! Perl 5.40/5.42 logical-XOR tokenization contract.\n\n'''
    '''use perl_parser_core::tokens::token_stream::TokenStream;\n'''
    '''use perl_tdd_support::must;\n'''
    '''use perl_token::TokenKind;\n\n'''
    '''#[test]\n'''
    '''fn logical_xor_uses_longest_valid_operator_spelling() -> Result<(), Box<dyn std::error::Error>> {\n'''
    '''    let mut stream = TokenStream::new("$a ^ $b; $a ^= $b; $a ^^ $b; $a ^^= $b;");\n'''
    '''    let mut operators = Vec::new();\n\n'''
    '''    loop {\n'''
    '''        let token = must(stream.next());\n'''
    '''        match token.kind() {\n'''
    '''            TokenKind::BitwiseXor\n'''
    '''            | TokenKind::XorAssign\n'''
    '''            | TokenKind::LogicalXor\n'''
    '''            | TokenKind::LogicalXorAssign => {\n'''
    '''                operators.push((token.kind(), token.text.to_string(), token.start(), token.end()));\n'''
    '''            }\n'''
    '''            TokenKind::Eof => break,\n'''
    '''            _ => {}\n'''
    '''        }\n'''
    '''    }\n\n'''
    '''    assert_eq!(\n'''
    '''        operators.iter().map(|(kind, text, _, _)| (*kind, text.as_str())).collect::<Vec<_>>(),\n'''
    '''        vec![\n'''
    '''            (TokenKind::BitwiseXor, "^"),\n'''
    '''            (TokenKind::XorAssign, "^="),\n'''
    '''            (TokenKind::LogicalXor, "^^"),\n'''
    '''            (TokenKind::LogicalXorAssign, "^^="),\n'''
    '''        ]\n'''
    '''    );\n'''
    '''    for (_, text, start, end) in operators {\n'''
    '''        assert_eq!(end - start, text.len(), "operator span must cover its exact bytes");\n'''
    '''    }\n'''
    '''    Ok(())\n'''
    '''}\n'''
)

Path("crates/perl-parser-core/tests/perl_5_40_42_logical_xor.rs").write_text(
    '''//! Perl 5.40 binary `^^` and Perl 5.42 assigning `^^=`.\n\n'''
    '''mod cpan_test_helpers;\n'''
    '''use cpan_test_helpers::assert_clean_parse;\n'''
    '''use perl_parser_core::Parser;\n\n'''
    '''#[test]\n'''
    '''fn parses_binary_logical_xor() {\n'''
    '''    assert_clean_parse("my $value = $left ^^ $right;");\n'''
    '''}\n\n'''
    '''#[test]\n'''
    '''fn parses_logical_xor_assignment() {\n'''
    '''    assert_clean_parse("$value ^^= expensive_check();");\n'''
    '''}\n\n'''
    '''#[test]\n'''
    '''fn parses_logical_xor_through_declaration_and_condition_gateways() {\n'''
    '''    assert_clean_parse(\n'''
    '''        "my $value ^^= $fallback; if ($left || $middle ^^ $right // $fallback) { 1; }",\n'''
    '''    );\n'''
    '''}\n\n'''
    '''#[test]\n'''
    '''fn preserves_logical_xor_operator_text_in_the_ast() {\n'''
    '''    let mut parser = Parser::new("$left ^^ $right; $value ^^= $fallback;");\n'''
    '''    let output = parser.parse_with_recovery();\n'''
    '''    assert!(output.diagnostics.is_empty(), "unexpected diagnostics: {:?}", output.diagnostics);\n'''
    '''    let sexp = output.ast.to_sexp();\n'''
    '''    assert!(sexp.contains("^^"), "binary logical XOR missing from AST: {sexp}");\n'''
    '''    assert!(sexp.contains("^^="), "logical XOR assignment missing from AST: {sexp}");\n'''
    '''}\n'''
)

print(
    "patched logical XOR contract: "
    f"parser_maps={parser_map_count}, "
    f"optional_maps={parser_optional_map_count}, "
    f"classification_lists={parser_list_count}"
)
