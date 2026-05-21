use super::{ModSpec, canon_run};

pub(super) fn split_tail_for_spec(tail: &str, spec: &ModSpec) -> Option<(String, Option<&'static str>)> {
    if !is_ascii_alpha(tail) {
        return None;
    }

    if !spec.allow_charset {
        return split_without_charset(tail, spec);
    }

    let (run_part, charset) = split_charset_suffix(tail);
    if !run_part.chars().all(|c| spec.run.contains(&c)) {
        return None;
    }

    Some((canon_run(run_part, spec), charset))
}

fn is_ascii_alpha(tail: &str) -> bool {
    tail.chars().all(|c| c.is_ascii_alphabetic())
}

fn split_without_charset(tail: &str, spec: &ModSpec) -> Option<(String, Option<&'static str>)> {
    if tail.chars().all(|c| spec.run.contains(&c)) {
        Some((canon_run(tail, spec), None))
    } else {
        None
    }
}

fn split_charset_suffix(tail: &str) -> (&str, Option<&'static str>) {
    if let Some(stripped) = tail.strip_suffix("aa") {
        return (stripped, Some("aa"));
    }

    for (suffix, value) in [('a', "a"), ('d', "d"), ('l', "l"), ('u', "u")] {
        if let Some(stripped) = tail.strip_suffix(suffix) {
            return (stripped, Some(value));
        }
    }

    (tail, None)
}
