/// Resolve a known export tag to its symbol list for a specific module.
///
/// The `tag` argument can be passed with or without a leading `:`.
/// Returns `None` when the module/tag pair is not in the built-in catalog.
#[must_use]
pub fn resolve_known_export_tag(module: &str, tag: &str) -> Option<&'static [&'static str]> {
    let normalized_tag = tag.strip_prefix(':').unwrap_or(tag);
    match (module, normalized_tag) {
        ("POSIX", "sys_wait_h") => Some(&["WIFEXITED", "WEXITSTATUS", "WIFSIGNALED", "WTERMSIG"]),
        ("POSIX", "fcntl_h") => Some(&["F_GETFL", "F_SETFL", "F_SETFD", "F_GETFD"]),
        ("POSIX", "termios_h") => Some(&["TCSANOW", "TCSADRAIN", "TCSAFLUSH", "B9600"]),
        ("File::Find", "find") => Some(&["find", "finddepth"]),
        ("Fcntl", "seek") => Some(&["SEEK_SET", "SEEK_CUR", "SEEK_END"]),
        ("Fcntl", "lock") => Some(&["LOCK_SH", "LOCK_EX", "LOCK_NB", "LOCK_UN"]),
        ("Encode", "fallback") => Some(&["FB_DEFAULT", "FB_CROAK", "FB_QUIET", "FB_WARN"]),
        _ => None,
    }
}
