pub(super) fn valid_repository_slug(raw: &str) -> bool {
    let Some((owner, repository)) = raw.split_once('/') else {
        return false;
    };
    !repository.contains('/')
        && valid_repository_segment(owner)
        && valid_repository_segment(repository)
}

fn valid_repository_segment(raw: &str) -> bool {
    !raw.is_empty()
        && raw != "."
        && raw != ".."
        && raw
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
}

pub(super) fn valid_repository_path(raw: &str) -> bool {
    !raw.is_empty()
        && !raw.starts_with('/')
        && !raw.starts_with('\\')
        && !raw.contains('\\')
        && !raw.contains(':')
        && !raw.contains('\0')
        && raw.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

pub(super) fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn normalize_release_version(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.trim() != raw {
        return None;
    }

    let raw = raw.strip_prefix('v').unwrap_or(raw);
    if raw.is_empty() || raw.starts_with('v') {
        return None;
    }

    let (without_build, build) = split_once_unique(raw, '+')?;
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) if !core.is_empty() && !prerelease.is_empty() => {
            (core, Some(prerelease))
        }
        Some(_) => return None,
        None => (without_build, None),
    };

    let mut core_parts = core.split('.');
    let major = core_parts.next()?;
    let minor = core_parts.next()?;
    let patch = core_parts.next()?;
    if core_parts.next().is_some()
        || !valid_numeric_core_identifier(major)
        || !valid_numeric_core_identifier(minor)
        || !valid_numeric_core_identifier(patch)
    {
        return None;
    }

    if let Some(prerelease) = prerelease {
        if !valid_identifier_list(prerelease, true) {
            return None;
        }
    }
    if let Some(build) = build {
        if !valid_identifier_list(build, false) {
            return None;
        }
    }

    let mut normalized = format!("{major}.{minor}.{patch}");
    if let Some(prerelease) = prerelease {
        normalized.push('-');
        normalized.push_str(prerelease);
    }
    if let Some(build) = build {
        normalized.push('+');
        normalized.push_str(build);
    }
    Some(normalized)
}

fn split_once_unique(raw: &str, delimiter: char) -> Option<(&str, Option<&str>)> {
    let Some((left, right)) = raw.split_once(delimiter) else {
        return Some((raw, None));
    };
    if left.is_empty() || right.is_empty() || right.contains(delimiter) {
        return None;
    }
    Some((left, Some(right)))
}

fn valid_numeric_core_identifier(raw: &str) -> bool {
    !raw.is_empty()
        && raw.bytes().all(|byte| byte.is_ascii_digit())
        && (raw == "0" || !raw.starts_with('0'))
}

fn valid_identifier_list(raw: &str, reject_numeric_leading_zero: bool) -> bool {
    raw.split('.').all(|identifier| {
        !identifier.is_empty()
            && identifier
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
            && (!reject_numeric_leading_zero
                || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                || identifier == "0"
                || !identifier.starts_with('0'))
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_release_version;

    #[test]
    fn normalizes_optional_tag_prefix() {
        assert_eq!(normalize_release_version("v0.17.0").as_deref(), Some("0.17.0"));
        assert_eq!(
            normalize_release_version("0.17.0-beta.1+build.7").as_deref(),
            Some("0.17.0-beta.1+build.7")
        );
    }

    #[test]
    fn rejects_ambiguous_release_versions() {
        for invalid in [
            "",
            " v0.17.0",
            "vv0.17.0",
            "0.17",
            "0.17.0.1",
            "00.17.0",
            "0.017.0",
            "0.17.00",
            "0.17.0-",
            "0.17.0-alpha..1",
            "0.17.0-01",
            "0.17.0+",
            "0.17.0+build..1",
            "0.17.0+build+2",
        ] {
            assert_eq!(normalize_release_version(invalid), None, "{invalid}");
        }
    }
}
