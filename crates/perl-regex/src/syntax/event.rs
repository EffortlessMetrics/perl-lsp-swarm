use crate::validator::RegexRange;

/// Extended-whitespace mode used while scanning one regex body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexExtendedMode {
    Off,
    Extended,
    ExtraExtended,
}

impl RegexExtendedMode {
    pub(crate) const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Effective scan state at one source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RegexModeState {
    pub(crate) extended: RegexExtendedMode,
    pub(crate) captures_by_default: bool,
}

impl Default for RegexModeState {
    fn default() -> Self {
        Self { extended: RegexExtendedMode::Off, captures_by_default: true }
    }
}

/// Regex group form recognized by the bounded structural scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexGroupKind {
    Capturing,
    NonCapturing,
    NamedCapture { name_range: RegexRange },
    Lookahead,
    NegativeLookahead,
    Lookbehind,
    NegativeLookbehind,
    Atomic,
    BranchReset,
    ModifierScope,
    Special,
}

/// Comment form excluded from executable regex structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexCommentKind {
    Group,
    ExtendedLine,
    ExtendedWhitespace,
}

/// Embedded executable or runtime-pattern form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexEmbeddedCodeKind {
    Immediate,
    Deferred,
}

/// Quantifier behavior after structural normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexQuantifierMode {
    Greedy,
    Lazy,
    Possessive,
}

/// Normalized quantifier bounds and mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RegexQuantifier {
    pub(crate) lower: usize,
    pub(crate) upper: Option<usize>,
    pub(crate) mode: RegexQuantifierMode,
}

impl RegexQuantifier {
    /// Whether the repetition count itself may vary at runtime.
    pub(crate) const fn is_variable(self) -> bool {
        match self.upper {
            Some(upper) => upper != self.lower,
            None => true,
        }
    }

    /// Whether the quantified atom can be entered more than once.
    pub(crate) const fn repeats_atom(self) -> bool {
        match self.upper {
            Some(upper) => upper > 1,
            None => true,
        }
    }

    /// Whether this quantifier can backtrack its repetition count.
    pub(crate) const fn is_backtracking(self) -> bool {
        !matches!(self.mode, RegexQuantifierMode::Possessive) && self.is_variable()
    }
}

/// Malformed/truncated construct observed without guessing a repaired structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexMalformedKind {
    UnterminatedQuotedLiteral,
    UnterminatedCharacterClass,
    UnterminatedComment,
    UnterminatedEmbeddedCode,
    UnterminatedNamedCapture,
    UnmatchedGroupClose,
    UnclosedGroup,
}

/// One source-backed structural event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexEventKind {
    Atom,
    Escape,
    QuotedLiteral { closed: bool },
    CharacterClass { closed: bool },
    Comment(RegexCommentKind),
    GroupOpen(RegexGroupKind),
    GroupClose(RegexGroupKind),
    ModeChange,
    Alternation,
    Quantifier(RegexQuantifier),
    UnicodeProperty { negated: bool, closed: bool },
    EmbeddedCode {
        kind: RegexEmbeddedCodeKind,
        opener_range: RegexRange,
        closed: bool,
    },
    Interpolation,
    Malformed(RegexMalformedKind),
}

/// One event plus the effective mode and group depth at that event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RegexEvent {
    pub(crate) kind: RegexEventKind,
    pub(crate) range: RegexRange,
    pub(crate) mode: RegexModeState,
    pub(crate) depth: usize,
}

/// Deterministic work budget that stopped event production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegexEventBudget {
    Events,
    Nesting,
    Steps,
}

/// Explicit event-stream limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RegexEventLimits {
    pub(crate) max_events: usize,
    pub(crate) max_depth: usize,
    pub(crate) max_steps: usize,
}

impl RegexEventLimits {
    pub(crate) fn for_input(input_len: usize) -> Self {
        Self {
            max_events: input_len.saturating_mul(2).saturating_add(8),
            max_depth: input_len.saturating_add(1).clamp(1, 4096),
            max_steps: input_len.saturating_mul(4).saturating_add(64),
        }
    }
}

/// Complete bounded structural scan result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegexEventStream {
    pub(crate) events: Vec<RegexEvent>,
    pub(crate) exhausted: Option<RegexEventBudget>,
    pub(crate) malformed: bool,
}

#[derive(Debug, Clone, Copy)]
struct GroupFrame {
    kind: RegexGroupKind,
    restore_mode: RegexModeState,
}

struct EventParser<'a> {
    pattern: &'a str,
    bytes: &'a [u8],
    pos: usize,
    mode: RegexModeState,
    stack: Vec<GroupFrame>,
    events: Vec<RegexEvent>,
    steps: usize,
    limits: RegexEventLimits,
    exhausted: Option<RegexEventBudget>,
    malformed: bool,
}

pub(crate) fn parse_regex_events(
    pattern: &str,
    initial_mode: RegexModeState,
) -> RegexEventStream {
    parse_regex_events_with_limits(
        pattern,
        initial_mode,
        RegexEventLimits::for_input(pattern.len()),
    )
}

fn parse_regex_events_with_limits(
    pattern: &str,
    initial_mode: RegexModeState,
    limits: RegexEventLimits,
) -> RegexEventStream {
    EventParser {
        pattern,
        bytes: pattern.as_bytes(),
        pos: 0,
        mode: initial_mode,
        stack: Vec::new(),
        events: Vec::new(),
        steps: 0,
        limits,
        exhausted: None,
        malformed: false,
    }
    .parse()
}

impl<'a> EventParser<'a> {
    fn parse(mut self) -> RegexEventStream {
        while self.pos < self.bytes.len() && self.exhausted.is_none() {
            if self.parse_quoted_literal()
                || self.parse_escape_or_unicode_property()
                || self.parse_character_class()
                || self.parse_extended_trivia()
                || self.parse_group()
                || self.parse_group_close()
                || self.parse_alternation()
                || self.parse_quantifier()
                || self.parse_interpolation()
            {
                continue;
            }

            let start = self.pos;
            let end = self.next_char_end(start);
            if !self.advance(end) {
                break;
            }
            if !self.emit(start, end, RegexEventKind::Atom, self.mode, self.stack.len()) {
                break;
            }
        }

        if self.exhausted.is_none() && !self.stack.is_empty() {
            self.malformed = true;
            let end = self.bytes.len();
            let _ = self.emit(
                end,
                end,
                RegexEventKind::Malformed(RegexMalformedKind::UnclosedGroup),
                self.mode,
                self.stack.len(),
            );
        }

        RegexEventStream {
            events: self.events,
            exhausted: self.exhausted,
            malformed: self.malformed,
        }
    }

    fn parse_quoted_literal(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b'\\')
            || self.bytes.get(self.pos + 1) != Some(&b'Q')
        {
            return false;
        }

        let start = self.pos;
        let mut cursor = start + 2;
        let mut closed = false;
        while cursor + 1 < self.bytes.len() {
            if self.bytes[cursor] == b'\\' && self.bytes[cursor + 1] == b'E' {
                cursor += 2;
                closed = true;
                break;
            }
            cursor += 1;
        }
        if !closed {
            cursor = self.bytes.len();
            self.malformed = true;
        }
        if !self.advance(cursor) {
            return true;
        }
        let _ = self.emit(
            start,
            cursor,
            RegexEventKind::QuotedLiteral { closed },
            self.mode,
            self.stack.len(),
        );
        if !closed {
            let _ = self.emit(
                cursor,
                cursor,
                RegexEventKind::Malformed(RegexMalformedKind::UnterminatedQuotedLiteral),
                self.mode,
                self.stack.len(),
            );
        }
        true
    }

    fn parse_escape_or_unicode_property(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b'\\') {
            return false;
        }

        let start = self.pos;
        if matches!(self.bytes.get(start + 1), Some(b'p' | b'P'))
            && self.bytes.get(start + 2) == Some(&b'{')
        {
            let negated = self.bytes[start + 1] == b'P';
            let mut cursor = start + 3;
            while cursor < self.bytes.len() && self.bytes[cursor] != b'}' {
                cursor += 1;
            }
            let closed = cursor < self.bytes.len();
            if closed {
                cursor += 1;
            } else {
                self.malformed = true;
            }
            if !self.advance(cursor) {
                return true;
            }
            let _ = self.emit(
                start,
                cursor,
                RegexEventKind::UnicodeProperty { negated, closed },
                self.mode,
                self.stack.len(),
            );
            return true;
        }

        let end = if start + 1 < self.bytes.len() {
            self.next_char_end(start + 1)
        } else {
            self.bytes.len()
        };
        if !self.advance(end) {
            return true;
        }
        let _ = self.emit(start, end, RegexEventKind::Escape, self.mode, self.stack.len());
        true
    }

    fn parse_character_class(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b'[') {
            return false;
        }

        let start = self.pos;
        let mut cursor = start + 1;
        if self.bytes.get(cursor) == Some(&b'^') {
            cursor += 1;
        }
        if self.bytes.get(cursor) == Some(&b']') {
            cursor += 1;
        }

        let mut closed = false;
        while cursor < self.bytes.len() {
            match self.bytes[cursor] {
                b'\\' => {
                    cursor = if cursor + 1 < self.bytes.len() {
                        self.next_char_end(cursor + 1)
                    } else {
                        self.bytes.len()
                    };
                }
                b']' => {
                    cursor += 1;
                    closed = true;
                    break;
                }
                _ => cursor = self.next_char_end(cursor),
            }
        }
        if !closed {
            self.malformed = true;
        }
        if !self.advance(cursor) {
            return true;
        }
        let _ = self.emit(
            start,
            cursor,
            RegexEventKind::CharacterClass { closed },
            self.mode,
            self.stack.len(),
        );
        if !closed {
            let _ = self.emit(
                cursor,
                cursor,
                RegexEventKind::Malformed(RegexMalformedKind::UnterminatedCharacterClass),
                self.mode,
                self.stack.len(),
            );
        }
        true
    }

    fn parse_extended_trivia(&mut self) -> bool {
        if !self.mode.extended.enabled() {
            return false;
        }

        if self.bytes.get(self.pos) == Some(&b'#') {
            let start = self.pos;
            let mut cursor = start;
            while cursor < self.bytes.len()
                && !matches!(self.bytes[cursor], b'\n' | b'\r')
            {
                cursor = self.next_char_end(cursor);
            }
            if !self.advance(cursor) {
                return true;
            }
            let _ = self.emit(
                start,
                cursor,
                RegexEventKind::Comment(RegexCommentKind::ExtendedLine),
                self.mode,
                self.stack.len(),
            );
            return true;
        }

        let Some(ch) = self.pattern.get(self.pos..).and_then(|rest| rest.chars().next()) else {
            return false;
        };
        if !ch.is_whitespace() {
            return false;
        }
        let start = self.pos;
        let mut cursor = start;
        while let Some(candidate) = self.pattern.get(cursor..).and_then(|rest| rest.chars().next()) {
            if !candidate.is_whitespace() {
                break;
            }
            cursor = cursor.saturating_add(candidate.len_utf8());
        }
        if !self.advance(cursor) {
            return true;
        }
        let _ = self.emit(
            start,
            cursor,
            RegexEventKind::Comment(RegexCommentKind::ExtendedWhitespace),
            self.mode,
            self.stack.len(),
        );
        true
    }

    fn parse_group(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b'(') {
            return false;
        }
        let start = self.pos;

        if self.bytes.get(start + 1) != Some(&b'?') {
            let kind = if self.mode.captures_by_default {
                RegexGroupKind::Capturing
            } else {
                RegexGroupKind::NonCapturing
            };
            return self.open_group(start, start + 1, kind, self.mode);
        }

        if self.bytes.get(start + 2) == Some(&b'#') {
            return self.parse_group_comment(start);
        }
        if self.bytes.get(start + 2) == Some(&b'{') {
            return self.parse_embedded_code(start, start + 2, RegexEmbeddedCodeKind::Immediate, 3);
        }
        if self.bytes.get(start + 2) == Some(&b'?')
            && self.bytes.get(start + 3) == Some(&b'{')
        {
            return self.parse_embedded_code(start, start + 3, RegexEmbeddedCodeKind::Deferred, 4);
        }

        match self.bytes.get(start + 2).copied() {
            Some(b':') => {
                return self.open_group(
                    start,
                    start + 3,
                    RegexGroupKind::NonCapturing,
                    self.mode,
                );
            }
            Some(b'=') => {
                return self.open_group(
                    start,
                    start + 3,
                    RegexGroupKind::Lookahead,
                    self.mode,
                );
            }
            Some(b'!') => {
                return self.open_group(
                    start,
                    start + 3,
                    RegexGroupKind::NegativeLookahead,
                    self.mode,
                );
            }
            Some(b'>') => {
                return self.open_group(
                    start,
                    start + 3,
                    RegexGroupKind::Atomic,
                    self.mode,
                );
            }
            Some(b'|') => {
                return self.open_group(
                    start,
                    start + 3,
                    RegexGroupKind::BranchReset,
                    self.mode,
                );
            }
            Some(b'<') => return self.parse_angle_group(start),
            Some(b'\'') => return self.parse_quoted_name_group(start),
            Some(b'P') if self.bytes.get(start + 3) == Some(&b'<') => {
                return self.parse_python_name_group(start);
            }
            _ => {}
        }

        if let Some(inline) = self.parse_inline_modifier_prefix(start) {
            return match inline.terminator {
                InlineModifierTerminator::Scope => {
                    self.open_group(
                        start,
                        inline.end,
                        RegexGroupKind::ModifierScope,
                        inline.mode,
                    )
                }
                InlineModifierTerminator::Change => {
                    if !self.advance(inline.end) {
                        return true;
                    }
                    self.mode = inline.mode;
                    let _ = self.emit(
                        start,
                        inline.end,
                        RegexEventKind::ModeChange,
                        self.mode,
                        self.stack.len(),
                    );
                    true
                }
            };
        }

        self.open_group(start, start + 2, RegexGroupKind::Special, self.mode)
    }

    fn parse_group_comment(&mut self, start: usize) -> bool {
        let mut cursor = start + 3;
        let mut closed = false;
        while cursor < self.bytes.len() {
            if self.bytes[cursor] == b')' {
                cursor += 1;
                closed = true;
                break;
            }
            cursor = self.next_char_end(cursor);
        }
        if !closed {
            self.malformed = true;
        }
        if !self.advance(cursor) {
            return true;
        }
        let _ = self.emit(
            start,
            cursor,
            RegexEventKind::Comment(RegexCommentKind::Group),
            self.mode,
            self.stack.len(),
        );
        if !closed {
            let _ = self.emit(
                cursor,
                cursor,
                RegexEventKind::Malformed(RegexMalformedKind::UnterminatedComment),
                self.mode,
                self.stack.len(),
            );
        }
        true
    }

    fn parse_embedded_code(
        &mut self,
        start: usize,
        brace_offset: usize,
        kind: RegexEmbeddedCodeKind,
        opener_width: usize,
    ) -> bool {
        let (end, closed) = self.embedded_code_end(brace_offset);
        if !closed {
            self.malformed = true;
        }
        if !self.advance(end) {
            return true;
        }
        let opener_range = RegexRange {
            start,
            end: start.saturating_add(opener_width).min(self.bytes.len()),
        };
        let _ = self.emit(
            start,
            end,
            RegexEventKind::EmbeddedCode { kind, opener_range, closed },
            self.mode,
            self.stack.len(),
        );
        if !closed {
            let _ = self.emit(
                end,
                end,
                RegexEventKind::Malformed(RegexMalformedKind::UnterminatedEmbeddedCode),
                self.mode,
                self.stack.len(),
            );
        }
        true
    }

    fn parse_angle_group(&mut self, start: usize) -> bool {
        match self.bytes.get(start + 3).copied() {
            Some(b'=') => self.open_group(
                start,
                start + 4,
                RegexGroupKind::Lookbehind,
                self.mode,
            ),
            Some(b'!') => self.open_group(
                start,
                start + 4,
                RegexGroupKind::NegativeLookbehind,
                self.mode,
            ),
            _ => self.open_named_group(start, start + 3, b'>'),
        }
    }

    fn parse_quoted_name_group(&mut self, start: usize) -> bool {
        self.open_named_group(start, start + 3, b'\'')
    }

    fn parse_python_name_group(&mut self, start: usize) -> bool {
        self.open_named_group(start, start + 4, b'>')
    }

    fn open_named_group(&mut self, start: usize, name_start: usize, close: u8) -> bool {
        let mut cursor = name_start;
        while cursor < self.bytes.len() && self.bytes[cursor] != close {
            cursor = self.next_char_end(cursor);
        }
        if cursor >= self.bytes.len() {
            self.malformed = true;
            let end = self.bytes.len();
            if self.advance(end) {
                let _ = self.emit(
                    start,
                    end,
                    RegexEventKind::Malformed(RegexMalformedKind::UnterminatedNamedCapture),
                    self.mode,
                    self.stack.len(),
                );
            }
            return true;
        }
        let name_range = RegexRange { start: name_start, end: cursor };
        self.open_group(
            start,
            cursor + 1,
            RegexGroupKind::NamedCapture { name_range },
            self.mode,
        )
    }

    fn open_group(
        &mut self,
        start: usize,
        end: usize,
        kind: RegexGroupKind,
        inner_mode: RegexModeState,
    ) -> bool {
        if self.stack.len() >= self.limits.max_depth {
            self.exhausted = Some(RegexEventBudget::Nesting);
            return true;
        }
        let restore_mode = self.mode;
        if !self.advance(end) {
            return true;
        }
        self.mode = inner_mode;
        self.stack.push(GroupFrame { kind, restore_mode });
        let _ = self.emit(
            start,
            end,
            RegexEventKind::GroupOpen(kind),
            self.mode,
            self.stack.len(),
        );
        true
    }

    fn parse_group_close(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b')') {
            return false;
        }
        let start = self.pos;
        let end = start + 1;
        if !self.advance(end) {
            return true;
        }
        if let Some(frame) = self.stack.pop() {
            let mode_inside = self.mode;
            let _ = self.emit(
                start,
                end,
                RegexEventKind::GroupClose(frame.kind),
                mode_inside,
                self.stack.len() + 1,
            );
            self.mode = frame.restore_mode;
        } else {
            self.malformed = true;
            let _ = self.emit(
                start,
                end,
                RegexEventKind::Malformed(RegexMalformedKind::UnmatchedGroupClose),
                self.mode,
                0,
            );
        }
        true
    }

    fn parse_alternation(&mut self) -> bool {
        if self.bytes.get(self.pos) != Some(&b'|') {
            return false;
        }
        let start = self.pos;
        let end = start + 1;
        if !self.advance(end) {
            return true;
        }
        let _ = self.emit(
            start,
            end,
            RegexEventKind::Alternation,
            self.mode,
            self.stack.len(),
        );
        true
    }

    fn parse_quantifier(&mut self) -> bool {
        let start = self.pos;
        let Some((quantifier, end)) = self.quantifier_at(start) else {
            return false;
        };
        if !self.advance(end) {
            return true;
        }
        let _ = self.emit(
            start,
            end,
            RegexEventKind::Quantifier(quantifier),
            self.mode,
            self.stack.len(),
        );
        true
    }

    fn parse_interpolation(&mut self) -> bool {
        let sigil = match self.bytes.get(self.pos).copied() {
            Some(b'$' | b'@') => self.bytes[self.pos],
            _ => return false,
        };
        let start = self.pos;
        let Some(next) = self.bytes.get(start + 1).copied() else {
            return false;
        };
        let dynamic = if next == b'{' {
            true
        } else if sigil == b'$' {
            next.is_ascii_alphanumeric()
                || next == b'_'
                || matches!(next, b'$' | b'@' | b'%' | b'&' | b'`' | b'\'')
        } else {
            next.is_ascii_alphabetic() || next == b'_'
        };
        if !dynamic {
            return false;
        }

        let end = if next == b'{' {
            self.braced_interpolation_end(start + 1)
        } else if next.is_ascii_alphanumeric() || next == b'_' {
            self.identifier_interpolation_end(start + 1)
        } else {
            (start + 2).min(self.bytes.len())
        };
        if !self.advance(end) {
            return true;
        }
        let _ = self.emit(
            start,
            end,
            RegexEventKind::Interpolation,
            self.mode,
            self.stack.len(),
        );
        true
    }

    fn quantifier_at(&self, start: usize) -> Option<(RegexQuantifier, usize)> {
        let (lower, upper, mut end) = match self.bytes.get(start).copied()? {
            b'?' => (0, Some(1), start + 1),
            b'*' => (0, None, start + 1),
            b'+' => (1, None, start + 1),
            b'{' => self.brace_quantifier(start)?,
            _ => return None,
        };
        let mode = match self.bytes.get(end).copied() {
            Some(b'?') => {
                end += 1;
                RegexQuantifierMode::Lazy
            }
            Some(b'+') => {
                end += 1;
                RegexQuantifierMode::Possessive
            }
            _ => RegexQuantifierMode::Greedy,
        };
        Some((RegexQuantifier { lower, upper, mode }, end))
    }

    fn brace_quantifier(&self, start: usize) -> Option<(usize, Option<usize>, usize)> {
        let mut cursor = start + 1;
        let (lower, next) = parse_decimal(self.bytes, cursor)?;
        cursor = next;
        match self.bytes.get(cursor).copied()? {
            b'}' => Some((lower, Some(lower), cursor + 1)),
            b',' => {
                cursor += 1;
                if self.bytes.get(cursor) == Some(&b'}') {
                    return Some((lower, None, cursor + 1));
                }
                let (upper, next) = parse_decimal(self.bytes, cursor)?;
                cursor = next;
                if self.bytes.get(cursor) != Some(&b'}') || upper < lower {
                    return None;
                }
                Some((lower, Some(upper), cursor + 1))
            }
            _ => None,
        }
    }

    fn parse_inline_modifier_prefix(&self, start: usize) -> Option<InlineModifierPrefix> {
        let mut cursor = start + 2;
        let mut saw_modifier = false;
        let mut disabling = false;
        let mut enable_x = 0usize;
        let mut disable_x = false;
        let mut enable_n = false;
        let mut disable_n = false;

        if self.bytes.get(cursor) == Some(&b'-') {
            disabling = true;
            cursor += 1;
        }

        while let Some(ch) = self.bytes.get(cursor).copied() {
            if ch == b'-' && !disabling {
                disabling = true;
                cursor += 1;
                continue;
            }
            if !is_inline_modifier(ch) {
                break;
            }
            saw_modifier = true;
            match (disabling, ch) {
                (false, b'x') => enable_x = enable_x.saturating_add(1),
                (true, b'x') => disable_x = true,
                (false, b'n') => enable_n = true,
                (true, b'n') => disable_n = true,
                _ => {}
            }
            cursor += 1;
        }

        if !saw_modifier {
            return None;
        }
        let terminator = match self.bytes.get(cursor).copied()? {
            b':' => InlineModifierTerminator::Scope,
            b')' => InlineModifierTerminator::Change,
            _ => return None,
        };
        let mut mode = self.mode;
        if disable_x {
            mode.extended = RegexExtendedMode::Off;
        } else if enable_x >= 2 {
            mode.extended = RegexExtendedMode::ExtraExtended;
        } else if enable_x == 1 {
            mode.extended = RegexExtendedMode::Extended;
        }
        if disable_n {
            mode.captures_by_default = true;
        } else if enable_n {
            mode.captures_by_default = false;
        }
        Some(InlineModifierPrefix { end: cursor + 1, mode, terminator })
    }

    fn embedded_code_end(&self, brace_offset: usize) -> (usize, bool) {
        let mut cursor = brace_offset + 1;
        let mut depth = 1usize;
        let mut quote = None;
        let mut escaped = false;

        while cursor < self.bytes.len() {
            let ch = self.bytes[cursor];
            if let Some(active_quote) = quote {
                if escaped {
                    escaped = false;
                } else if ch == b'\\' {
                    escaped = true;
                } else if ch == active_quote {
                    quote = None;
                }
                cursor += 1;
                continue;
            }
            match ch {
                b'\'' | b'"' => quote = Some(ch),
                b'{' => depth = depth.saturating_add(1),
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        cursor += 1;
                        if self.bytes.get(cursor) == Some(&b')') {
                            cursor += 1;
                        }
                        return (cursor, true);
                    }
                }
                _ => {}
            }
            cursor = self.next_char_end(cursor);
        }
        (self.bytes.len(), false)
    }

    fn braced_interpolation_end(&self, open: usize) -> usize {
        let mut cursor = open + 1;
        let mut depth = 1usize;
        let mut escaped = false;
        while cursor < self.bytes.len() {
            let ch = self.bytes[cursor];
            if escaped {
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else if ch == b'{' {
                depth = depth.saturating_add(1);
            } else if ch == b'}' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return cursor + 1;
                }
            }
            cursor = self.next_char_end(cursor);
        }
        self.bytes.len()
    }

    fn identifier_interpolation_end(&self, start: usize) -> usize {
        let mut cursor = start;
        while let Some(ch) = self.bytes.get(cursor).copied() {
            if ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b':' | b'\'') {
                cursor += 1;
            } else {
                break;
            }
        }
        cursor
    }

    fn next_char_end(&self, start: usize) -> usize {
        self.pattern
            .get(start..)
            .and_then(|rest| rest.chars().next())
            .map_or(self.bytes.len(), |ch| start.saturating_add(ch.len_utf8()))
    }

    fn advance(&mut self, end: usize) -> bool {
        if end < self.pos || end > self.bytes.len() {
            self.exhausted = Some(RegexEventBudget::Steps);
            return false;
        }
        let additional = end - self.pos;
        let Some(steps) = self.steps.checked_add(additional) else {
            self.exhausted = Some(RegexEventBudget::Steps);
            return false;
        };
        if steps > self.limits.max_steps {
            self.exhausted = Some(RegexEventBudget::Steps);
            return false;
        }
        self.steps = steps;
        self.pos = end;
        true
    }

    fn emit(
        &mut self,
        start: usize,
        end: usize,
        kind: RegexEventKind,
        mode: RegexModeState,
        depth: usize,
    ) -> bool {
        if self.events.len() >= self.limits.max_events {
            self.exhausted = Some(RegexEventBudget::Events);
            return false;
        }
        self.events.push(RegexEvent {
            kind,
            range: RegexRange { start, end },
            mode,
            depth,
        });
        true
    }
}

#[derive(Debug, Clone, Copy)]
struct InlineModifierPrefix {
    end: usize,
    mode: RegexModeState,
    terminator: InlineModifierTerminator,
}

#[derive(Debug, Clone, Copy)]
enum InlineModifierTerminator {
    Scope,
    Change,
}

fn parse_decimal(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start;
    let mut value = 0usize;
    let mut saw_digit = false;
    while let Some(ch) = bytes.get(cursor).copied() {
        if !ch.is_ascii_digit() {
            break;
        }
        saw_digit = true;
        value = value.checked_mul(10)?.checked_add(usize::from(ch - b'0'))?;
        cursor += 1;
    }
    saw_digit.then_some((value, cursor))
}

fn is_inline_modifier(ch: u8) -> bool {
    matches!(ch, b'i' | b'm' | b's' | b'x' | b'a' | b'd' | b'l' | b'u' | b'p' | b'n')
}

#[cfg(test)]
mod tests {
    use super::{
        RegexEventBudget, RegexEventKind, RegexEventLimits, RegexExtendedMode,
        RegexModeState, RegexQuantifier, RegexQuantifierMode, parse_regex_events,
        parse_regex_events_with_limits,
    };

    #[test]
    fn quantifier_bounds_and_modes_are_normalized() -> Result<(), Box<dyn std::error::Error>> {
        let stream = parse_regex_events("a?b{0,1}c{1}d{2}e{2,5}?f++", RegexModeState::default());
        let quantifiers = stream
            .events
            .iter()
            .filter_map(|event| match event.kind {
                RegexEventKind::Quantifier(quantifier) => Some(quantifier),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            quantifiers,
            vec![
                RegexQuantifier { lower: 0, upper: Some(1), mode: RegexQuantifierMode::Greedy },
                RegexQuantifier { lower: 0, upper: Some(1), mode: RegexQuantifierMode::Greedy },
                RegexQuantifier { lower: 1, upper: Some(1), mode: RegexQuantifierMode::Greedy },
                RegexQuantifier { lower: 2, upper: Some(2), mode: RegexQuantifierMode::Greedy },
                RegexQuantifier { lower: 2, upper: Some(5), mode: RegexQuantifierMode::Lazy },
                RegexQuantifier { lower: 1, upper: None, mode: RegexQuantifierMode::Possessive },
            ]
        );
        assert!(!quantifiers[0].repeats_atom());
        assert!(!quantifiers[2].is_variable());
        assert!(quantifiers[3].repeats_atom());
        assert!(!quantifiers[3].is_backtracking());
        assert!(quantifiers[4].is_backtracking());
        assert!(!quantifiers[5].is_backtracking());
        Ok(())
    }

    #[test]
    fn extended_comments_and_local_mode_changes_are_source_backed()
    -> Result<(), Box<dyn std::error::Error>> {
        let initial = RegexModeState {
            extended: RegexExtendedMode::Extended,
            captures_by_default: true,
        };
        let stream = parse_regex_events("# hidden (?{ x })\n(?-x:#(?{ y }))", initial);
        let embedded = stream
            .events
            .iter()
            .filter(|event| matches!(event.kind, RegexEventKind::EmbeddedCode { .. }))
            .collect::<Vec<_>>();
        assert_eq!(embedded.len(), 1);
        assert!(embedded[0].range.start > 20);
        assert!(!embedded[0].mode.extended.enabled());
        Ok(())
    }

    #[test]
    fn event_and_depth_budgets_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let event_limited = parse_regex_events_with_limits(
            "abcdef",
            RegexModeState::default(),
            RegexEventLimits { max_events: 2, max_depth: 10, max_steps: 100 },
        );
        assert_eq!(event_limited.exhausted, Some(RegexEventBudget::Events));

        let depth_limited = parse_regex_events_with_limits(
            "(((a)))",
            RegexModeState::default(),
            RegexEventLimits { max_events: 100, max_depth: 2, max_steps: 100 },
        );
        assert_eq!(depth_limited.exhausted, Some(RegexEventBudget::Nesting));

        let step_limited = parse_regex_events_with_limits(
            "abcdef",
            RegexModeState::default(),
            RegexEventLimits { max_events: 100, max_depth: 10, max_steps: 2 },
        );
        assert_eq!(step_limited.exhausted, Some(RegexEventBudget::Steps));
        Ok(())
    }
}
