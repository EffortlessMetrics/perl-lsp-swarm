use perl_position_fixtures::{
    Case, MANIFEST_PATH, Manifest, PROJECTION_PATH, load, raw_bytes, validate,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn corpus() -> Result<Manifest, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contents =
        fs::read_to_string(root.join(MANIFEST_PATH)).map_err(|error| error.to_string())?;
    let manifest: Manifest = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    validate(&manifest)?;
    Ok(manifest)
}

fn case_mut<'a>(manifest: &'a mut Manifest, id: &str) -> Result<&'a mut Case, String> {
    manifest.cases.iter_mut().find(|case| case.id == id).ok_or_else(|| format!("missing {id}"))
}

fn rejected(
    mut manifest: Manifest,
    id: &str,
    mutation: impl FnOnce(&mut Case),
    reason: &str,
) -> Result<(), String> {
    mutation(case_mut(&mut manifest, id)?);
    let failure = validate(&manifest).err().ok_or_else(|| format!("{id}: mutation passed"))?;
    if !failure.contains(reason) {
        return Err(format!("{id}: expected {reason}, got {failure}"));
    }
    Ok(())
}

fn replace_raw(case: &mut Case, raw: &[u8]) {
    case.raw_hex = raw.iter().map(|byte| format!("{byte:02x}")).collect();
    case.byte_len = raw.len();
    case.sha256 = Sha256::digest(raw).iter().map(|byte| format!("{byte:02x}")).collect();
}

#[test]
fn checked_subjects_are_complete_and_byte_exact() -> Result<(), String> {
    let manifest = corpus()?;
    if manifest.cases.len() < 40 {
        return Err("required population missing".into());
    }
    for case in &manifest.cases {
        let bytes = raw_bytes(case)?;
        if bytes.len() != case.byte_len {
            return Err(format!("{} byte length", case.id));
        }
    }
    let decisive =
        manifest.cases.iter().find(|case| case.id == "astral").ok_or("missing astral")?;
    if decisive.byte_len != 9
        || decisive.lines.len() != 2
        || (decisive.lines[0].start, decisive.lines[0].content_end, decisive.lines[0].separator_end)
            != (0, 6, 8)
        || (decisive.lines[1].start, decisive.lines[1].content_end, decisive.lines[1].separator_end)
            != (8, 9, 9)
    {
        return Err("literal x😀y CRLF worked fact changed".into());
    }
    if !decisive
        .parser_points
        .iter()
        .any(|point| (point.byte, point.row, point.column) == (7, 0, 7))
        || !decisive.wire.iter().any(|fact| {
            fact.direction == "outgoing"
                && fact.encoding == "utf-8"
                && fact.line == 0
                && fact.column == 7
                && fact.disposition == "invalid_separator_interior"
        })
    {
        return Err("parser/LSP CRLF domain contrast changed".into());
    }
    let terminal =
        manifest.cases.iter().find(|case| case.id == "terminal_lf").ok_or("missing terminal LF")?;
    if terminal.lines.len() != 2
        || (terminal.lines[1].start, terminal.lines[1].content_end, terminal.lines[1].separator_end)
            != (2, 2, 2)
    {
        return Err("terminal empty row fact changed".into());
    }
    let bom = manifest.cases.iter().find(|case| case.id == "bom_ascii").ok_or("missing BOM")?;
    let relation = bom.relation.as_ref().ok_or("missing BOM relation")?;
    if relation.target != "ascii_for_bom"
        || relation.boundary_map[1].target.is_some()
        || relation.boundary_map[2].target.is_some()
        || relation.boundary_map[3].target != Some(0)
    {
        return Err("BOM source relation changed".into());
    }
    let ls = manifest.cases.iter().find(|case| case.id == "ls").ok_or("missing LS")?;
    if ls.lines.len() != 1 || ls.lines[0].content_end != ls.byte_len {
        return Err("Ropey-only separator became canonical row".into());
    }
    Ok(())
}

#[test]
fn byte_and_projection_negative_controls_fail_closed() -> Result<(), String> {
    let manifest = corpus()?;
    let mut missing = manifest.clone();
    missing.cases.retain(|case| case.id != "ps");
    if !validate(&missing)
        .err()
        .is_some_and(|error| error.contains("required versioned case inventory"))
    {
        return Err("missing required case passed".into());
    }
    rejected(
        manifest.clone(),
        "ls",
        |case| case.tags.retain(|tag| tag != "ropey_control"),
        "required tag inventory",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| case.tags.retain(|tag| tag != "decisive"),
        "required tag inventory",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| case.tags.push("unreviewed".into()),
        "required tag inventory",
    )?;
    rejected(
        manifest.clone(),
        "invalid_leading_start",
        |case| case.chunks.push("all_byte_cuts".into()),
        "invalid UTF-8 ingress facts differ",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| case.exhaustive = false,
        "exhaustive coverage class",
    )?;
    rejected(
        manifest.clone(),
        "crlf",
        |case| case.raw_hex = "610a62".into(),
        "byte length/digest",
    )?;
    rejected(manifest.clone(), "crlf", |case| replace_raw(case, b"a\nb"), "scalar sequence")?;
    rejected(
        manifest.clone(),
        "mixed",
        |case| case.raw_hex = case.raw_hex.replacen("0d0a", "0a", 1),
        "byte length/digest",
    )?;
    rejected(
        manifest.clone(),
        "mixed",
        |case| replace_raw(case, b"a\nb\nc\r\nd"),
        "scalar sequence",
    )?;
    rejected(
        manifest.clone(),
        "bom_ascii",
        |case| case.raw_hex = case.raw_hex.replacen("efbbbf", "", 1),
        "byte length/digest",
    )?;
    rejected(manifest.clone(), "bom_ascii", |case| replace_raw(case, b"abc"), "scalar sequence")?;
    rejected(
        manifest.clone(),
        "ascii",
        |case| case.raw_hex.replace_range(0..2, "64"),
        "byte length/digest",
    )?;
    rejected(manifest.clone(), "ls", |case| case.lines[0].content_end = 1, "LF line records")?;
    rejected(
        manifest.clone(),
        "terminal_lf",
        |case| {
            case.lines.pop();
        },
        "LF line records",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| {
            case.wire.retain(|fact| {
                !(fact.encoding == "utf-16"
                    && fact.column == 2
                    && fact.disposition == "invalid_code_unit_boundary")
            })
        },
        "wire facts",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| {
            case.wire.retain(|fact| {
                !(fact.encoding == "utf-8"
                    && fact.column == 2
                    && fact.disposition == "invalid_code_unit_boundary")
            })
        },
        "wire facts",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| case.wire.retain(|fact| fact.disposition != "invalid_separator_interior"),
        "wire facts",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| {
            case.parser_queries.retain(|query| {
                !(query.row == 0 && query.column == 7 && query.disposition == "exact")
            })
        },
        "parser point queries",
    )?;
    rejected(
        manifest.clone(),
        "terminal_lf",
        |case| case.parser_queries.retain(|query| query.disposition != "old_row_noncanonical"),
        "parser point queries",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| case.parser_queries.retain(|query| query.disposition != "invalid_byte_boundary"),
        "parser point queries",
    )?;
    rejected(
        manifest.clone(),
        "bare_cr",
        |case| {
            let duplicate = case.lines[0].clone();
            case.lines.push(duplicate);
        },
        "LF line records",
    )?;
    rejected(
        manifest.clone(),
        "bom_ascii",
        |case| {
            if let Some(relation) = case.relation.as_mut() {
                relation.boundary_map.pop();
            }
        },
        "relation boundary map",
    )?;
    rejected(
        manifest.clone(),
        "bom_ascii",
        |case| {
            if let Some(relation) = case.relation.as_mut() {
                relation.ranges.pop();
            }
        },
        "relation range facts",
    )?;
    rejected(
        manifest.clone(),
        "bom_unicode",
        |case| case.relation = None,
        "required source relation",
    )?;
    rejected(
        manifest.clone(),
        "nonleading_bom",
        |case| case.scalars.retain(|value| *value != 0xfeff),
        "scalar sequence",
    )?;
    rejected(
        manifest.clone(),
        "invalid_leading_middle",
        |case| case.subject = Some(case.id.clone()),
        "invalid UTF-8 ingress facts differ",
    )?;
    rejected(
        manifest.clone(),
        "invalid_continuation_middle",
        |case| {
            if let Some(error) = case.decode_error.as_mut() {
                error.kind = "invalid_leading".into();
            }
        },
        "decoder error class/span",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| {
            case.wire.pop();
        },
        "wire facts",
    )?;
    rejected(
        manifest.clone(),
        "astral",
        |case| {
            case.refusals.pop();
        },
        "refusal facts",
    )?;
    rejected(
        manifest,
        "ascii",
        |case| case.subject = Some("same-visible-different-source".into()),
        "decode/source-domain",
    )?;
    Ok(())
}

#[test]
fn generated_projection_is_current_and_cannot_be_authority() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = load(&root)?;
    let actual =
        fs::read_to_string(root.join(PROJECTION_PATH)).map_err(|error| error.to_string())?;
    if actual != manifest.markdown() {
        return Err("generated Markdown changed or stale".into());
    }
    let edited = actual.replacen("`astral`", "`edited-astral`", 1);
    if edited == manifest.markdown() {
        return Err("hand-edited Markdown accepted".into());
    }
    Ok(())
}
