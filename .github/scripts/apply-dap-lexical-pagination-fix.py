#!/usr/bin/env python3
"""Apply the bounded lexical-collection pagination repair on the DAP scorecard branch."""

from __future__ import annotations

from pathlib import Path


def replace_once(text: str, old: str, new: str, context: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{context}: expected one match, found {count}")
    return text.replace(old, new, 1)


def patch_variables() -> None:
    path = Path("crates/perl-dap/src/debug_adapter/variables.rs")
    text = path.read_text(encoding="utf-8")

    text = replace_once(
        text,
        """    /// Arrays (`@foo`) and hashes (`%foo`) are emitted as `ARRAY(0x0)` / `HASH(0x0)`
    /// so that `VariableParser::parse_assignment` recognises them as expandable
    /// collections — the same format used by the `V` command for package variables.
""",
        """    /// Arrays and hashes are recovered through `B::SV::object_2svref` and
    /// serialized as bounded one-line literals. This lets `VariableParser` retain
    /// child counts and serve later `variables` pages. Pointer-like placeholders
    /// remain only as an honest fallback when the read-only B object cannot be converted.
""",
        "lexical collection documentation",
    )

    text = replace_once(
        text,
        '                "p eval {{ require B; ",\n',
        '                "p eval {{ require B; require Data::Dumper; ",\n'
        '                "local $Data::Dumper::Terse=1; ",\n'
        '                "local $Data::Dumper::Indent=0; ",\n'
        '                "local $Data::Dumper::Useqq=1; ",\n'
        '                "local $Data::Dumper::Sortkeys=1; ",\n'
        '                "local $Data::Dumper::Maxdepth=4; ",\n',
        "Data::Dumper setup",
    )

    old_collection_lines = """                "  if ($rt eq 'B::AV') {{ $v='ARRAY(0x0)' }} ",
                "  elsif ($rt eq 'B::HV') {{ $v='HASH(0x0)' }} ",
"""
    new_collection_lines = """                "  if ($rt eq 'B::AV') {{ ",
                "    my $r=eval{{$s->object_2svref}}; ",
                "    if (ref($r) eq 'ARRAY') {{ ",
                "      my @v=@$r; splice(@v,1024) if @v>1024; ",
                "      $v=eval{{Data::Dumper::Dumper(\\@v)}}; ",
                "    }} ",
                "    $v//='ARRAY(0x0)'; ",
                "  }} ",
                "  elsif ($rt eq 'B::HV') {{ ",
                "    my $r=eval{{$s->object_2svref}}; ",
                "    if (ref($r) eq 'HASH') {{ ",
                "      my @k=sort keys %$r; splice(@k,1024) if @k>1024; ",
                "      my %v; @v{{@k}}=@$r{{@k}}; ",
                "      $v=eval{{Data::Dumper::Dumper(\\%v)}}; ",
                "    }} ",
                "    $v//='HASH(0x0)'; ",
                "  }} ",
"""
    text = replace_once(
        text,
        old_collection_lines,
        new_collection_lines,
        "B collection rendering",
    )

    text = replace_once(
        text,
        """        // Arrays must produce an ARRAY(0x0) value parseable by VariableParser.
        assert!(
            cmd.contains("ARRAY(0x0)"),
            "Perl code must format array vars as ARRAY(0x0): {cmd}"
        );
        // Hashes must produce a HASH(0x0) value parseable by VariableParser.
        assert!(cmd.contains("HASH(0x0)"), "Perl code must format hash vars as HASH(0x0): {cmd}");
""",
        """        assert!(
            cmd.contains("object_2svref"),
            "Perl code must recover the read-only collection value from B: {cmd}"
        );
        assert!(
            cmd.contains("Data::Dumper::Dumper"),
            "Perl code must emit a one-line parseable collection literal: {cmd}"
        );
        assert!(
            cmd.contains("splice(@v,1024)") && cmd.contains("splice(@k,1024)"),
            "Perl code must bound array and hash materialization: {cmd}"
        );
        // Pointer-like values remain explicit fallbacks when B cannot expose the collection.
        assert!(cmd.contains("ARRAY(0x0)"), "array fallback must remain explicit: {cmd}");
        assert!(cmd.contains("HASH(0x0)"), "hash fallback must remain explicit: {cmd}");
""",
        "B collection command assertions",
    )

    insertion_anchor = """    #[test]
    fn build_locals_b_eval_cmd_output_format_matches_variable_parser() {
"""
    deep_test = """    #[test]
    fn parsed_lexical_array_preserves_a_deep_page() {
        let values = (1..=500).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let lines = vec![format!("@big = [{values}]")];
        let (roots, child_cache) =
            DebugAdapter::parse_scope_variables_from_lines(&lines, 11, 0, 1024);
        let root = roots
            .iter()
            .find(|variable| variable.name == "@big")
            .expect("@big root must be rendered");
        assert_eq!(root.indexed_variables, Some(500));
        assert!(root.variables_reference > 0);

        let children = child_cache
            .get(&root.variables_reference)
            .expect("@big children must be cached for paging");
        assert_eq!(children.len(), 500);
        assert_eq!(children[250].name, "[250]");
        assert_eq!(children[250].value, "251");
        assert_eq!(children[274].name, "[274]");
        assert_eq!(children[274].value, "275");
    }

"""
    text = replace_once(
        text,
        insertion_anchor,
        deep_test + insertion_anchor,
        "deep lexical pagination test",
    )

    path.write_text(text, encoding="utf-8")


def patch_scope_cache() -> None:
    path = Path("crates/perl-dap/src/debug_adapter/parsing/scope_variables.rs")
    text = path.read_text(encoding="utf-8")
    text = replace_once(
        text,
        "use crate::value::PerlValue;\n",
        "use crate::value::PerlValue;\n\nconst MAX_CACHED_CHILDREN: usize = 1024;\n",
        "child cache bound",
    )
    text = replace_once(
        text,
        ".render_children(&value, 0, 256)",
        ".render_children(&value, 0, MAX_CACHED_CHILDREN)",
        "child cache materialization",
    )
    path.write_text(text, encoding="utf-8")


def patch_scorecard_fixture() -> None:
    path = Path("scripts/ci/dap_scorecard_probes.py")
    text = path.read_text(encoding="utf-8")
    for old, new in (
        ("our $x = 41;", "my $x = 41;"),
        ("our @big = (1..500);", "my @big = (1..500);"),
        ("our %meta = (name => \\\"dap-scorecard\\\");", "my %meta = (name => \\\"dap-scorecard\\\");"),
    ):
        text = replace_once(text, old, new, f"scorecard lexical {old}")
    path.write_text(text, encoding="utf-8")


def main() -> None:
    patch_variables()
    patch_scope_cache()
    patch_scorecard_fixture()


if __name__ == "__main__":
    main()
