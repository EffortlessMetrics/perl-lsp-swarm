# Source coordinate fixtures v1

Generated from `crates/perl-position-fixtures/fixtures/position-fixtures.v1.json`. Do not edit. ADR-0048 (`lf-source-lines/v1`) governs LF rows. Invalid UTF-8 is ingress evidence only.

## `ascii`

Tags: basic, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

Hex: `616263`

Relation: `identity` to `ascii`; boundaries: 0→0, 1→1, 2→2, 3→3; ranges: 0..3 `exact`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `ascii_for_bom`

Tags: bom, parser_subject, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

Hex: `616263`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `astral`

Tags: unicode, crlf, small, decisive. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 9. SHA-256: `587527b0f4f0a1573965d9e6c295f0ce1fd3250fc1e7d2bd81bb6db986782b40`.

Hex: `78f09f9880790d0a7a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 6 | 8 | crlf |
| 1 | 8 | 9 | 9 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 4 | invalid_code_unit_boundary | — |
| incoming | utf-16 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | exact | 6 |
| outgoing | utf-8 | 0 | 6 | exact | 6 |
| incoming | utf-16 | 0 | 4 | exact | 6 |
| outgoing | utf-16 | 0 | 4 | exact | 6 |
| incoming | utf-8 | 0 | 7 | line_end_normalized | 6 |
| incoming | utf-16 | 0 | 5 | line_end_normalized | 6 |
| outgoing | utf-8 | 0 | 7 | invalid_separator_interior | — |
| outgoing | utf-16 | 0 | 5 | invalid_separator_interior | — |
| incoming | utf-8 | 1 | 0 | exact | 8 |
| outgoing | utf-8 | 1 | 0 | exact | 8 |
| incoming | utf-16 | 1 | 0 | exact | 8 |
| outgoing | utf-16 | 1 | 0 | exact | 8 |
| incoming | utf-8 | 1 | 1 | exact | 9 |
| outgoing | utf-8 | 1 | 1 | exact | 9 |
| incoming | utf-16 | 1 | 1 | exact | 9 |
| outgoing | utf-16 | 1 | 1 | exact | 9 |
| incoming | utf-8 | 1 | 2 | line_end_normalized | 9 |
| incoming | utf-16 | 1 | 2 | line_end_normalized | 9 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 10 of 12; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:5` `exact`, `0:6` `exact`, `0:7` `exact`, `0:8` `old_row_noncanonical`, `0:2` `invalid_byte_boundary`, `0:3` `invalid_byte_boundary`, `0:4` `invalid_byte_boundary`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `separator_interior` → `invalid_newline_boundary`, `scalar_interior` → `invalid_code_unit_boundary`.

## `bare_cr`

Tags: newline, bare_cr, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `af9081672dd5ef3247a30c2db5b0dafcc9bcf981a26aefb3c55d210d43fcc14e`.

Hex: `610d62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `bmp`

Tags: unicode, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 7. SHA-256: `b67ff668c6a00a3cf8df751e8bea1c379ba792c621b49ac70d95557481252ea8`.

Hex: `61c3a9e4b8ad7a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 7 | 7 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 2 | exact | 3 |
| outgoing | utf-16 | 0 | 2 | exact | 3 |
| incoming | utf-8 | 0 | 4 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 5 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 6 | exact | 6 |
| outgoing | utf-8 | 0 | 6 | exact | 6 |
| incoming | utf-16 | 0 | 3 | exact | 6 |
| outgoing | utf-16 | 0 | 3 | exact | 6 |
| incoming | utf-8 | 0 | 7 | exact | 7 |
| outgoing | utf-8 | 0 | 7 | exact | 7 |
| incoming | utf-16 | 0 | 4 | exact | 7 |
| outgoing | utf-16 | 0 | 4 | exact | 7 |
| incoming | utf-8 | 0 | 8 | line_end_normalized | 7 |
| incoming | utf-16 | 0 | 5 | line_end_normalized | 7 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 9 of 9; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:3` `exact`, `0:6` `exact`, `0:7` `exact`, `0:2` `invalid_byte_boundary`, `0:4` `invalid_byte_boundary`, `0:5` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `bom_after_lf`

Tags: bom, newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 6. SHA-256: `432c618116b12a567487d0890833e7333fc04f00b05f12c5b015ee33cd6b9d18`.

Hex: `610aefbbbf62`

Relation: `identity` to `bom_after_lf`; boundaries: 0→0, 1→1, 2→2, 3→3, 4→4, 5→5, 6→6; ranges: 0..6 `exact`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 6 | 6 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 1 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 1 | 3 | exact | 5 |
| outgoing | utf-8 | 1 | 3 | exact | 5 |
| incoming | utf-16 | 1 | 1 | exact | 5 |
| outgoing | utf-16 | 1 | 1 | exact | 5 |
| incoming | utf-8 | 1 | 4 | exact | 6 |
| outgoing | utf-8 | 1 | 4 | exact | 6 |
| incoming | utf-16 | 1 | 2 | exact | 6 |
| outgoing | utf-16 | 1 | 2 | exact | 6 |
| incoming | utf-8 | 1 | 5 | line_end_normalized | 6 |
| incoming | utf-16 | 1 | 3 | line_end_normalized | 6 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 6 of 9; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `1:1` `invalid_byte_boundary`, `1:2` `invalid_byte_boundary`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `bom_ascii`

Tags: bom, presentation, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 6. SHA-256: `1c28dc3f1f804a1ad9c9b4b4cf5e2658d16ad4ed08e3020d04a8d2865018947c`.

Hex: `efbbbf616263`

Relation: `strip_leading_bom` to `ascii_for_bom`; boundaries: 0→elided, 1→elided, 2→elided, 3→0, 4→1, 5→2, 6→3; ranges: 0..3 `elided`, 3..3 `exact`, 3..6 `exact`, 0..4 `crosses_elided_prefix`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 6 | 6 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 1 | exact | 3 |
| outgoing | utf-16 | 0 | 1 | exact | 3 |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | exact | 6 |
| outgoing | utf-8 | 0 | 6 | exact | 6 |
| incoming | utf-16 | 0 | 4 | exact | 6 |
| outgoing | utf-16 | 0 | 4 | exact | 6 |
| incoming | utf-8 | 0 | 7 | line_end_normalized | 6 |
| incoming | utf-16 | 0 | 5 | line_end_normalized | 6 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 8 of 8; full facts in machine authority): `0:0` `exact`, `0:3` `exact`, `0:4` `exact`, `0:5` `exact`, `0:6` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `bom_only`

Tags: bom, presentation, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `f1945cd6c19e56b3c1c78943ef5ec18116907a4ca1efc40a57d48ab1db7adfc5`.

Hex: `efbbbf`

Relation: `strip_leading_bom` to `empty_for_bom`; boundaries: 0→elided, 1→elided, 2→elided, 3→0; ranges: 0..3 `elided`, 3..3 `exact`, 3..3 `exact`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 1 | exact | 3 |
| outgoing | utf-16 | 0 | 1 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:3` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `bom_unicode`

Tags: bom, presentation, unicode, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 8. SHA-256: `31be38e7105468a46885dbb8523dfc46065c18c46c489e9da33cff7d6171f22c`.

Hex: `efbbbff09f988078`

Relation: `strip_leading_bom` to `unicode_for_bom`; boundaries: 0→elided, 1→elided, 2→elided, 3→0, 4→1, 5→2, 6→3, 7→4, 8→5; ranges: 0..3 `elided`, 3..3 `exact`, 3..8 `exact`, 0..7 `crosses_elided_prefix`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 8 | 8 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 1 | exact | 3 |
| outgoing | utf-16 | 0 | 1 | exact | 3 |
| incoming | utf-8 | 0 | 4 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 5 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 6 | invalid_code_unit_boundary | — |
| incoming | utf-16 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 7 | exact | 7 |
| outgoing | utf-8 | 0 | 7 | exact | 7 |
| incoming | utf-16 | 0 | 3 | exact | 7 |
| outgoing | utf-16 | 0 | 3 | exact | 7 |
| incoming | utf-8 | 0 | 8 | exact | 8 |
| outgoing | utf-8 | 0 | 8 | exact | 8 |
| incoming | utf-16 | 0 | 4 | exact | 8 |
| outgoing | utf-16 | 0 | 4 | exact | 8 |
| incoming | utf-8 | 0 | 9 | line_end_normalized | 8 |
| incoming | utf-16 | 0 | 5 | line_end_normalized | 8 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 10 of 10; full facts in machine authority): `0:0` `exact`, `0:3` `exact`, `0:7` `exact`, `0:8` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `0:4` `invalid_byte_boundary`, `0:5` `invalid_byte_boundary`, `0:6` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `combining`

Tags: unicode, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `d49168b35af0fe6d2e78b39e517c0be5fc7639c3158769a5894f653ca8798575`.

Hex: `61cc8162`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 2 | exact | 3 |
| outgoing | utf-16 | 0 | 2 | exact | 3 |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 3 | exact | 4 |
| outgoing | utf-16 | 0 | 3 | exact | 4 |
| incoming | utf-8 | 0 | 5 | line_end_normalized | 4 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 4 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 6 of 6; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:3` `exact`, `0:4` `exact`, `0:2` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `consecutive_lf`

Tags: newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `38022fd2b8dbc5cb3d2cee74e083edbf59e3d4e13d067ebcb5db633d4cff4d8c`.

Hex: `610a0a62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 2 | 3 | lf |
| 2 | 3 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-16 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-8 | 2 | 0 | exact | 3 |
| outgoing | utf-8 | 2 | 0 | exact | 3 |
| incoming | utf-16 | 2 | 0 | exact | 3 |
| outgoing | utf-16 | 2 | 0 | exact | 3 |
| incoming | utf-8 | 2 | 1 | exact | 4 |
| outgoing | utf-8 | 2 | 1 | exact | 4 |
| incoming | utf-16 | 2 | 1 | exact | 4 |
| outgoing | utf-16 | 2 | 1 | exact | 4 |
| incoming | utf-8 | 2 | 2 | line_end_normalized | 4 |
| incoming | utf-16 | 2 | 2 | line_end_normalized | 4 |
| incoming | utf-8 | 3 | 0 | invalid_line | — |
| incoming | utf-16 | 3 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 8; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `1:1` `old_row_noncanonical`, `3:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `crcrlf`

Tags: newline, crlf, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 5. SHA-256: `5e63341b6816df299c0c6d61e6ec4a5168ee01f279feb39dbeb2c3784f31ed94`.

Hex: `610d0d0a62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 2 | 4 | crlf |
| 1 | 4 | 5 | 5 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | line_end_normalized | 2 |
| incoming | utf-16 | 0 | 3 | line_end_normalized | 2 |
| outgoing | utf-8 | 0 | 3 | invalid_separator_interior | — |
| outgoing | utf-16 | 0 | 3 | invalid_separator_interior | — |
| incoming | utf-8 | 1 | 0 | exact | 4 |
| outgoing | utf-8 | 1 | 0 | exact | 4 |
| incoming | utf-16 | 1 | 0 | exact | 4 |
| outgoing | utf-16 | 1 | 0 | exact | 4 |
| incoming | utf-8 | 1 | 1 | exact | 5 |
| outgoing | utf-8 | 1 | 1 | exact | 5 |
| incoming | utf-16 | 1 | 1 | exact | 5 |
| outgoing | utf-16 | 1 | 1 | exact | 5 |
| incoming | utf-8 | 1 | 2 | line_end_normalized | 5 |
| incoming | utf-16 | 1 | 2 | line_end_normalized | 5 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 6 of 8; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `0:4` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `separator_interior` → `invalid_newline_boundary`.

## `crlf`

Tags: newline, crlf, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `18745f36a05e29072709042d6062ce54f1b08ff36c27ba80c39f81fb010c8ce2`.

Hex: `610d0a62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 3 | crlf |
| 1 | 3 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| outgoing | utf-8 | 0 | 2 | invalid_separator_interior | — |
| outgoing | utf-16 | 0 | 2 | invalid_separator_interior | — |
| incoming | utf-8 | 1 | 0 | exact | 3 |
| outgoing | utf-8 | 1 | 0 | exact | 3 |
| incoming | utf-16 | 1 | 0 | exact | 3 |
| outgoing | utf-16 | 1 | 0 | exact | 3 |
| incoming | utf-8 | 1 | 1 | exact | 4 |
| outgoing | utf-8 | 1 | 1 | exact | 4 |
| incoming | utf-16 | 1 | 1 | exact | 4 |
| outgoing | utf-16 | 1 | 1 | exact | 4 |
| incoming | utf-8 | 1 | 2 | line_end_normalized | 4 |
| incoming | utf-16 | 1 | 2 | line_end_normalized | 4 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `separator_interior` → `invalid_newline_boundary`.

## `double_bom`

Tags: bom, presentation, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 7. SHA-256: `aa9d9d8ad7a736cb08a7f3fff0e5429898dbfa9abefe332f3c598f75c63ae0e5`.

Hex: `efbbbfefbbbf78`

Relation: `strip_leading_bom` to `single_bom_for_double`; boundaries: 0→elided, 1→elided, 2→elided, 3→0, 4→1, 5→2, 6→3, 7→4; ranges: 0..3 `elided`, 3..3 `exact`, 3..7 `exact`, 0..6 `crosses_elided_prefix`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 7 | 7 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 1 | exact | 3 |
| outgoing | utf-16 | 0 | 1 | exact | 3 |
| incoming | utf-8 | 0 | 4 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 5 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 6 | exact | 6 |
| outgoing | utf-8 | 0 | 6 | exact | 6 |
| incoming | utf-16 | 0 | 2 | exact | 6 |
| outgoing | utf-16 | 0 | 2 | exact | 6 |
| incoming | utf-8 | 0 | 7 | exact | 7 |
| outgoing | utf-8 | 0 | 7 | exact | 7 |
| incoming | utf-16 | 0 | 3 | exact | 7 |
| outgoing | utf-16 | 0 | 3 | exact | 7 |
| incoming | utf-8 | 0 | 8 | line_end_normalized | 7 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 7 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 9 of 9; full facts in machine authority): `0:0` `exact`, `0:3` `exact`, `0:6` `exact`, `0:7` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `0:4` `invalid_byte_boundary`, `0:5` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `empty`

Tags: basic, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 0. SHA-256: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

Hex: ``

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 0 | 0 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-16 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 2 of 2; full facts in machine authority): `0:0` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`.

## `empty_for_bom`

Tags: bom, parser_subject, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 0. SHA-256: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

Hex: ``

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 0 | 0 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-16 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 2 of 2; full facts in machine authority): `0:0` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`.

## `ff`

Tags: ropey_control, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `1fde8ba60c8ea5cd122bedc76b4ae87973874bf8e766e565838fd013f06da4b5`.

Hex: `610c62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `invalid_continuation_end`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 4. SHA-256: `542be57ffeb637e36586989ba240654cb5363c31b34e2673613b2980b2798607`.

Hex: `6162e241`

Decoder error: `invalid_continuation` at bytes 2..3.

## `invalid_continuation_middle`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 4. SHA-256: `68eac75d130fdea70946f0e5cbd81b9c9186bedc8712b2255ad618ac0479f75a`.

Hex: `61e24162`

Decoder error: `invalid_continuation` at bytes 1..2.

## `invalid_continuation_start`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 5. SHA-256: `e03d63696b1aca6cf71df1bbb023c1c4eb52283ca14ce3a9d150616e0e005bf0`.

Hex: `e241616263`

Decoder error: `invalid_continuation` at bytes 0..1.

## `invalid_leading_end`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 2. SHA-256: `8dd06b5ab6b594257e41b7d8dd440a4062eddc67fdab5c13b4dc300176896f6e`.

Hex: `61ff`

Decoder error: `invalid_leading` at bytes 1..2.

## `invalid_leading_middle`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 3. SHA-256: `01ce0241d2a0e71a4fecd5a8d71157fe2787197732fc15d889cbcf36c38e3c68`.

Hex: `61ff62`

Decoder error: `invalid_leading` at bytes 1..2.

## `invalid_leading_start`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 2. SHA-256: `6446546190caef8efbaf9e738e2fe8ac30583ec4bf2c0f6468f8c4a47bda0e91`.

Hex: `ff61`

Decoder error: `invalid_leading` at bytes 0..1.

## `invalid_truncated_end`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 4. SHA-256: `a28b8ebb80799713d3da397737778ac0c1c756683b0b3e22338ea20b95adcafc`.

Hex: `6162e282`

Decoder error: `truncated` at bytes 2..4.

## `invalid_truncated_middle`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 2. SHA-256: `548c8bb0512688bbaf64ccd7f7814d127ee122d170f3d4e42c00f73471d038cb`.

Hex: `61e2`

Decoder error: `truncated` at bytes 1..2.

## `invalid_truncated_start`

Tags: invalid_utf8, ingress. Coverage: `all small-case boundaries`. Decode: `invalid_utf8_ingress`. Bytes: 1. SHA-256: `30a5bfa58e128af9e5a4955725d8ad26d4d574a537b58b7dc6d357acad578572`.

Hex: `e2`

Decoder error: `truncated` at bytes 0..1.

## `large_line`

Tags: generated_boundary, newline. Coverage: `selected large-boundary probes`. Decode: `valid_utf8`. Bytes: 2049. SHA-256: `80456798a4ccb2faa38e49e3d6740c4d417431bb9e682de2a29e71ff77c2ae7c`.

Hex: `78787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878787878780a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 2048 | 2049 | lf |
| 1 | 2049 | 2049 | 2049 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1024 | exact | 1024 |
| outgoing | utf-8 | 0 | 1024 | exact | 1024 |
| incoming | utf-16 | 0 | 1024 | exact | 1024 |
| outgoing | utf-16 | 0 | 1024 | exact | 1024 |
| incoming | utf-8 | 0 | 2048 | exact | 2048 |
| outgoing | utf-8 | 0 | 2048 | exact | 2048 |
| incoming | utf-16 | 0 | 2048 | exact | 2048 |
| outgoing | utf-16 | 0 | 2048 | exact | 2048 |
| incoming | utf-8 | 0 | 2049 | line_end_normalized | 2048 |
| incoming | utf-16 | 0 | 2049 | line_end_normalized | 2048 |
| incoming | utf-8 | 1 | 0 | exact | 2049 |
| outgoing | utf-8 | 1 | 0 | exact | 2049 |
| incoming | utf-16 | 1 | 0 | exact | 2049 |
| outgoing | utf-16 | 1 | 0 | exact | 2049 |
| incoming | utf-8 | 1 | 1 | line_end_normalized | 2049 |
| incoming | utf-16 | 1 | 1 | line_end_normalized | 2049 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 4 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2049` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `lf`

Tags: newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78`.

Hex: `610a62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | exact | 3 |
| outgoing | utf-8 | 1 | 1 | exact | 3 |
| incoming | utf-16 | 1 | 1 | exact | 3 |
| outgoing | utf-16 | 1 | 1 | exact | 3 |
| incoming | utf-8 | 1 | 2 | line_end_normalized | 3 |
| incoming | utf-16 | 1 | 2 | line_end_normalized | 3 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 4 of 6; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `lfcr`

Tags: newline, bare_cr, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `af2629b401d17245f7e5b6822e30cc588a4034c44c6d3265a26a6af81628aab0`.

Hex: `610a0d62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | exact | 3 |
| outgoing | utf-8 | 1 | 1 | exact | 3 |
| incoming | utf-16 | 1 | 1 | exact | 3 |
| outgoing | utf-16 | 1 | 1 | exact | 3 |
| incoming | utf-8 | 1 | 2 | exact | 4 |
| outgoing | utf-8 | 1 | 2 | exact | 4 |
| incoming | utf-16 | 1 | 2 | exact | 4 |
| outgoing | utf-16 | 1 | 2 | exact | 4 |
| incoming | utf-8 | 1 | 3 | line_end_normalized | 4 |
| incoming | utf-16 | 1 | 3 | line_end_normalized | 4 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 4 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `ls`

Tags: ropey_control, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 5. SHA-256: `fe2d3b945530c806f1ff5298f4486e3f1d2656c1bb026709f785a5f84d23af64`.

Hex: `61e280a862`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 5 | 5 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | line_end_normalized | 5 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 5 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 7 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:4` `exact`, `0:5` `exact`, `0:2` `invalid_byte_boundary`, `0:3` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `many_lines`

Tags: generated_boundary, newline. Coverage: `selected large-boundary probes`. Decode: `valid_utf8`. Bytes: 256. SHA-256: `9ffebbffc3bb5d28ef8b7658373084b2204ee0e177e0d1d47b3b96d146f3467c`.

Hex: `780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a780a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 3 | 4 | lf |
| 2 | 4 | 5 | 6 | lf |
| 3 | 6 | 7 | 8 | lf |
| 4 | 8 | 9 | 10 | lf |
| 5 | 10 | 11 | 12 | lf |
| 6 | 12 | 13 | 14 | lf |
| 7 | 14 | 15 | 16 | lf |
| 8 | 16 | 17 | 18 | lf |
| 9 | 18 | 19 | 20 | lf |
| 10 | 20 | 21 | 22 | lf |
| 11 | 22 | 23 | 24 | lf |
| 12 | 24 | 25 | 26 | lf |
| 13 | 26 | 27 | 28 | lf |
| 14 | 28 | 29 | 30 | lf |
| 15 | 30 | 31 | 32 | lf |
| 16 | 32 | 33 | 34 | lf |
| 17 | 34 | 35 | 36 | lf |
| 18 | 36 | 37 | 38 | lf |
| 19 | 38 | 39 | 40 | lf |
| 20 | 40 | 41 | 42 | lf |
| 21 | 42 | 43 | 44 | lf |
| 22 | 44 | 45 | 46 | lf |
| 23 | 46 | 47 | 48 | lf |
| 24 | 48 | 49 | 50 | lf |
| 25 | 50 | 51 | 52 | lf |
| 26 | 52 | 53 | 54 | lf |
| 27 | 54 | 55 | 56 | lf |
| 28 | 56 | 57 | 58 | lf |
| 29 | 58 | 59 | 60 | lf |
| 30 | 60 | 61 | 62 | lf |
| 31 | 62 | 63 | 64 | lf |
| 32 | 64 | 65 | 66 | lf |
| 33 | 66 | 67 | 68 | lf |
| 34 | 68 | 69 | 70 | lf |
| 35 | 70 | 71 | 72 | lf |
| 36 | 72 | 73 | 74 | lf |
| 37 | 74 | 75 | 76 | lf |
| 38 | 76 | 77 | 78 | lf |
| 39 | 78 | 79 | 80 | lf |
| 40 | 80 | 81 | 82 | lf |
| 41 | 82 | 83 | 84 | lf |
| 42 | 84 | 85 | 86 | lf |
| 43 | 86 | 87 | 88 | lf |
| 44 | 88 | 89 | 90 | lf |
| 45 | 90 | 91 | 92 | lf |
| 46 | 92 | 93 | 94 | lf |
| 47 | 94 | 95 | 96 | lf |
| 48 | 96 | 97 | 98 | lf |
| 49 | 98 | 99 | 100 | lf |
| 50 | 100 | 101 | 102 | lf |
| 51 | 102 | 103 | 104 | lf |
| 52 | 104 | 105 | 106 | lf |
| 53 | 106 | 107 | 108 | lf |
| 54 | 108 | 109 | 110 | lf |
| 55 | 110 | 111 | 112 | lf |
| 56 | 112 | 113 | 114 | lf |
| 57 | 114 | 115 | 116 | lf |
| 58 | 116 | 117 | 118 | lf |
| 59 | 118 | 119 | 120 | lf |
| 60 | 120 | 121 | 122 | lf |
| 61 | 122 | 123 | 124 | lf |
| 62 | 124 | 125 | 126 | lf |
| 63 | 126 | 127 | 128 | lf |
| 64 | 128 | 129 | 130 | lf |
| 65 | 130 | 131 | 132 | lf |
| 66 | 132 | 133 | 134 | lf |
| 67 | 134 | 135 | 136 | lf |
| 68 | 136 | 137 | 138 | lf |
| 69 | 138 | 139 | 140 | lf |
| 70 | 140 | 141 | 142 | lf |
| 71 | 142 | 143 | 144 | lf |
| 72 | 144 | 145 | 146 | lf |
| 73 | 146 | 147 | 148 | lf |
| 74 | 148 | 149 | 150 | lf |
| 75 | 150 | 151 | 152 | lf |
| 76 | 152 | 153 | 154 | lf |
| 77 | 154 | 155 | 156 | lf |
| 78 | 156 | 157 | 158 | lf |
| 79 | 158 | 159 | 160 | lf |
| 80 | 160 | 161 | 162 | lf |
| 81 | 162 | 163 | 164 | lf |
| 82 | 164 | 165 | 166 | lf |
| 83 | 166 | 167 | 168 | lf |
| 84 | 168 | 169 | 170 | lf |
| 85 | 170 | 171 | 172 | lf |
| 86 | 172 | 173 | 174 | lf |
| 87 | 174 | 175 | 176 | lf |
| 88 | 176 | 177 | 178 | lf |
| 89 | 178 | 179 | 180 | lf |
| 90 | 180 | 181 | 182 | lf |
| 91 | 182 | 183 | 184 | lf |
| 92 | 184 | 185 | 186 | lf |
| 93 | 186 | 187 | 188 | lf |
| 94 | 188 | 189 | 190 | lf |
| 95 | 190 | 191 | 192 | lf |
| 96 | 192 | 193 | 194 | lf |
| 97 | 194 | 195 | 196 | lf |
| 98 | 196 | 197 | 198 | lf |
| 99 | 198 | 199 | 200 | lf |
| 100 | 200 | 201 | 202 | lf |
| 101 | 202 | 203 | 204 | lf |
| 102 | 204 | 205 | 206 | lf |
| 103 | 206 | 207 | 208 | lf |
| 104 | 208 | 209 | 210 | lf |
| 105 | 210 | 211 | 212 | lf |
| 106 | 212 | 213 | 214 | lf |
| 107 | 214 | 215 | 216 | lf |
| 108 | 216 | 217 | 218 | lf |
| 109 | 218 | 219 | 220 | lf |
| 110 | 220 | 221 | 222 | lf |
| 111 | 222 | 223 | 224 | lf |
| 112 | 224 | 225 | 226 | lf |
| 113 | 226 | 227 | 228 | lf |
| 114 | 228 | 229 | 230 | lf |
| 115 | 230 | 231 | 232 | lf |
| 116 | 232 | 233 | 234 | lf |
| 117 | 234 | 235 | 236 | lf |
| 118 | 236 | 237 | 238 | lf |
| 119 | 238 | 239 | 240 | lf |
| 120 | 240 | 241 | 242 | lf |
| 121 | 242 | 243 | 244 | lf |
| 122 | 244 | 245 | 246 | lf |
| 123 | 246 | 247 | 248 | lf |
| 124 | 248 | 249 | 250 | lf |
| 125 | 250 | 251 | 252 | lf |
| 126 | 252 | 253 | 254 | lf |
| 127 | 254 | 255 | 256 | lf |
| 128 | 256 | 256 | 256 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 64 | 0 | exact | 128 |
| outgoing | utf-8 | 64 | 0 | exact | 128 |
| incoming | utf-16 | 64 | 0 | exact | 128 |
| outgoing | utf-16 | 64 | 0 | exact | 128 |
| incoming | utf-8 | 64 | 1 | exact | 129 |
| outgoing | utf-8 | 64 | 1 | exact | 129 |
| incoming | utf-16 | 64 | 1 | exact | 129 |
| outgoing | utf-16 | 64 | 1 | exact | 129 |
| incoming | utf-8 | 64 | 2 | line_end_normalized | 129 |
| incoming | utf-16 | 64 | 2 | line_end_normalized | 129 |
| incoming | utf-8 | 128 | 0 | exact | 256 |
| outgoing | utf-8 | 128 | 0 | exact | 256 |
| incoming | utf-16 | 128 | 0 | exact | 256 |
| outgoing | utf-16 | 128 | 0 | exact | 256 |
| incoming | utf-8 | 128 | 1 | line_end_normalized | 256 |
| incoming | utf-16 | 128 | 1 | line_end_normalized | 256 |
| incoming | utf-8 | 129 | 0 | invalid_line | — |
| incoming | utf-16 | 129 | 0 | invalid_line | — |

Parser point controls (showing first 12 of 134; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `1:2` `old_row_noncanonical`, `2:2` `old_row_noncanonical`, `3:2` `old_row_noncanonical`, `4:2` `old_row_noncanonical`, `5:2` `old_row_noncanonical`, `6:2` `old_row_noncanonical`, `7:2` `old_row_noncanonical`, `8:2` `old_row_noncanonical`, `9:2` `old_row_noncanonical`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `mixed`

Tags: newline, crlf, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 9. SHA-256: `01d51db6c53828452bdbc06268f0000891729ab0bd8d743998cc7f14dc6fa7db`.

Hex: `610d0a620a630d0a64`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 3 | crlf |
| 1 | 3 | 4 | 5 | lf |
| 2 | 5 | 6 | 8 | crlf |
| 3 | 8 | 9 | 9 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| outgoing | utf-8 | 0 | 2 | invalid_separator_interior | — |
| outgoing | utf-16 | 0 | 2 | invalid_separator_interior | — |
| incoming | utf-8 | 1 | 0 | exact | 3 |
| outgoing | utf-8 | 1 | 0 | exact | 3 |
| incoming | utf-16 | 1 | 0 | exact | 3 |
| outgoing | utf-16 | 1 | 0 | exact | 3 |
| incoming | utf-8 | 1 | 1 | exact | 4 |
| outgoing | utf-8 | 1 | 1 | exact | 4 |
| incoming | utf-16 | 1 | 1 | exact | 4 |
| outgoing | utf-16 | 1 | 1 | exact | 4 |
| incoming | utf-8 | 1 | 2 | line_end_normalized | 4 |
| incoming | utf-16 | 1 | 2 | line_end_normalized | 4 |
| incoming | utf-8 | 2 | 0 | exact | 5 |
| outgoing | utf-8 | 2 | 0 | exact | 5 |
| incoming | utf-16 | 2 | 0 | exact | 5 |
| outgoing | utf-16 | 2 | 0 | exact | 5 |
| incoming | utf-8 | 2 | 1 | exact | 6 |
| outgoing | utf-8 | 2 | 1 | exact | 6 |
| incoming | utf-16 | 2 | 1 | exact | 6 |
| outgoing | utf-16 | 2 | 1 | exact | 6 |
| incoming | utf-8 | 2 | 2 | line_end_normalized | 6 |
| incoming | utf-16 | 2 | 2 | line_end_normalized | 6 |
| outgoing | utf-8 | 2 | 2 | invalid_separator_interior | — |
| outgoing | utf-16 | 2 | 2 | invalid_separator_interior | — |
| incoming | utf-8 | 3 | 0 | exact | 8 |
| outgoing | utf-8 | 3 | 0 | exact | 8 |
| incoming | utf-16 | 3 | 0 | exact | 8 |
| outgoing | utf-16 | 3 | 0 | exact | 8 |
| incoming | utf-8 | 3 | 1 | exact | 9 |
| outgoing | utf-8 | 3 | 1 | exact | 9 |
| incoming | utf-16 | 3 | 1 | exact | 9 |
| outgoing | utf-16 | 3 | 1 | exact | 9 |
| incoming | utf-8 | 3 | 2 | line_end_normalized | 9 |
| incoming | utf-16 | 3 | 2 | line_end_normalized | 9 |
| incoming | utf-8 | 4 | 0 | invalid_line | — |
| incoming | utf-16 | 4 | 0 | invalid_line | — |

Parser point controls (showing first 7 of 14; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `old_row_noncanonical`, `1:2` `old_row_noncanonical`, `2:3` `old_row_noncanonical`, `4:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `separator_interior` → `invalid_newline_boundary`.

## `multiple_terminal_lf`

Tags: newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `a7da489976d0047490617adb4f7a1f27f7af8b52a5176fd002ffe471863520ab`.

Hex: `610a0a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 2 | 3 | lf |
| 2 | 3 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-16 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-8 | 2 | 0 | exact | 3 |
| outgoing | utf-8 | 2 | 0 | exact | 3 |
| incoming | utf-16 | 2 | 0 | exact | 3 |
| outgoing | utf-16 | 2 | 0 | exact | 3 |
| incoming | utf-8 | 2 | 1 | line_end_normalized | 3 |
| incoming | utf-16 | 2 | 1 | line_end_normalized | 3 |
| incoming | utf-8 | 3 | 0 | invalid_line | — |
| incoming | utf-16 | 3 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `1:1` `old_row_noncanonical`, `3:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `nel`

Tags: ropey_control, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `75e7cf7e9bf896d878d3b49a7225fc0ef80d9b03356eb6d783bb276e67afe8fa`.

Hex: `61c28562`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 2 | exact | 3 |
| outgoing | utf-16 | 0 | 2 | exact | 3 |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 3 | exact | 4 |
| outgoing | utf-16 | 0 | 3 | exact | 4 |
| incoming | utf-8 | 0 | 5 | line_end_normalized | 4 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 4 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 6 of 6; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:3` `exact`, `0:4` `exact`, `0:2` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `newline_only`

Tags: newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 1. SHA-256: `01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b`.

Hex: `0a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 0 | 1 | lf |
| 1 | 1 | 1 | 1 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-16 | 0 | 1 | line_end_normalized | 0 |
| incoming | utf-8 | 1 | 0 | exact | 1 |
| outgoing | utf-8 | 1 | 0 | exact | 1 |
| incoming | utf-16 | 1 | 0 | exact | 1 |
| outgoing | utf-16 | 1 | 0 | exact | 1 |
| incoming | utf-8 | 1 | 1 | line_end_normalized | 1 |
| incoming | utf-16 | 1 | 1 | line_end_normalized | 1 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 3 of 4; full facts in machine authority): `0:0` `exact`, `0:1` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `nonleading_bom`

Tags: bom, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 5. SHA-256: `47a12dcb64e9ad8dc2c0819464d72388679ea0da7811edea2acedaf2f13deda7`.

Hex: `61efbbbf62`

Relation: `identity` to `nonleading_bom`; boundaries: 0→0, 1→1, 2→2, 3→3, 4→4, 5→5; ranges: 0..5 `exact`.

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 5 | 5 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | line_end_normalized | 5 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 5 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 7 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:4` `exact`, `0:5` `exact`, `0:2` `invalid_byte_boundary`, `0:3` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `nul`

Tags: basic, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `59b271ae1bbcb1d31d41929817f4b16fb439eb4f31520b5ad1d5ce98920a7138`.

Hex: `610062`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `ps`

Tags: ropey_control, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 5. SHA-256: `0a169a1fef421b35075889027e5d9e64af923b488973ec73c383dc175d0ed60c`.

Hex: `61e280a962`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 5 | 5 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | line_end_normalized | 5 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 5 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 7 of 7; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:4` `exact`, `0:5` `exact`, `0:2` `invalid_byte_boundary`, `0:3` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `repeated`

Tags: basic, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 6. SHA-256: `d71f7504ba0dee48d41556a5145780d04e11f120e8571feef49bb7930b3d9a66`.

Hex: `616261616261`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 6 | 6 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 4 | exact | 4 |
| outgoing | utf-16 | 0 | 4 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 5 | exact | 5 |
| outgoing | utf-16 | 0 | 5 | exact | 5 |
| incoming | utf-8 | 0 | 6 | exact | 6 |
| outgoing | utf-8 | 0 | 6 | exact | 6 |
| incoming | utf-16 | 0 | 6 | exact | 6 |
| outgoing | utf-16 | 0 | 6 | exact | 6 |
| incoming | utf-8 | 0 | 7 | line_end_normalized | 6 |
| incoming | utf-16 | 0 | 7 | line_end_normalized | 6 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 8 of 8; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `0:4` `exact`, `0:5` `exact`, `0:6` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `single_bom_for_double`

Tags: bom, parser_subject, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 4. SHA-256: `84144a41283d6dc344addf4e83189d83fdd656439675abf54e6f44ccb73b4eb6`.

Hex: `efbbbf78`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 4 | 4 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 1 | exact | 3 |
| outgoing | utf-16 | 0 | 1 | exact | 3 |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | line_end_normalized | 4 |
| incoming | utf-16 | 0 | 3 | line_end_normalized | 4 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 6 of 6; full facts in machine authority): `0:0` `exact`, `0:3` `exact`, `0:4` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `terminal_cr`

Tags: newline, bare_cr, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 2. SHA-256: `961a57df036f6c4f44ca8a054271c45e823f469bcc439f0255c40974c3e3d131`.

Hex: `610d`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 2 | 2 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | line_end_normalized | 2 |
| incoming | utf-16 | 0 | 3 | line_end_normalized | 2 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 4 of 4; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `terminal_lf`

Tags: newline, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 2. SHA-256: `87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7`.

Hex: `610a`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 1 | 2 | lf |
| 1 | 2 | 2 | 2 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 1 |
| incoming | utf-8 | 1 | 0 | exact | 2 |
| outgoing | utf-8 | 1 | 0 | exact | 2 |
| incoming | utf-16 | 1 | 0 | exact | 2 |
| outgoing | utf-16 | 1 | 0 | exact | 2 |
| incoming | utf-8 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-16 | 1 | 1 | line_end_normalized | 2 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 4 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `old_row_noncanonical`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.

## `unicode_crlf`

Tags: newline, crlf, unicode, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 8. SHA-256: `a0c8bd5ea1b67299e862358d5f32e447d3fdfdfb66bf70162fa2630b359788c1`.

Hex: `c3a90d0af09f9880`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 2 | 4 | crlf |
| 1 | 4 | 8 | 8 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 1 | exact | 2 |
| outgoing | utf-16 | 0 | 1 | exact | 2 |
| incoming | utf-8 | 0 | 3 | line_end_normalized | 2 |
| incoming | utf-16 | 0 | 2 | line_end_normalized | 2 |
| outgoing | utf-8 | 0 | 3 | invalid_separator_interior | — |
| outgoing | utf-16 | 0 | 2 | invalid_separator_interior | — |
| incoming | utf-8 | 1 | 0 | exact | 4 |
| outgoing | utf-8 | 1 | 0 | exact | 4 |
| incoming | utf-16 | 1 | 0 | exact | 4 |
| outgoing | utf-16 | 1 | 0 | exact | 4 |
| incoming | utf-8 | 1 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 1 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 1 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-16 | 1 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 1 | 4 | exact | 8 |
| outgoing | utf-8 | 1 | 4 | exact | 8 |
| incoming | utf-16 | 1 | 2 | exact | 8 |
| outgoing | utf-16 | 1 | 2 | exact | 8 |
| incoming | utf-8 | 1 | 5 | line_end_normalized | 8 |
| incoming | utf-16 | 1 | 3 | line_end_normalized | 8 |
| incoming | utf-8 | 2 | 0 | invalid_line | — |
| incoming | utf-16 | 2 | 0 | invalid_line | — |

Parser point controls (showing first 9 of 11; full facts in machine authority): `0:0` `exact`, `0:2` `exact`, `0:3` `exact`, `0:4` `old_row_noncanonical`, `0:1` `invalid_byte_boundary`, `1:1` `invalid_byte_boundary`, `1:2` `invalid_byte_boundary`, `1:3` `invalid_byte_boundary`, `2:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `separator_interior` → `invalid_newline_boundary`, `scalar_interior` → `invalid_code_unit_boundary`.

## `unicode_for_bom`

Tags: bom, parser_subject, unicode, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 5. SHA-256: `10f5e9cdd01d869815a52f43599f9c372ffa4b41cda11b5180da85dc0928333c`.

Hex: `f09f988078`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 5 | 5 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 2 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 3 | invalid_code_unit_boundary | — |
| incoming | utf-16 | 0 | 1 | invalid_code_unit_boundary | — |
| incoming | utf-8 | 0 | 4 | exact | 4 |
| outgoing | utf-8 | 0 | 4 | exact | 4 |
| incoming | utf-16 | 0 | 2 | exact | 4 |
| outgoing | utf-16 | 0 | 2 | exact | 4 |
| incoming | utf-8 | 0 | 5 | exact | 5 |
| outgoing | utf-8 | 0 | 5 | exact | 5 |
| incoming | utf-16 | 0 | 3 | exact | 5 |
| outgoing | utf-16 | 0 | 3 | exact | 5 |
| incoming | utf-8 | 0 | 6 | line_end_normalized | 5 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 5 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 7 of 7; full facts in machine authority): `0:0` `exact`, `0:4` `exact`, `0:5` `exact`, `0:1` `invalid_byte_boundary`, `0:2` `invalid_byte_boundary`, `0:3` `invalid_byte_boundary`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`, `scalar_interior` → `invalid_code_unit_boundary`.

## `vt`

Tags: ropey_control, small. Coverage: `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: `98992d7f2eec732149414be555d6c3ae77ac49988fa72a122683cd1b77a3e4bf`.

Hex: `610b62`

| Row | Start | Content end | Separator end | Kind |
| ---: | ---: | ---: | ---: | --- |
| 0 | 0 | 3 | 3 | none |

| Direction | Encoding | Line | Column | Disposition | Byte |
| --- | --- | ---: | ---: | --- | ---: |
| incoming | utf-8 | 0 | 0 | exact | 0 |
| outgoing | utf-8 | 0 | 0 | exact | 0 |
| incoming | utf-16 | 0 | 0 | exact | 0 |
| outgoing | utf-16 | 0 | 0 | exact | 0 |
| incoming | utf-8 | 0 | 1 | exact | 1 |
| outgoing | utf-8 | 0 | 1 | exact | 1 |
| incoming | utf-16 | 0 | 1 | exact | 1 |
| outgoing | utf-16 | 0 | 1 | exact | 1 |
| incoming | utf-8 | 0 | 2 | exact | 2 |
| outgoing | utf-8 | 0 | 2 | exact | 2 |
| incoming | utf-16 | 0 | 2 | exact | 2 |
| outgoing | utf-16 | 0 | 2 | exact | 2 |
| incoming | utf-8 | 0 | 3 | exact | 3 |
| outgoing | utf-8 | 0 | 3 | exact | 3 |
| incoming | utf-16 | 0 | 3 | exact | 3 |
| outgoing | utf-16 | 0 | 3 | exact | 3 |
| incoming | utf-8 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-16 | 0 | 4 | line_end_normalized | 3 |
| incoming | utf-8 | 1 | 0 | invalid_line | — |
| incoming | utf-16 | 1 | 0 | invalid_line | — |

Parser point controls (showing first 5 of 5; full facts in machine authority): `0:0` `exact`, `0:1` `exact`, `0:2` `exact`, `0:3` `exact`, `1:0` `invalid_row`.

Refusals: `equal_range` → `exact`, `source_byte_out_of_bounds` → `invalid_source_byte`, `wrong_source` → `source_mismatch`, `wrong_signature` → `signature_mismatch`, `wrong_schema` → `schema_mismatch`, `overflow_resource` → `overflow_or_resource_boundary`, `instrument_failure` → `instrument_failure`, `reversed_range` → `invalid_range_order`.
