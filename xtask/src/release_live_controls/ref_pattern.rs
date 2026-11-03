//! Bounded character-based matching for GitHub's documented fnmatch subset.
//!
//! GitHub documents Ruby File.fnmatch with FNM_PATHNAME. Recursive matching
//! belongs to a whole `**/` component; terminal or embedded repeated stars
//! remain ordinary segment stars. GitHub explicitly excludes backslash
//! quoting and `[^...]`, even though Ruby itself accepts those forms.
use std::collections::BTreeSet;

const MAX_PATTERN_CHARS: usize = 1024;
const MAX_SUBJECT_CHARS: usize = 1024;

#[derive(Debug)]
enum Token {
    Literal(char),
    Any,
    Star,
    Directories,
    Class { negated: bool, ranges: Vec<(char, char)> },
}

pub(super) fn matches(pattern: &str, subject: &str) -> Result<bool, String> {
    let subject: Vec<char> = subject.chars().collect();
    if subject.len() > MAX_SUBJECT_CHARS {
        return Err("ref exceeds the observer's matching bound".into());
    }
    let tokens = parse(pattern)?;
    // Position sets bound work to tokens * subject length; unlike recursive
    // wildcard backtracking, repeated stars cannot cause exponential work.
    let mut positions = BTreeSet::from([0]);
    for token in tokens {
        let mut next = BTreeSet::new();
        match token {
            Token::Star => {
                // A star's reachable positions form contiguous intervals
                // within each path segment. Scan each position only once.
                let mut reachable = false;
                for position in 0..=subject.len() {
                    reachable |= positions.contains(&position);
                    if reachable {
                        next.insert(position);
                    }
                    if subject.get(position) == Some(&'/') {
                        reachable = false;
                    }
                }
            }
            Token::Directories => {
                let mut reachable = false;
                for position in 0..=subject.len() {
                    let starts_here = positions.contains(&position);
                    reachable |= starts_here;
                    if starts_here
                        || (reachable && position > 0 && subject.get(position - 1) == Some(&'/'))
                    {
                        next.insert(position);
                    }
                }
            }
            token => {
                for position in positions {
                    let Some(character) = subject.get(position).copied() else {
                        continue;
                    };
                    let matched = match &token {
                        Token::Literal(expected) => character == *expected,
                        Token::Any => character != '/',
                        Token::Class { negated, ranges } => {
                            character != '/'
                                && (ranges
                                    .iter()
                                    .any(|(start, end)| *start <= character && character <= *end)
                                    != *negated)
                        }
                        Token::Star | Token::Directories => false,
                    };
                    if matched {
                        next.insert(position + 1);
                    }
                }
            }
        }
        positions = next;
    }
    Ok(positions.contains(&subject.len()))
}

fn parse(pattern: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = pattern.chars().collect();
    if chars.len() > MAX_PATTERN_CHARS {
        return Err("ref pattern exceeds the observer's matching bound".into());
    }
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while let Some(character) = chars.get(cursor).copied() {
        match character {
            '\\' | '{' | '}' => return Err("unsupported GitHub ref-pattern syntax".into()),
            '?' => {
                tokens.push(Token::Any);
                cursor += 1;
            }
            '*' => {
                let start = cursor;
                while chars.get(cursor) == Some(&'*') {
                    cursor += 1;
                }
                let component_start = start == 0 || chars.get(start - 1) == Some(&'/');
                if cursor - start == 2 && component_start && chars.get(cursor) == Some(&'/') {
                    tokens.push(Token::Directories);
                    cursor += 1;
                } else {
                    tokens.push(Token::Star);
                }
            }
            '[' => {
                cursor += 1;
                if chars.get(cursor) == Some(&']') {
                    return Err("unsupported leading-closing-bracket ref class".into());
                }
                if chars.get(cursor) == Some(&'^') {
                    return Err("GitHub does not support caret-negated ref classes".into());
                }
                let negated = chars.get(cursor) == Some(&'!');
                if negated {
                    cursor += 1;
                }
                if chars.get(cursor) == Some(&']') {
                    return Err("unsupported leading-closing-bracket ref class".into());
                }
                let mut members = Vec::new();
                loop {
                    match chars.get(cursor).copied() {
                        Some(']') if !members.is_empty() => {
                            cursor += 1;
                            break;
                        }
                        Some('[' | '\\') => return Err("unsupported ref character class".into()),
                        Some(character) => {
                            members.push(character);
                            cursor += 1;
                        }
                        None => return Err("unterminated ref character class".into()),
                    }
                }
                let mut ranges = Vec::new();
                let mut member = 0;
                while let Some(start) = members.get(member).copied() {
                    if members.get(member + 1) == Some(&'-')
                        && let Some(end) = members.get(member + 2).copied()
                    {
                        if start > end {
                            return Err("reversed ref character range".into());
                        }
                        ranges.push((start, end));
                        member += 3;
                    } else {
                        ranges.push((start, start));
                        member += 1;
                    }
                }
                tokens.push(Token::Class { negated, ranges });
            }
            character => {
                tokens.push(Token::Literal(character));
                cursor += 1;
            }
        }
    }
    Ok(tokens)
}
