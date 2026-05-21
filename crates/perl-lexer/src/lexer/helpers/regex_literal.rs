use crate::PerlLexer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegexScanAction {
    CloseLiteral,
    StartCharacterClass,
    EndCharacterClass,
    Escape,
    Advance,
}

pub(crate) fn regex_scan_action(ch: char, in_character_class: bool) -> RegexScanAction {
    match ch {
        '/' if !in_character_class => RegexScanAction::CloseLiteral,
        '\\' => RegexScanAction::Escape,
        '[' => RegexScanAction::StartCharacterClass,
        ']' if in_character_class => RegexScanAction::EndCharacterClass,
        _ => RegexScanAction::Advance,
    }
}

pub(crate) fn consume_ascii_alnum_run(lexer: &mut PerlLexer<'_>) {
    while let Some(ch) = lexer.current_char() {
        if ch.is_ascii_alphanumeric() {
            lexer.advance();
            continue;
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::{RegexScanAction, regex_scan_action};

    #[test]
    fn regex_scan_action_respects_character_class_state() {
        assert_eq!(regex_scan_action('/', false), RegexScanAction::CloseLiteral);
        assert_eq!(regex_scan_action('/', true), RegexScanAction::Advance);
        assert_eq!(regex_scan_action('[', false), RegexScanAction::StartCharacterClass);
        assert_eq!(regex_scan_action(']', true), RegexScanAction::EndCharacterClass);
        assert_eq!(regex_scan_action('\\', false), RegexScanAction::Escape);
    }
}
