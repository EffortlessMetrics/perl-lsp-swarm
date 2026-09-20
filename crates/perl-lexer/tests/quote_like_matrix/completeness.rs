//! Mechanical completeness, uniqueness, and negative-control predicates.

use super::schema::{
    Axis, Disposition, ExpectedKind, MatrixRow, NextOrdinary, OperatorFamily, PERL_PROFILE,
    SCHEMA_VERSION, SourceContext,
};

pub fn validate(rows: &[MatrixRow]) -> Result<(), String> {
    if rows.is_empty() {
        return Err("matrix contains no rows".to_string());
    }
    check_identity(rows)?;
    check_unique_ids(rows)?;
    check_row_shape(rows)?;
    check_operator_coverage(rows)?;
    check_two_body_coverage(rows)?;
    check_shared_axes(rows)?;
    check_suppression_independence(rows)?;
    Ok(())
}

fn check_identity(rows: &[MatrixRow]) -> Result<(), String> {
    for row in rows {
        if row.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "{} uses schema {} instead of {SCHEMA_VERSION}",
                row.id, row.schema_version
            ));
        }
        if row.perl_profile != PERL_PROFILE {
            return Err(format!(
                "{} uses profile {} instead of {PERL_PROFILE} without a schema transition",
                row.id, row.perl_profile
            ));
        }
    }
    Ok(())
}

fn check_unique_ids(rows: &[MatrixRow]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        if !seen.insert(row.id) {
            return Err(format!("duplicate case ID {}", row.id));
        }
    }
    Ok(())
}

fn check_row_shape(rows: &[MatrixRow]) -> Result<(), String> {
    for row in rows {
        if row.expected.len() < 2 {
            return Err(format!(
                "{} is first-token-only; expected ordered tokens including EOF",
                row.id
            ));
        }
        if !matches!(row.expected.last().map(|token| token.kind), Some(ExpectedKind::Eof)) {
            return Err(format!("{} does not end with EOF", row.id));
        }
        if row.expected.iter().filter(|token| matches!(token.kind, ExpectedKind::Eof)).count() != 1
        {
            return Err(format!("{} must contain exactly one EOF token", row.id));
        }
        match row.next_ordinary {
            NextOrdinary::Present { text, .. } => {
                if !row.source.contains(text) {
                    return Err(format!("{} next ordinary {text:?} is not in source", row.id));
                }
            }
            NextOrdinary::EatenByError | NextOrdinary::EatenByComment => {
                if !row.source.contains("after") {
                    return Err(format!(
                        "{} claims following code was eaten but source has no named `after` follower",
                        row.id
                    ));
                }
            }
            NextOrdinary::NoneAtEof => {}
        }
        if row.axes.is_empty() {
            return Err(format!("{} has no axis tags", row.id));
        }
    }
    Ok(())
}

fn check_operator_coverage(rows: &[MatrixRow]) -> Result<(), String> {
    for operator in OperatorFamily::ALL {
        let owned = rows.iter().filter(|row| row.operator == operator).collect::<Vec<_>>();
        if owned.is_empty() {
            return Err(format!("operator {} has no rows", operator.as_str()));
        }
        for axis in [
            Axis::AttachedPaired,
            Axis::AttachedUnpaired,
            Axis::ImmediateHash,
            Axis::WhitespaceSeparated,
            Axis::CommentGapBeforePaired,
            Axis::MalformedFollower,
            Axis::HashKey,
            Axis::Method,
            Axis::FatArrow,
        ] {
            require_axis(&owned, operator, axis)?;
        }
        if !owned.iter().any(|row| {
            row.disposition == Disposition::Clean
                && row.expected.iter().any(|token| token.kind.is_quote_like())
        }) {
            return Err(format!("operator {} has no clean quote-like row", operator.as_str()));
        }
    }
    Ok(())
}

fn check_two_body_coverage(rows: &[MatrixRow]) -> Result<(), String> {
    for operator in OperatorFamily::ALL.into_iter().filter(|operator| operator.is_two_body()) {
        let owned = rows.iter().filter(|row| row.operator == operator).collect::<Vec<_>>();
        require_axis(&owned, operator, Axis::MixedSecondDelimiter)?;
        require_axis(&owned, operator, Axis::CommentBetweenBodies)?;
    }
    Ok(())
}

fn check_shared_axes(rows: &[MatrixRow]) -> Result<(), String> {
    for axis in [
        Axis::NestedPaired,
        Axis::EscapedDelimiter,
        Axis::EmptyBody,
        Axis::MultilineLf,
        Axis::MultilineCrlf,
        Axis::MultilineCr,
        Axis::Unicode,
        Axis::Modifier,
        Axis::HashSlice,
        Axis::Division,
        Axis::DefinedOr,
        Axis::FileTest,
        Axis::SubroutineName,
        Axis::PackageName,
        Axis::Label,
        Axis::ConsecutiveCommentGap,
    ] {
        if !rows.iter().any(|row| row.axes.contains(&axis)) {
            return Err(format!("matrix is missing shared axis {axis:?}"));
        }
    }
    Ok(())
}

fn check_suppression_independence(rows: &[MatrixRow]) -> Result<(), String> {
    for operator in OperatorFamily::ALL {
        let hash = rows.iter().any(|row| {
            row.operator == operator
                && row.source_context == SourceContext::HashKey
                && !row.expected.iter().any(|token| token.kind.is_quote_like())
        });
        if !hash {
            return Err(format!(
                "operator {} lacks an independent hash-key suppression row",
                operator.as_str()
            ));
        }
    }
    Ok(())
}

fn require_axis(rows: &[&MatrixRow], operator: OperatorFamily, axis: Axis) -> Result<(), String> {
    if rows.iter().any(|row| row.axes.contains(&axis)) {
        Ok(())
    } else {
        Err(format!("operator {} is missing axis {axis:?}", operator.as_str()))
    }
}

pub fn without_operator(rows: &[MatrixRow], operator: OperatorFamily) -> Vec<MatrixRow> {
    rows.iter().copied().filter(|row| row.operator != operator).collect()
}
