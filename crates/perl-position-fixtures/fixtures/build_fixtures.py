"""Independent, reviewable seed for the checked v1 literal expected-fact JSON.

Run only when deliberately changing the authority. The Rust validator never
imports this script or any production mapper. Review the generated JSON diff.
"""

import hashlib
import json
from pathlib import Path


HERE = Path(__file__).resolve().parent
CASES = [
    ("empty", b"", ["basic", "small"]),
    ("ascii", b"abc", ["basic", "small"]),
    ("bmp", "aé中z".encode(), ["unicode", "small"]),
    ("astral", "x😀y\r\nz".encode(), ["unicode", "crlf", "small", "decisive"]),
    ("combining", "a\u0301b".encode(), ["unicode", "small"]),
    ("repeated", b"abaaba", ["basic", "small"]),
    ("nul", b"a\x00b", ["basic", "small"]),
    ("lf", b"a\nb", ["newline", "small"]),
    ("crlf", b"a\r\nb", ["newline", "crlf", "small"]),
    ("bare_cr", b"a\rb", ["newline", "bare_cr", "small"]),
    ("terminal_cr", b"a\r", ["newline", "bare_cr", "small"]),
    ("mixed", b"a\r\nb\nc\r\nd", ["newline", "crlf", "small"]),
    ("consecutive_lf", b"a\n\nb", ["newline", "small"]),
    ("newline_only", b"\n", ["newline", "small"]),
    ("terminal_lf", b"a\n", ["newline", "small"]),
    ("multiple_terminal_lf", b"a\n\n", ["newline", "small"]),
    ("crcrlf", b"a\r\r\nb", ["newline", "crlf", "small"]),
    ("lfcr", b"a\n\rb", ["newline", "bare_cr", "small"]),
    ("unicode_crlf", "é\r\n😀".encode(), ["newline", "crlf", "unicode", "small"]),
    ("vt", b"a\x0bb", ["ropey_control", "small"]),
    ("ff", b"a\x0cb", ["ropey_control", "small"]),
    ("nel", "a\u0085b".encode(), ["ropey_control", "small"]),
    ("ls", "a\u2028b".encode(), ["ropey_control", "small"]),
    ("ps", "a\u2029b".encode(), ["ropey_control", "small"]),
    ("bom_ascii", b"\xef\xbb\xbfabc", ["bom", "presentation", "small"]),
    ("ascii_for_bom", b"abc", ["bom", "parser_subject", "small"]),
    ("bom_unicode", "\ufeff😀x".encode(), ["bom", "presentation", "unicode", "small"]),
    ("unicode_for_bom", "😀x".encode(), ["bom", "parser_subject", "unicode", "small"]),
    ("bom_only", b"\xef\xbb\xbf", ["bom", "presentation", "small"]),
    ("empty_for_bom", b"", ["bom", "parser_subject", "small"]),
    ("double_bom", "\ufeff\ufeffx".encode(), ["bom", "presentation", "small"]),
    ("single_bom_for_double", "\ufeffx".encode(), ["bom", "parser_subject", "small"]),
    ("nonleading_bom", "a\ufeffb".encode(), ["bom", "small"]),
    ("bom_after_lf", "a\n\ufeffb".encode(), ["bom", "newline", "small"]),
    ("large_line", b"x" * 2048 + b"\n", ["generated_boundary", "newline"]),
    ("many_lines", b"x\n" * 128, ["generated_boundary", "newline"]),
    ("invalid_truncated_start", b"\xe2", ["invalid_utf8", "ingress"]),
    ("invalid_truncated_middle", b"a\xe2", ["invalid_utf8", "ingress"]),
    ("invalid_truncated_end", b"ab\xe2\x82", ["invalid_utf8", "ingress"]),
    ("invalid_leading_start", b"\xffa", ["invalid_utf8", "ingress"]),
    ("invalid_leading_middle", b"a\xffb", ["invalid_utf8", "ingress"]),
    ("invalid_leading_end", b"a\xff", ["invalid_utf8", "ingress"]),
    ("invalid_continuation_start", b"\xe2Aabc", ["invalid_utf8", "ingress"]),
    ("invalid_continuation_middle", b"a\xe2Ab", ["invalid_utf8", "ingress"]),
    ("invalid_continuation_end", b"ab\xe2A", ["invalid_utf8", "ingress"]),
]

RELATIONS = {
    "bom_ascii": "ascii_for_bom",
    "bom_unicode": "unicode_for_bom",
    "bom_only": "empty_for_bom",
    "double_bom": "single_bom_for_double",
}


def expected_case(case_id, raw, tags):
    item = dict(
        id=case_id, tags=tags, raw_identity="literal-hex/v1", raw_hex=raw.hex(),
        exhaustive="generated_boundary" not in tags,
        byte_len=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
        decode="valid_utf8", decode_error=None, scalars=[], subject=case_id, relation=None,
        lines=[], boundaries=[], parser_points=[], parser_queries=[], wire=[], refusals=[],
        chunks=["contiguous", "all_byte_cuts", "empty_chunks"],
    )
    try:
        source = raw.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        kind = "truncated" if "unexpected end" in error.reason else "invalid_continuation" if "continuation" in error.reason else "invalid_leading"
        item.update(decode="invalid_utf8_ingress", decode_error=dict(kind=kind, start=error.start, end=error.end), subject=None, chunks=[])
        return item
    item["scalars"] = [ord(char) for char in source]
    cursor = 0
    for char in source:
        item["boundaries"].append(cursor)
        cursor += len(char.encode())
    item["boundaries"].append(cursor)
    start = 0
    for offset, value in enumerate(raw):
        if value == 10:
            crlf = offset > start and raw[offset - 1] == 13
            item["lines"].append(dict(start=start, content_end=offset - int(crlf), separator_end=offset + 1, separator="crlf" if crlf else "lf"))
            start = offset + 1
    item["lines"].append(dict(start=start, content_end=len(raw), separator_end=len(raw), separator="none"))
    for offset in item["boundaries"]:
        row = next((i for i, line in enumerate(item["lines"]) if offset < line["separator_end"]), len(item["lines"]) - 1)
        item["parser_points"].append(dict(byte=offset, row=row, column=offset - item["lines"][row]["start"]))
    for row, line in enumerate(item["lines"]):
        content = raw[line["start"]:line["content_end"]].decode()
        byte_col = 0
        utf16_col = 0
        for char in [*content, None]:
            for encoding, col in (("utf-8", byte_col), ("utf-16", utf16_col)):
                for direction in ("incoming", "outgoing"):
                    item["wire"].append(dict(encoding=encoding, direction=direction, line=row, column=col, disposition="exact", byte=line["start"] + byte_col))
            if char is not None:
                for col in range(byte_col + 1, byte_col + len(char.encode())):
                    item["wire"].append(dict(encoding="utf-8", direction="incoming", line=row, column=col, disposition="invalid_code_unit_boundary", byte=None))
                for col in range(utf16_col + 1, utf16_col + len(char.encode("utf-16-le")) // 2):
                    item["wire"].append(dict(encoding="utf-16", direction="incoming", line=row, column=col, disposition="invalid_code_unit_boundary", byte=None))
                byte_col += len(char.encode())
                utf16_col += len(char.encode("utf-16-le")) // 2
        for encoding, col in (("utf-8", byte_col + 1), ("utf-16", utf16_col + 1)):
            item["wire"].append(dict(encoding=encoding, direction="incoming", line=row, column=col, disposition="line_end_normalized", byte=line["content_end"]))
        if line["separator"] == "crlf":
            for encoding, col in (("utf-8", byte_col + 1), ("utf-16", utf16_col + 1)):
                item["wire"].append(dict(encoding=encoding, direction="outgoing", line=row, column=col, disposition="invalid_separator_interior", byte=None))
    for encoding in ("utf-8", "utf-16"):
        item["wire"].append(dict(encoding=encoding, direction="incoming", line=len(item["lines"]), column=0, disposition="invalid_line", byte=None))
    item["refusals"] = [
        dict(kind="equal_range", start=0, end=0, disposition="exact"),
        dict(kind="source_byte_out_of_bounds", start=len(raw) + 1, end=None, disposition="invalid_source_byte"),
        dict(kind="wrong_source", start=None, end=None, disposition="source_mismatch"),
        dict(kind="wrong_signature", start=None, end=None, disposition="signature_mismatch"),
        dict(kind="wrong_schema", start=None, end=None, disposition="schema_mismatch"),
        dict(kind="overflow_resource", start=None, end=None, disposition="overflow_or_resource_boundary"),
        dict(kind="instrument_failure", start=None, end=None, disposition="instrument_failure"),
    ]
    if raw:
        item["refusals"].append(dict(kind="reversed_range", start=1, end=0, disposition="invalid_range_order"))
    crlf = next((line for line in item["lines"] if line["separator"] == "crlf"), None)
    if crlf:
        item["refusals"].append(dict(kind="separator_interior", start=crlf["content_end"] + 1, end=None, disposition="invalid_newline_boundary"))
    interior = next((offset for offset in range(len(raw)) if offset not in item["boundaries"]), None)
    if interior is not None:
        item["refusals"].append(dict(kind="scalar_interior", start=interior, end=None, disposition="invalid_code_unit_boundary"))
    all_boundaries = item["boundaries"][:]
    if not item["exhaustive"]:
        item["boundaries"] = sorted(set(offset for offset in (0, 1, len(raw) // 2, max(0, len(raw) - 1), len(raw)) if offset in item["boundaries"]))
        item["parser_points"] = [point for point in item["parser_points"] if point["byte"] in item["boundaries"]]
        last = len(item["lines"]) - 1
        def selected(fact):
            row = fact["line"]
            if row not in (0, len(item["lines"]) // 2, last, len(item["lines"])):
                return False
            if row == len(item["lines"]):
                return True
            line = item["lines"][row]
            content = raw[line["start"]:line["content_end"]].decode()
            end = len(content.encode()) if fact["encoding"] == "utf-8" else len(content.encode("utf-16-le")) // 2
            return fact["column"] in (0, end // 2, end, end + 1)
        item["wire"] = [fact for fact in item["wire"] if selected(fact)]
    item["parser_queries"] = [dict(row=point["row"], column=point["column"], disposition="exact", byte=point["byte"]) for point in item["parser_points"]]
    for row, line in enumerate(item["lines"]):
        if line["separator"] != "none":
            item["parser_queries"].append(dict(row=row, column=line["separator_end"] - line["start"], disposition="old_row_noncanonical", byte=None))
        for byte in range(line["start"], line["separator_end"]):
            if byte not in all_boundaries:
                item["parser_queries"].append(dict(row=row, column=byte - line["start"], disposition="invalid_byte_boundary", byte=None))
    item["parser_queries"].append(dict(row=len(item["lines"]), column=0, disposition="invalid_row", byte=None))
    return item


def main():
    manifest = dict(schema_version="position-fixtures/v1", policy_id="lf-source-lines/v1", cases=[expected_case(*case) for case in sorted(CASES)])
    by_id = {case["id"]: case for case in manifest["cases"]}
    # These are independent worked facts from ADR-0048 and the issue. Keep them
    # visible here so a generator change cannot silently rewrite the decisive row.
    decisive = by_id["astral"]
    assert decisive["byte_len"] == 9
    assert decisive["lines"] == [
        dict(start=0, content_end=6, separator_end=8, separator="crlf"),
        dict(start=8, content_end=9, separator_end=9, separator="none"),
    ]
    assert dict(byte=7, row=0, column=7) in decisive["parser_points"]
    assert dict(encoding="utf-8", direction="outgoing", line=0, column=7, disposition="invalid_separator_interior", byte=None) in decisive["wire"]
    assert by_id["terminal_lf"]["lines"] == [
        dict(start=0, content_end=1, separator_end=2, separator="lf"),
        dict(start=2, content_end=2, separator_end=2, separator="none"),
    ]
    assert by_id["ls"]["raw_hex"] == "61e280a862"
    assert by_id["ls"]["lines"] == [dict(start=0, content_end=5, separator_end=5, separator="none")]
    for source_id, target_id in RELATIONS.items():
        source = by_id[source_id]
        ranges = [
            dict(source_start=0, source_end=3, target_start=None, target_end=None, disposition="elided"),
            dict(source_start=3, source_end=3, target_start=0, target_end=0, disposition="exact"),
            dict(source_start=3, source_end=source["byte_len"], target_start=0, target_end=by_id[target_id]["byte_len"], disposition="exact"),
        ]
        cross_end = next((boundary for boundary in source["boundaries"] if boundary > 3), None)
        if cross_end is not None:
            ranges.append(dict(source_start=0, source_end=cross_end, target_start=0, target_end=cross_end - 3, disposition="crosses_elided_prefix"))
        source["relation"] = dict(target=target_id, kind="strip_leading_bom", boundary_map=[dict(source=boundary, target=boundary - 3 if boundary >= 3 else None) for boundary in range(source["byte_len"] + 1)], ranges=ranges)
    assert by_id["bom_ascii"]["raw_hex"] == "efbbbf616263"
    assert by_id["bom_ascii"]["relation"]["boundary_map"][1:4] == [
        dict(source=1, target=None), dict(source=2, target=None), dict(source=3, target=0),
    ]
    for identity_id in ("ascii", "nonleading_bom", "bom_after_lf"):
        identity = by_id[identity_id]
        identity["relation"] = dict(target=identity_id, kind="identity", boundary_map=[dict(source=boundary, target=boundary) for boundary in range(identity["byte_len"] + 1)], ranges=[dict(source_start=0, source_end=identity["byte_len"], target_start=0, target_end=identity["byte_len"], disposition="exact")])
    output = HERE / "position-fixtures.v1.json"
    output.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
