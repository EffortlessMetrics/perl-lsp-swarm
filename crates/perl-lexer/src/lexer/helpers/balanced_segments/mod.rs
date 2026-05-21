mod scan;

use crate::PerlLexer;
use scan::{consume_balanced_segment_core, SegmentDelimiters};

impl PerlLexer<'_> {
    /// General-purpose balanced-segment consumer (no quote-boundary recovery).
    ///
    /// For use inside double-quoted string interpolation where the outer `"` must
    /// act as a recovery boundary, use [`consume_balanced_segment_in_string`] instead.
    #[allow(dead_code)] // Recovery helper retained for future interpolation callers.
    #[inline]
    pub(crate) fn consume_balanced_segment(&mut self, open: char, close: char) -> Option<usize> {
        consume_balanced_segment_core(
            self,
            SegmentDelimiters {
                open,
                close,
                terminator: None,
            },
        )
    }

    #[inline]
    pub(crate) fn consume_balanced_segment_in_string(
        &mut self,
        open: char,
        close: char,
        terminator: char,
    ) -> Option<usize> {
        consume_balanced_segment_core(
            self,
            SegmentDelimiters {
                open,
                close,
                terminator: Some(terminator),
            },
        )
    }
}
