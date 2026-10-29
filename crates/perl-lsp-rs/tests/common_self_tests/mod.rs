//! Shared-helper self-tests, registered only by `lsp_test_infrastructure_validation`.

mod binary_resolution;
mod handshake;
mod protocol_io;
mod test_reliability;
mod timeout_scaler;

use crate::common::{
    STDERR_PARTIAL_MAX_BYTES, STDERR_TAIL_LINES, record_stderr_chunk, start_lsp_server,
};

#[cfg(test)]
mod stderr_tests {
    use super::{STDERR_PARTIAL_MAX_BYTES, STDERR_TAIL_LINES, record_stderr_chunk};
    use std::collections::VecDeque;

    #[test]
    fn stderr_fragments_reconstruct_across_reads() {
        let mut tail = (VecDeque::new(), Vec::new());
        assert!(record_stderr_chunk(&mut tail, b"warning: split ").is_empty());
        assert_eq!(
            record_stderr_chunk(&mut tail, b"across reads\nnext"),
            vec!["warning: split across reads"]
        );

        assert_eq!(tail.0.iter().collect::<Vec<_>>(), vec!["warning: split across reads"]);
        assert_eq!(tail.1, b"next");
    }

    #[test]
    fn stderr_preserves_utf8_code_points_split_across_reads() {
        let mut tail = (VecDeque::new(), Vec::new());
        let character = "界".as_bytes();

        assert!(record_stderr_chunk(&mut tail, &character[..1]).is_empty());
        assert_eq!(
            record_stderr_chunk(&mut tail, &[character[1], character[2], b'\n']),
            vec!["界"]
        );
    }

    #[test]
    fn unterminated_stderr_keeps_newest_bounded_bytes() {
        let mut tail = (VecDeque::new(), Vec::new());
        record_stderr_chunk(
            &mut tail,
            format!("old{}new", "x".repeat(STDERR_PARTIAL_MAX_BYTES)).as_bytes(),
        );

        assert!(tail.1.len() <= STDERR_PARTIAL_MAX_BYTES);
        assert!(tail.1.ends_with(b"new"));
        assert!(tail.0.len() <= STDERR_TAIL_LINES);
    }

    #[test]
    fn stderr_echo_lines_survive_tail_eviction() {
        let mut tail = (VecDeque::new(), Vec::new());
        for index in 0..STDERR_TAIL_LINES {
            record_stderr_chunk(&mut tail, format!("line {index}\n").as_bytes());
        }

        let completed = record_stderr_chunk(&mut tail, b"new line\n");

        assert_eq!(completed, vec!["new line"]);
        assert_eq!(tail.0.len(), STDERR_TAIL_LINES);
        assert_eq!(tail.0.front().map(String::as_str), Some("line 1"));
        assert_eq!(tail.0.back().map(String::as_str), Some("new line"));
    }
}
