//! Server-Sent Events frame parser for streaming AI responses.
//!
//! Every read on this path is bounded by an [`AiResponseBudget`]. The reader
//! counts each response byte it consumes and refuses a line *before* extending
//! its buffer past the line limit, so a peer that never emits a delimiter
//! cannot grow an allocation the way [`std::io::BufRead::read_line`] would.

use super::budget::{AiResponseBudget, BudgetKind, BudgetViolation};
use std::io::{self, BufRead};

/// A parsed SSE event.
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// Event type (from `event:` field). Defaults to "message".
    pub event: String,
    /// Event data (from `data:` field(s), joined by newlines).
    pub data: String,
}

/// Carry a budget violation through the `io::Error` channel this path already
/// uses, without flattening it to a message: the typed value travels as the
/// error's source and is recovered by [`budget_violation`].
pub(crate) fn budget_io_error(violation: BudgetViolation) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, violation)
}

/// Recover the typed violation from an error produced by [`budget_io_error`].
///
/// Returns `None` for an ordinary transport or IO failure.
pub(crate) fn budget_violation(error: &io::Error) -> Option<BudgetViolation> {
    error.get_ref().and_then(|source| source.downcast_ref::<BudgetViolation>()).copied()
}

/// A line reader that never allocates an unbounded line.
///
/// It works from [`BufRead::fill_buf`], whose window is owned by the
/// underlying buffered reader, and copies at most the line limit out of it. A
/// delimiter-free response therefore terminates with a
/// [`BudgetKind::LineBytes`] violation instead of consuming memory until the
/// peer relents.
pub(crate) struct BoundedLineReader<R: BufRead> {
    inner: R,
    budget: AiResponseBudget,
    consumed: u64,
    line: Vec<u8>,
}

impl<R: BufRead> BoundedLineReader<R> {
    /// Wrap `inner`, bounding every read by `budget`.
    pub(crate) fn new(inner: R, budget: AiResponseBudget) -> Self {
        Self { inner, budget, consumed: 0, line: Vec::new() }
    }

    /// Bytes consumed from the response body so far.
    ///
    /// Observation for proof only: it is how a test distinguishes a bound
    /// enforced while reading from one applied after the fact.
    #[cfg(test)]
    pub(crate) const fn consumed(&self) -> u64 {
        self.consumed
    }

    /// The line most recently read, with its `\n` or `\r\n` terminator removed.
    pub(crate) fn line(&self) -> &[u8] {
        &self.line
    }

    /// Read the next line into the internal buffer.
    ///
    /// Returns `Ok(false)` at end of stream. A final line without a trailing
    /// newline is returned as a line.
    ///
    /// # Errors
    ///
    /// Propagates IO failures, and returns a [`budget_io_error`] when the
    /// response would cross its total-byte or per-line limit.
    pub(crate) fn next_line(&mut self) -> Result<bool, io::Error> {
        self.line.clear();
        loop {
            let available = match self.inner.fill_buf() {
                Ok(available) => available,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };

            if available.is_empty() {
                // End of stream: a pending unterminated line is still a line.
                return Ok(!self.line.is_empty());
            }

            // `position` yields an in-range index, so `head` is always a
            // prefix of the window and `take` never exceeds its length.
            let (head, take, terminated) = match available.iter().position(|byte| *byte == b'\n') {
                Some(index) => (&available[..index], index.saturating_add(1), true),
                None => (available, available.len(), false),
            };

            // Total response allowance: refuse before consuming past it, so a
            // body exactly at the limit still reads and one byte over does not.
            let would_consume = self.consumed.saturating_add(take as u64);
            if let Err(violation) = self.budget.check(BudgetKind::ResponseBytes, would_consume) {
                return Err(budget_io_error(violation));
            }

            // Line allowance: checked before the copy, never after it.
            let would_extend = (self.line.len() as u64).saturating_add(head.len() as u64);
            if let Err(violation) = self.budget.check(BudgetKind::LineBytes, would_extend) {
                return Err(budget_io_error(violation));
            }

            self.line.extend_from_slice(head);
            self.inner.consume(take);
            self.consumed = would_consume;

            if terminated {
                // A CRLF terminator leaves its carriage return behind.
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                return Ok(true);
            }
        }
    }
}

/// Parse SSE frames from a byte stream reader.
///
/// Yields events one at a time. Handles:
/// - Multi-line `data:` fields (joined by newline)
/// - Comment lines (`:` prefix, ignored)
/// - Keepalive empty lines
/// - `[DONE]` sentinel
pub struct SseParser<R: BufRead> {
    reader: BoundedLineReader<R>,
    budget: AiResponseBudget,
    done: bool,
    events_yielded: u64,
}

impl<R: BufRead> SseParser<R> {
    /// Create a new SSE parser under the compiled default budget.
    pub fn new(reader: R) -> Self {
        Self::with_budget(reader, AiResponseBudget::compiled_default())
    }

    /// Create a new SSE parser under an explicit budget.
    pub(crate) fn with_budget(reader: R, budget: AiResponseBudget) -> Self {
        Self {
            reader: BoundedLineReader::new(reader, budget),
            budget,
            done: false,
            events_yielded: 0,
        }
    }

    /// Bytes consumed from the response body so far. See
    /// [`BoundedLineReader::consumed`].
    #[cfg(test)]
    pub(crate) const fn consumed(&self) -> u64 {
        self.reader.consumed()
    }

    /// Stop the parser and report `violation` through the IO error channel.
    fn refuse(&mut self, violation: BudgetViolation) -> io::Error {
        self.done = true;
        budget_io_error(violation)
    }

    /// Read the next SSE event. Returns None when stream is done.
    ///
    /// # Errors
    ///
    /// Propagates IO failures, and returns a [`budget_io_error`] when the
    /// response crosses a response-byte, line-byte, event-byte, or event-count
    /// limit. A refused event is terminal: no truncated event is emitted.
    pub fn next_event(&mut self) -> Result<Option<SseEvent>, io::Error> {
        if self.done {
            return Ok(None);
        }

        let mut event_type = String::from("message");
        let mut data_lines: Vec<String> = Vec::new();
        let mut data_bytes: u64 = 0;
        let mut has_data = false;

        loop {
            // Any read failure — a transport error or a response-byte/line-byte
            // refusal — ends the response. Latching here keeps every violation
            // kind terminal, not just the ones this loop raises itself.
            let has_line = self.reader.next_line().inspect_err(|_| self.done = true)?;
            if !has_line {
                self.done = true;
                if has_data {
                    break;
                }
                return Ok(None);
            }

            // The line is already bounded by the line budget, so this decode
            // allocates at most that much. Accumulating raw bytes for the
            // whole line first keeps a multibyte character split across two
            // buffer refills intact.
            let line = String::from_utf8_lossy(self.reader.line());
            let line = line.trim_end_matches('\r');

            // Empty line = event boundary
            if line.is_empty() {
                if has_data {
                    break;
                }
                continue;
            }

            // Comment line
            if line.starts_with(':') {
                continue;
            }

            // Parse field
            if let Some(value) = line.strip_prefix("event:") {
                event_type = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("data:") {
                let value = value.trim();
                if value == "[DONE]" {
                    self.done = true;
                    if has_data {
                        break;
                    }
                    return Ok(None);
                }
                // Per-event data allowance: checked before the payload is
                // retained, so an oversized event is refused rather than
                // truncated. The `event:` value needs no separate bound; it is
                // overwritten each time and already capped by the line budget.
                //
                // Each retained field costs its value plus the one separator
                // byte `join` will insert after it. Charging that byte is what
                // bounds an event built from empty `data:` fields: by value
                // length alone they are free, so a peer could push millions of
                // `String` entries — and millions of join separators — while
                // this counter stayed at zero, capped only by the far larger
                // whole-response allowance.
                let retained = (value.len() as u64).saturating_add(1);
                let would_retain = data_bytes.saturating_add(retained);
                if let Err(violation) = self.budget.check(BudgetKind::EventBytes, would_retain) {
                    return Err(self.refuse(violation));
                }
                data_bytes = would_retain;
                data_lines.push(value.to_string());
                has_data = true;
            }
            // Ignore unknown fields (id:, retry:, etc.)
        }

        let yielded = self.events_yielded.saturating_add(1);
        if let Err(violation) = self.budget.check(BudgetKind::EventCount, yielded) {
            return Err(self.refuse(violation));
        }
        self.events_yielded = yielded;

        Ok(Some(SseEvent { event: event_type, data: data_lines.join("\n") }))
    }
}

#[cfg(test)]
mod tests {
    use super::{AiResponseBudget, BudgetKind, SseParser, budget_violation};
    use std::io::{BufReader, Cursor, Read};

    /// A reader that supplies `byte` forever without ever emitting a newline.
    ///
    /// This is the discriminating control for the bounded reader: under
    /// `BufRead::read_line` the same stream grows a `String` until the process
    /// dies, so a test that terminates here only does so because the line
    /// budget is enforced before the copy.
    struct EndlessBytes {
        byte: u8,
        served: u64,
    }

    impl Read for EndlessBytes {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            buf.fill(self.byte);
            self.served = self.served.saturating_add(buf.len() as u64);
            Ok(buf.len())
        }
    }

    /// A reader that hands back one byte per `read` call, forcing every
    /// multi-byte sequence to straddle a buffer refill.
    struct OneByteAtATime {
        bytes: Vec<u8>,
        position: usize,
    }

    impl Read for OneByteAtATime {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(slot) = buf.first_mut() else {
                return Ok(0);
            };
            let Some(byte) = self.bytes.get(self.position) else {
                return Ok(0);
            };
            *slot = *byte;
            self.position = self.position.saturating_add(1);
            Ok(1)
        }
    }

    fn narrowed(kind: BudgetKind, value: u64) -> AiResponseBudget {
        let budget = AiResponseBudget::compiled_default().with_limit(kind, value);
        let Ok(budget) = budget else {
            unreachable!("{kind} narrowing to {value} must be within the compiled maximum");
        };
        budget
    }

    #[test]
    fn parse_simple_event() {
        let input = "data: {\"test\": true}\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        assert!(event.is_some());
        let event = event.as_ref();
        assert_eq!(event.map(|e| e.event.as_str()), Some("message"));
        assert_eq!(event.map(|e| e.data.as_str()), Some("{\"test\": true}"));
    }

    #[test]
    fn parse_done_sentinel() {
        let input = "data: [DONE]\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let result = parser.next_event();
        assert!(result.is_ok());
        assert!(result.ok().flatten().is_none());
    }

    #[test]
    fn skip_comments() {
        let input = ": keepalive\ndata: hello\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("hello".to_string()));
    }

    #[test]
    fn multi_data_lines_joined() {
        let input = "data: line1\ndata: line2\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("line1\nline2".to_string()));
    }

    #[test]
    fn custom_event_type() {
        let input = "event: custom\ndata: payload\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        assert!(event.is_some());
        let event = event.as_ref();
        assert_eq!(event.map(|e| e.event.as_str()), Some("custom"));
        assert_eq!(event.map(|e| e.data.as_str()), Some("payload"));
    }

    #[test]
    fn multiple_events() {
        let input = "data: first\n\ndata: second\n\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let e1 = parser.next_event().ok().flatten();
        assert_eq!(e1.map(|e| e.data), Some("first".to_string()));
        let e2 = parser.next_event().ok().flatten();
        assert_eq!(e2.map(|e| e.data), Some("second".to_string()));
        let e3 = parser.next_event();
        assert!(e3.is_ok());
        assert!(e3.ok().flatten().is_none());
    }

    #[test]
    fn empty_stream() {
        let input = "";
        let mut parser = SseParser::new(Cursor::new(input));
        let result = parser.next_event();
        assert!(result.is_ok());
        assert!(result.ok().flatten().is_none());
    }

    #[test]
    fn crlf_terminated_lines_parse_as_lf_terminated_ones_do() {
        let input = "event: custom\r\ndata: payload\r\n\r\n";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        let event = event.as_ref();
        assert_eq!(event.map(|e| e.event.as_str()), Some("custom"));
        assert_eq!(event.map(|e| e.data.as_str()), Some("payload"));
    }

    #[test]
    fn a_final_line_without_a_newline_still_completes_its_event() {
        let input = "data: trailing";
        let mut parser = SseParser::new(Cursor::new(input));
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("trailing".to_string()));
    }

    #[test]
    fn a_delimiter_free_stream_is_refused_after_bounded_consumption() {
        // Without the line bound this call does not return.
        let budget = narrowed(BudgetKind::LineBytes, 4_096);
        let mut parser =
            SseParser::with_budget(BufReader::new(EndlessBytes { byte: b'x', served: 0 }), budget);

        let Err(error) = parser.next_event() else {
            unreachable!("a delimiter-free stream must not produce an event");
        };
        let violation = budget_violation(&error);
        assert_eq!(violation.map(|v| v.kind), Some(BudgetKind::LineBytes));
        assert_eq!(violation.map(|v| v.limit), Some(4_096));

        // Enforcement happened while reading, not after buffering the stream:
        // consumption stayed within one buffered window of the limit.
        assert!(
            parser.consumed() <= 4_096 + 8_192,
            "reader consumed {} bytes for a 4096-byte line limit",
            parser.consumed()
        );
    }

    #[test]
    fn a_response_exactly_at_the_total_limit_is_read() {
        let body = "data: hello\n\n";
        let budget = narrowed(BudgetKind::ResponseBytes, body.len() as u64);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("hello".to_string()));
    }

    #[test]
    fn a_response_one_byte_over_the_total_limit_is_refused() {
        let body = "data: hello\n\n";
        let budget = narrowed(BudgetKind::ResponseBytes, (body.len() as u64) - 1);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let Err(error) = parser.next_event() else {
            unreachable!("an over-limit response must not produce an event");
        };
        assert_eq!(budget_violation(&error).map(|v| v.kind), Some(BudgetKind::ResponseBytes));
        // Terminal for every violation kind, including those the reader
        // raises: the parser must not resume reading a refused response.
        assert!(matches!(parser.next_event(), Ok(None)));
    }

    #[test]
    fn an_event_over_its_byte_limit_is_refused_rather_than_truncated() {
        let body = "data: 0123456789\ndata: 0123456789\n\n";
        let budget = narrowed(BudgetKind::EventBytes, 12);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let Err(error) = parser.next_event() else {
            unreachable!("an over-limit event must not be emitted");
        };
        assert_eq!(budget_violation(&error).map(|v| v.kind), Some(BudgetKind::EventBytes));
        // Terminal: the parser does not resume with a partial event.
        assert!(matches!(parser.next_event(), Ok(None)));
    }

    #[test]
    fn an_event_exactly_at_its_byte_limit_is_emitted() {
        // Ten payload bytes plus the one separator byte the field is charged
        // for: eleven is the exact cost, and it must be admitted.
        let body = "data: 0123456789\n\n";
        let budget = narrowed(BudgetKind::EventBytes, 11);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("0123456789".to_string()));
    }

    #[test]
    fn a_field_costs_one_byte_more_than_its_value() {
        // One byte under the exact cost is refused — the boundary control for
        // the separator charge above.
        let body = "data: 0123456789\n\n";
        let budget = narrowed(BudgetKind::EventBytes, 10);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let Err(error) = parser.next_event() else {
            unreachable!("a field costing eleven must not fit a limit of ten");
        };
        assert_eq!(budget_violation(&error).map(|v| v.kind), Some(BudgetKind::EventBytes));
    }

    #[test]
    fn an_event_built_from_empty_data_fields_is_still_bounded() {
        // By value length alone these fields are free, so without the
        // separator charge this event would grow one `String` entry and one
        // join separator per line until the whole-response allowance — orders
        // of magnitude above the event limit — finally stopped it.
        let body = "data:\n".repeat(64);
        let budget = narrowed(BudgetKind::EventBytes, 8);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        let Err(error) = parser.next_event() else {
            unreachable!("empty fields must not escape the event budget");
        };
        let violation = budget_violation(&error);
        assert_eq!(violation.map(|v| v.kind), Some(BudgetKind::EventBytes));
        assert_eq!(violation.map(|v| v.observed_at_least), Some(9));
    }

    #[test]
    fn many_tiny_events_are_refused_once_the_count_limit_is_crossed() {
        let body = "data: a\n\n".repeat(64);
        let budget = narrowed(BudgetKind::EventCount, 3);
        let mut parser = SseParser::with_budget(Cursor::new(body), budget);
        for _ in 0..3 {
            assert!(parser.next_event().ok().flatten().is_some());
        }
        let Err(error) = parser.next_event() else {
            unreachable!("the fourth event must cross a limit of three");
        };
        let violation = budget_violation(&error);
        assert_eq!(violation.map(|v| v.kind), Some(BudgetKind::EventCount));
        assert_eq!(violation.map(|v| v.observed_at_least), Some(4));
    }

    #[test]
    fn a_multibyte_character_split_across_reads_survives_intact() {
        // Every byte arrives in its own `read`, so the three-byte snowman and
        // the four-byte emoji each straddle a refill boundary.
        let body = "data: \u{2603}\u{1F980}\n\n".as_bytes().to_vec();
        let reader = BufReader::new(OneByteAtATime { bytes: body, position: 0 });
        let mut parser = SseParser::new(reader);
        let event = parser.next_event().ok().flatten();
        assert_eq!(event.map(|e| e.data), Some("\u{2603}\u{1F980}".to_string()));
    }

    #[test]
    fn an_ordinary_io_error_is_not_reported_as_a_budget_violation() {
        // Negative control: `budget_violation` must not claim every failure.
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"))
            }
        }
        let mut parser = SseParser::new(BufReader::new(Failing));
        let Err(error) = parser.next_event() else {
            unreachable!("a failing transport must surface an error");
        };
        assert_eq!(budget_violation(&error), None);
    }
}
