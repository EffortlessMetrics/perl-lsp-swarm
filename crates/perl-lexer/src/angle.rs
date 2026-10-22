//! Source-aware angle bodies. The caller has consumed the opening `<`.
use crate::{AngleScanDimension, LexerError, LexerMode, PerlLexer};

/// Exact half-open byte geometry of a closed angle term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AngleSpan {
    /// First body byte, immediately after `<`.
    pub body_start: usize,
    /// Closing `>` byte offset.
    pub body_end: usize,
    /// First byte after `>`.
    pub end: usize,
}

impl PerlLexer<'_> {
    /// Cumulative angle work, in `(UTF-8 bytes, Unicode scalar inspections)`.
    #[must_use]
    pub fn angle_scan_usage(&self) -> (usize, usize) {
        (self.angle_scan_bytes, self.angle_scan_steps)
    }

    fn charge_angle(
        &mut self,
        dimension: AngleScanDimension,
        count: usize,
        position: usize,
    ) -> Result<(), LexerError> {
        let (spent, limit) = match dimension {
            AngleScanDimension::Bytes => {
                (&mut self.angle_scan_bytes, self.config.max_angle_scan_bytes)
            }
            AngleScanDimension::Steps => {
                (&mut self.angle_scan_steps, self.config.max_angle_scan_steps)
            }
        };
        if count > limit.saturating_sub(*spent) {
            return Err(LexerError::AngleBudgetExhausted {
                dimension,
                limit,
                usage: *spent,
                position,
            });
        }
        *spent += count;
        Ok(())
    }

    /// Scan the body at the current live boundary without allocating its contents.
    ///
    /// The opener must have been consumed normally. Every inspected scalar and
    /// byte is charged, including escaped lookahead and unsuccessful closure
    /// searches. A missing closer preserves the first punctuation boundary only
    /// after the scan establishes LF/EOF. A resource refusal terminates lexing.
    pub fn scan_angle_body(&mut self) -> Result<AngleSpan, LexerError> {
        let result = self.scan_angle_body_inner();
        if matches!(result, Err(LexerError::AngleBudgetExhausted { .. })) {
            self.position = self.input.len();
        }
        result
    }

    fn scan_angle_body_inner(&mut self) -> Result<AngleSpan, LexerError> {
        let body_start = self.position;
        let mut cursor = body_start;
        let mut escaped = false;
        let mut recovery = None;
        while cursor < self.input.len() {
            if self.angle_scan_steps >= self.config.max_angle_scan_steps {
                return Err(LexerError::AngleBudgetExhausted {
                    dimension: AngleScanDimension::Steps,
                    limit: self.config.max_angle_scan_steps,
                    usage: self.angle_scan_steps,
                    position: cursor,
                });
            }
            self.charge_angle(AngleScanDimension::Bytes, 1, cursor)?;
            self.angle_scan_steps += 1;
            let byte = *self
                .input_bytes
                .get(cursor)
                .ok_or_else(|| LexerError::Other("invalid angle cursor".into()))?;
            // `input` is valid UTF-8 and the cursor advances by complete scalars.
            let width = if byte < 0x80 {
                1
            } else if byte < 0xe0 {
                2
            } else if byte < 0xf0 {
                3
            } else {
                4
            };
            self.charge_angle(AngleScanDimension::Bytes, width - 1, cursor)?;
            if byte == b'\n' {
                break;
            }
            if !escaped && byte == b'>' {
                self.position = cursor + 1;
                self.mode = LexerMode::ExpectOperator;
                self.after_newline = false;
                return Ok(AngleSpan { body_start, body_end: cursor, end: self.position });
            }
            if !escaped && recovery.is_none() && matches!(byte, b';' | b',' | b')' | b']' | b'}') {
                recovery = Some(cursor);
            }
            escaped = !escaped && byte == b'\\';
            cursor += width;
        }
        let boundary = recovery.unwrap_or(cursor);
        self.position = boundary;
        self.mode = LexerMode::ExpectOperator;
        Err(LexerError::UnterminatedAngle {
            position: body_start.saturating_sub(1),
            recovery: boundary,
        })
    }
}
