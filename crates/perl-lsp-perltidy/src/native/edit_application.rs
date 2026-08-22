//! Independent fallible edit-application oracle (#8048 shift-left seam).
//!
//! [`apply_edits_exact`] applies text edits against exact source bytes for
//! formatter tests and receipts. It is an **oracle**: it deliberately shares
//! no geometry code with any production range constructor or mapper
//! (`TextRange::whole_document`, `FormatRange::whole_document`,
//! `LspServer::get_document_end_position`, `TextRange::at_byte_offset`), so a
//! proof that passes through it cannot inherit a defect from the code under
//! test.
//!
//! Rejection is the contract: reversed, out-of-bounds/unreachable,
//! overlapping, duplicate, and mid-code-point edits are typed errors — never clamped.
//! Distinct zero-width edits at one position retain their input order. This
//! is the explicit contract of this independent oracle; it does not claim to
//! model the order used by any current LSP or production edit applicator.
//!
//! Authority boundary: no production caller. #10239/#10242 own wiring this
//! into native/wire plan application; until then it is proof-only surface.

/// Position encoding declared by the caller for `(line, character)` pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    /// LSP wire positions: characters count UTF-16 code units.
    Utf16CodeUnits,
    /// Native byte plans: characters count UTF-8 bytes since line start.
    Utf8Bytes,
}

/// One replacement over a `(line, character)` range in a declared encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSpec {
    /// Zero-based start line.
    pub start_line: u32,
    /// Start character in the declared encoding.
    pub start_character: u32,
    /// Zero-based end line (inclusive-exclusive pair with the character).
    pub end_line: u32,
    /// End character in the declared encoding.
    pub end_character: u32,
    /// Replacement bytes inserted in place of the range.
    pub new_text: String,
}

impl EditSpec {
    /// Build a spec from explicit fields.
    #[must_use]
    pub fn new(
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
        new_text: impl Into<String>,
    ) -> Self {
        Self { start_line, start_character, end_line, end_character, new_text: new_text.into() }
    }
}

/// Why an edit set cannot be applied exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditApplicationError {
    /// An edit's start is after its end.
    ReversedRange {
        /// Offending edit index.
        edit_index: usize,
    },
    /// A position no scan of the exact source can reach (out of bounds,
    /// past line content, or inside a CRLF pair).
    UnreachablePosition {
        /// Offending edit index.
        edit_index: usize,
        /// Declared line of the unreachable position.
        line: u32,
        /// Declared character of the unreachable position.
        character: u32,
    },
    /// A position pointing into one half of a multi-unit code point (a UTF-16
    /// surrogate half, or mid-byte within a multi-byte UTF-8 char).
    MidCodePoint {
        /// Offending edit index.
        edit_index: usize,
        /// Declared line of the mid-code-point position.
        line: u32,
        /// Declared character of the mid-code-point position.
        character: u32,
    },
    /// Two edits replace overlapping source spans.
    OverlappingEdits {
        /// Index of the earlier edit (by resolved span).
        first_edit_index: usize,
        /// Index of the later edit overlapping it.
        second_edit_index: usize,
    },
    /// Two identical zero-width edits would make insertion order ambiguous.
    DuplicateEdits {
        /// Index of the first identical edit (by resolved span).
        first_edit_index: usize,
        /// Index of the second identical edit.
        second_edit_index: usize,
    },
}

impl std::fmt::Display for EditApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::ReversedRange { edit_index } => {
                write!(formatter, "edit {edit_index}: start is after end")
            }
            Self::UnreachablePosition { edit_index, line, character } => {
                write!(
                    formatter,
                    "edit {edit_index}: position ({line}, {character}) is unreachable in the exact source"
                )
            }
            Self::MidCodePoint { edit_index, line, character } => write!(
                formatter,
                "edit {edit_index}: position ({line}, {character}) falls inside a code point"
            ),
            Self::OverlappingEdits { first_edit_index, second_edit_index } => {
                write!(formatter, "edits {first_edit_index} and {second_edit_index} overlap")
            }
            Self::DuplicateEdits { first_edit_index, second_edit_index } => write!(
                formatter,
                "edits {first_edit_index} and {second_edit_index} are identical zero-width insertions"
            ),
        }
    }
}

impl std::error::Error for EditApplicationError {}

struct ResolvedEdit {
    original_index: usize,
    start_byte: usize,
    end_byte: usize,
}

/// Apply `edits` to `source` exactly, or reject the complete set.
///
/// The function never clamps and never partially applies: if any edit is
/// invalid, the whole call fails with a typed error describing the first
/// validation failure in resolution order. Valid edits may be adjacent but
/// must not overlap; application is equivalent to applying them back to front
/// over the exact predecessor bytes.
pub fn apply_edits_exact(
    source: &str,
    edits: &[EditSpec],
    encoding: PositionEncoding,
) -> Result<String, EditApplicationError> {
    let mut resolved = Vec::with_capacity(edits.len());

    for (original_index, edit) in edits.iter().enumerate() {
        let start_byte = resolve_position(
            source,
            edit.start_line,
            edit.start_character,
            encoding,
            original_index,
        )?;
        let end_byte =
            resolve_position(source, edit.end_line, edit.end_character, encoding, original_index)?;

        if start_byte > end_byte {
            return Err(EditApplicationError::ReversedRange { edit_index: original_index });
        }

        resolved.push(ResolvedEdit { original_index, start_byte, end_byte });
    }

    let mut order: Vec<&ResolvedEdit> = resolved.iter().collect();
    order.sort_by_key(|edit| (edit.start_byte, edit.end_byte));

    for window in order.windows(2) {
        let (first, second) = (window[0], window[1]);
        if second.start_byte < first.end_byte {
            return Err(EditApplicationError::OverlappingEdits {
                first_edit_index: first.original_index,
                second_edit_index: second.original_index,
            });
        }
    }

    let mut group_start = 0;
    while group_start < order.len() {
        let first = order[group_start];
        if first.start_byte == first.end_byte {
            let mut group_end = group_start + 1;
            while group_end < order.len()
                && order[group_end].start_byte == first.start_byte
                && order[group_end].end_byte == first.end_byte
            {
                group_end += 1;
            }

            for (offset, current) in order[group_start + 1..group_end].iter().enumerate() {
                for previous in &order[group_start..group_start + 1 + offset] {
                    if edits[previous.original_index].new_text
                        == edits[current.original_index].new_text
                    {
                        return Err(EditApplicationError::DuplicateEdits {
                            first_edit_index: previous.original_index,
                            second_edit_index: current.original_index,
                        });
                    }
                }
            }
            group_start = group_end;
        } else {
            group_start += 1;
        }
    }

    let mut output = String::with_capacity(source.len());
    let mut cursor = 0_usize;

    for edit in &order {
        output.push_str(&source[cursor..edit.start_byte]);
        output.push_str(&edits[edit.original_index].new_text);
        cursor = edit.end_byte;
    }
    output.push_str(&source[cursor..]);

    Ok(output)
}

/// Resolve one declared `(line, character)` position by scanning the exact
/// source with no production geometry code.
fn resolve_position(
    source: &str,
    target_line: u32,
    target_character: u32,
    encoding: PositionEncoding,
    edit_index: usize,
) -> Result<usize, EditApplicationError> {
    let unit = |ch: char| match encoding {
        PositionEncoding::Utf16CodeUnits => ch.len_utf16() as u32,
        PositionEncoding::Utf8Bytes => ch.len_utf8() as u32,
    };

    let mut line = 0_u32;
    let mut character = 0_u32;
    let mut offset = 0_usize;
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        if (line, character) == (target_line, target_character) {
            return Ok(offset);
        }

        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                    offset += 2;
                } else {
                    offset += 1;
                }
                line = line.saturating_add(1);
                character = 0;
            }
            '\n' => {
                offset += 1;
                line = line.saturating_add(1);
                character = 0;
            }
            other => {
                let width = unit(other);
                if target_line == line
                    && width > 1
                    && character < target_character
                    && target_character < character + width
                {
                    return Err(EditApplicationError::MidCodePoint {
                        edit_index,
                        line: target_line,
                        character: target_character,
                    });
                }
                offset += other.len_utf8();
                character = character.saturating_add(width);
            }
        }
    }

    if (line, character) == (target_line, target_character) {
        Ok(offset)
    } else {
        Err(EditApplicationError::UnreachablePosition {
            edit_index,
            line: target_line,
            character: target_character,
        })
    }
}
