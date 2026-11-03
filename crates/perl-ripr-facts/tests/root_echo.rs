//! Root-echo normalization (#17260).
//!
//! #17260 platform rule: on Windows backslash is a separator, so a backslash
//! root echoes forward-slash under the `posix` claim; on Unix backslash is a
//! literal filename char and the emitters scan `Path::new(root)` verbatim, so
//! the echo stays verbatim to name the scanned directory (a verbatim
//! backslash is valid posix). Forward-slash roots are byte-identical on both
//! (goldens green unmodified).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_with};

fn build_packet_with_root(root: &str) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: None,
        head: None,
        fact_classes: "files,owners",
        diff: None,
    }))
}

#[test]
fn backslash_root_echo_is_posix() {
    // Stage the fixture at the forward-slash path; request it with
    // backslashes, as a Windows caller passing a native path would. On Unix
    // the backslash spelling names a different (nonexistent) directory, so the
    // scan finds nothing — the echo assertion below is still the point.
    let root = "target/ripr-g8/echo";
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "mkdir echo fixture");
    must_with(std::fs::write(format!("{root}/lib/App.pm"), "package App;\n1;\n"), "stage App.pm");
    let packet = build_packet_with_root("target\\ripr-g8\\echo");
    let _ = std::fs::remove_dir_all(root);

    #[cfg(windows)]
    assert_eq!(packet["root"]["repo_relative"], "target/ripr-g8/echo");
    #[cfg(not(windows))]
    assert_eq!(packet["root"]["repo_relative"], "target\\ripr-g8\\echo");
    assert_eq!(packet["root"]["path_style"], "posix");
}

#[test]
fn forward_slash_root_echo_is_unchanged() {
    let root = "target/ripr-g8/plain";
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "mkdir plain fixture");
    must_with(std::fs::write(format!("{root}/lib/App.pm"), "package App;\n1;\n"), "stage App.pm");
    let packet = build_packet_with_root(root);
    let _ = std::fs::remove_dir_all(root);

    assert_eq!(packet["root"]["repo_relative"], root);
    assert_eq!(packet["root"]["path_style"], "posix");
}
