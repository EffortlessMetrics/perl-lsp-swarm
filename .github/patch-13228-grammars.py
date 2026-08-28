#!/usr/bin/env python3
"""Apply precedence-correct logical-XOR edits to secondary grammars."""

from __future__ import annotations

from pathlib import Path


def replace_exact(path: str | Path, old: str, new: str) -> None:
    file_path = Path(path)
    text = file_path.read_text()
    actual = text.count(old)
    if actual != 1:
        raise SystemExit(
            f"{file_path}: expected one occurrence, found {actual}: {old!r}"
        )
    file_path.write_text(text.replace(old, new))


# Keep word `or` / `xor` at their existing lower-precedence tiers while
# grouping the C-style `||`, `^^`, and `//` operators together, as Perl does.
pest_old = '''logical_or_expression = { logical_xor_expression ~ (logical_or_op ~ logical_xor_expression)* }
logical_or_op = { "||" | "or" }
logical_xor_expression = { defined_or_expression ~ (logical_xor_op ~ defined_or_expression)* }
logical_xor_op = { "xor" }
defined_or_expression = { logical_and_expression ~ ("//" ~ logical_and_expression)* }
'''
pest_new = '''logical_or_expression = { logical_xor_expression ~ (logical_or_op ~ logical_xor_expression)* }
logical_or_op = { "or" }
logical_xor_expression = { defined_or_expression ~ (logical_xor_op ~ defined_or_expression)* }
logical_xor_op = { "xor" }
defined_or_expression = {
    logical_and_expression ~ (cstyle_logical_op ~ logical_and_expression)*
}
cstyle_logical_op = { "||" | "^^" | "//" }
'''

for pest_path in (
    "crates/perl-parser-pest/src/grammar.pest",
    "archive/crates/tree-sitter-perl-rs/src/grammar.pest",
):
    replace_exact(pest_path, pest_old, pest_new)
    replace_exact(
        pest_path,
        '    | "&&=" | "||=" | "//=" | "&.=" | "|.=" | "^.="',
        '    | "&&=" | "||=" | "^^=" | "//=" | "&.=" | "|.=" | "^.="',
    )

replace_exact(
    "tree-sitter-perl/grammar.js",
    "        [prec.left, binop, choice('||', '//'), TERMPREC.OROR], // _OROR_DORDOR",
    "        [prec.left, binop, choice('||', '^^', '//'), TERMPREC.OROR], // _OROR_DORDOR",
)
replace_exact(
    "tree-sitter-perl/grammar.js",
    "          '&&=', '||=', '//=',",
    "          '&&=', '||=', '^^=', '//=',",
)

print("patched Pest and Tree-sitter logical-XOR grammar surfaces")
