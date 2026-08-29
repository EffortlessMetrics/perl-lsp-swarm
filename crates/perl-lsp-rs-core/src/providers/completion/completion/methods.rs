//! Method completion for Perl
//!
//! Provides context-aware method completion including DBI and common client APIs.

use super::lexical_context::{is_in_comment, is_in_heredoc, is_in_pod, is_in_regex, is_in_string};
use super::scope_distance;
use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use perl_lexer::find_data_marker_byte_lexed;
use perl_semantic_analyzer::symbol::{Symbol, SymbolKind, SymbolTable};
use std::borrow::Cow;
use std::collections::HashSet;

/// Extract the receiver module name from the completion prefix for static calls.
///
/// For `LWP::UserAgent->ge` the prefix is `LWP::UserAgent->ge` and we extract
/// `LWP::UserAgent`.  Returns `None` when the receiver is a variable (`$obj->`)
/// or when the prefix has no `->`.
fn static_receiver_module(prefix: &str) -> Option<&str> {
    let arrow = prefix.rfind("->")?;
    let receiver = prefix[..arrow].trim();
    // Static receivers start with an uppercase ASCII letter and contain no sigil.
    if !receiver.starts_with('$')
        && !receiver.starts_with('@')
        && !receiver.starts_with('%')
        && receiver.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        Some(receiver)
    } else {
        None
    }
}

/// DBI database handle methods
pub const DBI_DB_METHODS: &[(&str, &str)] = &[
    ("do", "Execute a single SQL statement"),
    ("prepare", "Prepare a SQL statement"),
    ("prepare_cached", "Prepare and cache a SQL statement"),
    ("selectrow_array", "Execute and fetch a single row as array"),
    ("selectrow_arrayref", "Execute and fetch a single row as arrayref"),
    ("selectrow_hashref", "Execute and fetch a single row as hashref"),
    ("selectall_arrayref", "Execute and fetch all rows as arrayref"),
    ("selectall_hashref", "Execute and fetch all rows as hashref"),
    ("begin_work", "Begin a database transaction"),
    ("commit", "Commit the current transaction"),
    ("rollback", "Rollback the current transaction"),
    ("disconnect", "Disconnect from the database"),
    ("last_insert_id", "Get the last inserted row ID"),
    ("quote", "Quote a string for SQL"),
    ("quote_identifier", "Quote an identifier for SQL"),
    ("ping", "Check if database connection is alive"),
];

/// DBI statement handle methods
pub const DBI_ST_METHODS: &[(&str, &str)] = &[
    ("bind_param", "Bind a parameter to the statement"),
    ("bind_param_inout", "Bind an in/out parameter"),
    ("execute", "Execute the prepared statement"),
    ("fetch", "Fetch the next row as arrayref"),
    ("fetchrow_array", "Fetch the next row as array"),
    ("fetchrow_arrayref", "Fetch the next row as arrayref"),
    ("fetchrow_hashref", "Fetch the next row as hashref"),
    ("fetchall_arrayref", "Fetch all remaining rows as arrayref"),
    ("fetchall_hashref", "Fetch all remaining rows as hashref of hashrefs"),
    ("finish", "Finish the statement handle"),
    ("rows", "Get the number of rows affected"),
];

/// Parameter signatures for DBI database-handle methods.
///
/// Each entry is `(name, signature, description)`.
pub const DBI_DB_METHOD_SIGS: &[(&str, &str, &str)] = &[
    ("do", "do($statement, \\@attr?, @bind_values?)", "Execute a single SQL statement"),
    ("prepare", "prepare($statement, \\@attr?)", "Prepare a SQL statement for execution"),
    (
        "prepare_cached",
        "prepare_cached($statement, \\@attr?, $if_active?)",
        "Prepare and cache a SQL statement",
    ),
    (
        "selectrow_array",
        "selectrow_array($statement, \\@attr?, @bind)",
        "Execute and return first row as list",
    ),
    (
        "selectrow_arrayref",
        "selectrow_arrayref($statement, \\@attr?, @bind)",
        "Execute and return first row as arrayref",
    ),
    (
        "selectrow_hashref",
        "selectrow_hashref($statement, \\@attr?, @bind)",
        "Execute and return first row as hashref",
    ),
    (
        "selectall_arrayref",
        "selectall_arrayref($statement, \\@attr?, @bind)",
        "Execute and return all rows as arrayref",
    ),
    (
        "selectall_hashref",
        "selectall_hashref($statement, $key_field, \\@attr?, @bind)",
        "Execute and return all rows as hashref",
    ),
    ("begin_work", "begin_work()", "Begin a database transaction"),
    ("commit", "commit()", "Commit the current transaction"),
    ("rollback", "rollback()", "Rollback the current transaction"),
    ("disconnect", "disconnect()", "Disconnect from the database"),
    (
        "last_insert_id",
        "last_insert_id($catalog, $schema, $table, $field, \\@attr?)",
        "Get the last inserted row ID",
    ),
    ("quote", "quote($value, $data_type?)", "Quote a string value for use in SQL"),
    ("quote_identifier", "quote_identifier($name)", "Quote an identifier for SQL"),
    ("ping", "ping()", "Check if the database connection is still alive"),
];

/// Parameter signatures for DBI statement-handle methods.
///
/// Each entry is `(name, signature, description)`.
pub const DBI_ST_METHOD_SIGS: &[(&str, &str, &str)] = &[
    (
        "bind_param",
        "bind_param($param_num, $bind_value, \\@attr?)",
        "Bind a value to a placeholder",
    ),
    (
        "bind_param_inout",
        "bind_param_inout($param_num, \\$bind_value, $max_len)",
        "Bind an in/out parameter",
    ),
    ("execute", "execute(@bind_values?)", "Execute the prepared statement"),
    ("fetch", "fetch()", "Fetch the next row as arrayref (alias for fetchrow_arrayref)"),
    ("fetchrow_array", "fetchrow_array()", "Fetch the next row as a list"),
    ("fetchrow_arrayref", "fetchrow_arrayref()", "Fetch the next row as an arrayref"),
    ("fetchrow_hashref", "fetchrow_hashref($name?)", "Fetch the next row as a hashref"),
    (
        "fetchall_arrayref",
        "fetchall_arrayref($slice?, $max_rows?)",
        "Fetch all remaining rows as arrayref",
    ),
    (
        "fetchall_hashref",
        "fetchall_hashref($key_field)",
        "Fetch all remaining rows as hashref of hashrefs",
    ),
    ("finish", "finish()", "Indicate no more rows will be fetched"),
    ("rows", "rows()", "Return the number of rows affected or returned"),
];

/// Static methods documented by `Mojo::Pg`.
pub const MOJO_PG_METHODS: &[(&str, &str)] = &[("new", "Create a Mojo::Pg database wrapper")];

/// Static methods documented by `Mojo::mysql`.
pub const MOJO_MYSQL_METHODS: &[(&str, &str)] = &[
    ("new", "Create a Mojo::mysql database wrapper"),
    ("strict_mode", "Create a database wrapper with strict mode enabled"),
];

/// Static methods documented by `HTTP::Tiny`.
pub const HTTP_TINY_STATIC_METHODS: &[(&str, &str)] = &[
    ("new", "Create an HTTP::Tiny client"),
    ("can_ssl", "Check whether SSL support is available"),
];

/// Common instance methods documented by `HTTP::Tiny`.
pub const HTTP_TINY_METHODS: &[(&str, &str)] = &[
    ("get", "Send an HTTP GET request"),
    ("head", "Send an HTTP HEAD request"),
    ("put", "Send an HTTP PUT request"),
    ("post", "Send an HTTP POST request"),
    ("patch", "Send an HTTP PATCH request"),
    ("delete", "Send an HTTP DELETE request"),
    ("post_form", "Send form data with an HTTP POST request"),
    ("mirror", "Mirror a URL to a local file"),
    ("request", "Send a request with an explicit HTTP method"),
    ("www_form_urlencode", "Encode form data for a query or request body"),
    ("can_ssl", "Check whether SSL support is available"),
    ("connected", "Report the current keep-alive peer"),
];

/// Common instance methods documented by `LWP::UserAgent`.
///
/// `put` and `delete` are real `LWP::UserAgent` instance methods since LWP
/// 6.56 (2023); verified against a live Perl oracle (`perl -MLWP::UserAgent`,
/// LWP 6.82: `defined *LWP::UserAgent::put{CODE}` / `...::delete{CODE}`).
pub const LWP_USER_AGENT_METHODS: &[(&str, &str)] = &[
    ("request", "Send an HTTP request"),
    ("simple_request", "Send one HTTP request without redirects"),
    ("get", "Send an HTTP GET request"),
    ("head", "Send an HTTP HEAD request"),
    ("post", "Send an HTTP POST request"),
    ("put", "Send an HTTP PUT request"),
    ("delete", "Send an HTTP DELETE request"),
    ("mirror", "Mirror a URL to a local file"),
    ("agent", "Get or set the user-agent string"),
    ("cookie_jar", "Get or set the cookie jar"),
    ("credentials", "Set credentials for a protection space"),
    ("default_header", "Get or set one default request header"),
    ("default_headers", "Get or set default request headers"),
    ("max_redirect", "Get or set the redirect limit"),
    ("max_size", "Get or set the response-size limit"),
    ("parse_head", "Get or set HTML head parsing"),
    ("requests_redirectable", "Get or set redirectable request methods"),
    ("ssl_opts", "Get or set SSL options"),
    ("timeout", "Get or set the request timeout"),
    ("proxy", "Get or set a proxy for protocols"),
    ("no_proxy", "Add domains that bypass the proxy"),
    ("env_proxy", "Load proxy settings from the environment"),
    ("clone", "Clone the user agent"),
    ("is_protocol_supported", "Check whether a protocol is supported"),
];

/// Static constructors and factories documented by `Path::Tiny`.
pub const PATH_TINY_STATIC_METHODS: &[(&str, &str)] = &[
    ("new", "Create a Path::Tiny object"),
    ("cwd", "Return the current directory as an absolute Path::Tiny object"),
    ("rootdir", "Return the filesystem root as a Path::Tiny object"),
    ("tempfile", "Create a temporary file path"),
    ("tempdir", "Create a temporary directory path"),
];

/// Documented, non-deprecated `Path::Tiny` instance methods.
pub const PATH_TINY_METHODS: &[(&str, &str)] = &[
    ("absolute", "Return an absolute path"),
    ("append", "Append data to a file"),
    ("append_raw", "Append raw bytes to a file"),
    ("append_utf8", "Append UTF-8 text to a file"),
    ("assert", "Assert a condition and return the path"),
    ("basename", "Return the final path component"),
    ("cached_temp", "Return the cached temporary-file object"),
    ("canonpath", "Return the platform-canonical path string"),
    ("child", "Return a child path"),
    ("children", "List child paths"),
    ("chmod", "Set file or directory permissions"),
    ("copy", "Copy the path to a destination"),
    ("digest", "Calculate a file digest"),
    ("edit", "Edit a file through a callback"),
    ("edit_lines", "Edit file lines through a callback"),
    ("edit_lines_raw", "Edit raw file lines through a callback"),
    ("edit_lines_utf8", "Edit UTF-8 file lines through a callback"),
    ("edit_raw", "Edit a raw file through a callback"),
    ("edit_utf8", "Edit a UTF-8 file through a callback"),
    ("exists", "Check whether the path exists"),
    ("filehandle", "Open and return a file handle"),
    ("has_same_bytes", "Compare file contents byte for byte"),
    ("is_absolute", "Check whether the path is absolute"),
    ("is_dir", "Check whether the path is a directory"),
    ("is_file", "Check whether the path is a non-directory file"),
    ("is_relative", "Check whether the path is relative"),
    ("is_rootdir", "Check whether the path is a filesystem root"),
    ("iterator", "Return a lazy directory iterator"),
    ("lines", "Read file contents as lines"),
    ("lines_raw", "Read raw file contents as lines"),
    ("lines_utf8", "Read UTF-8 file contents as lines"),
    ("lstat", "Return lstat metadata for the path"),
    ("mkdir", "Create the directory and missing parents"),
    ("move", "Move the path to a destination"),
    ("opena", "Open a file handle for appending"),
    ("opena_raw", "Open a raw file handle for appending"),
    ("opena_utf8", "Open a UTF-8 file handle for appending"),
    ("openr", "Open a file handle for reading"),
    ("openr_raw", "Open a raw file handle for reading"),
    ("openr_utf8", "Open a UTF-8 file handle for reading"),
    ("openrw", "Open a file handle for reading and writing"),
    ("openrw_raw", "Open a raw file handle for reading and writing"),
    ("openrw_utf8", "Open a UTF-8 file handle for reading and writing"),
    ("openw", "Open a file handle for writing"),
    ("openw_raw", "Open a raw file handle for writing"),
    ("openw_utf8", "Open a UTF-8 file handle for writing"),
    ("parent", "Return a parent path"),
    ("realpath", "Resolve the path against the filesystem"),
    ("relative", "Return a path relative to another base"),
    ("remove", "Remove a file path"),
    ("remove_tree", "Remove a directory tree"),
    ("sibling", "Return a sibling path"),
    ("size", "Return file size in bytes"),
    ("size_human", "Return a human-readable file size"),
    ("slurp", "Read an entire file"),
    ("slurp_raw", "Read an entire file as raw bytes"),
    ("slurp_utf8", "Read an entire file as UTF-8 text"),
    ("spew", "Write an entire file atomically"),
    ("spew_raw", "Write raw bytes atomically"),
    ("spew_utf8", "Write UTF-8 text atomically"),
    ("stat", "Return stat metadata for the path"),
    ("stringify", "Return the normalized path string"),
    ("subsumes", "Check whether this path contains another path"),
    ("tempdir", "Create a temporary directory under this path"),
    ("tempfile", "Create a temporary file under this path"),
    ("touch", "Create the file or update its timestamps"),
    ("touchpath", "Create missing parents and touch the file"),
    ("visit", "Visit directory descendants through a callback"),
    ("volume", "Return the path volume component"),
];

const GENERIC_OBJECT_METHODS: &[(&str, &str)] = &[
    ("new", "Constructor"),
    ("isa", "Check if object is of given class"),
    ("can", "Check if object can call method"),
    ("DOES", "Check if object does role"),
    ("VERSION", "Get version"),
    ("DESTROY", "Called when the last reference to the object is released (garbage collected)"),
    ("AUTOLOAD", "Automatic method dispatcher for undefined methods"),
];

/// Look up DBI method documentation by receiver hint and method name.
///
/// `receiver_hint` is the variable name or token before `->` (e.g. `"$dbh"`, `"$sth"`).
/// Returns `(signature, description)` or `None` if not a known DBI method.
///
/// When the receiver is ambiguous, database-handle methods take priority.
pub fn get_dbi_method_documentation(
    receiver_hint: &str,
    method_name: &str,
) -> Option<(&'static str, &'static str)> {
    let is_db = receiver_hint.ends_with("dbh")
        || receiver_hint.contains("DBI")
        || receiver_hint.contains("connect");
    let is_st = receiver_hint.ends_with("sth");

    let table: &[(&str, &str, &str)] = if is_db {
        DBI_DB_METHOD_SIGS
    } else if is_st {
        DBI_ST_METHOD_SIGS
    } else {
        // Unknown receiver — check db table first, then st table
        if let Some(entry) = DBI_DB_METHOD_SIGS.iter().find(|(n, _, _)| *n == method_name) {
            return Some((entry.1, entry.2));
        }
        DBI_ST_METHOD_SIGS
    };

    table.iter().find(|(n, _, _)| *n == method_name).map(|(_, sig, desc)| (*sig, *desc))
}

/// Infer receiver type from context (for DBI method completion)
pub fn infer_receiver_type(context: &CompletionContext, source: &str) -> Option<String> {
    // Look backwards from the position to find the receiver
    let prefix = context.receiver_prefix().trim_end_matches("->");

    // Simple heuristics for DBI types based on variable name
    if prefix.ends_with("$dbh") {
        return Some("DBI::db".to_string());
    }
    if prefix.ends_with("$sth") {
        return Some("DBI::st".to_string());
    }

    // Look at the broader context - check if variable was assigned from DBI->connect
    if let Some(var_pos) = source.rfind(prefix) {
        // Look backwards for assignment
        let before_var = &source[..var_pos];
        if let Some(assign_pos) = before_var.rfind('=') {
            let assignment = &source[assign_pos..var_pos + prefix.len()];

            // Check if this looks like DBI->connect result
            if assignment.contains("DBI") && assignment.contains("connect") {
                return Some("DBI::db".to_string());
            }

            // Check if this looks like prepare/prepare_cached result
            if assignment.contains("prepare") {
                return Some("DBI::st".to_string());
            }
        }
    }

    None
}

fn is_code_position(source: &str, position: usize) -> bool {
    !(is_in_string(source, position)
        || is_in_comment(source, position)
        || is_in_heredoc(source, position)
        || is_in_regex(source, position)
        || is_in_pod(source, position)
        || find_data_marker_byte_lexed(source).is_some_and(|marker| position >= marker))
}

fn is_module_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

fn any_module_use_before(
    source: &str,
    position: usize,
    module: &str,
    mut predicate: impl FnMut(&str) -> bool,
) -> bool {
    let source = source.get(..position).unwrap_or(source);
    let mut search_start = 0usize;

    while let Some(relative_use) = source[search_start..].find("use") {
        let use_start = search_start + relative_use;
        let after_use_start = use_start + "use".len();
        search_start = after_use_start;

        if !is_code_position(source, use_start)
            || source[..use_start].chars().next_back().is_some_and(is_module_identifier_char)
        {
            continue;
        }

        let after_use = &source[after_use_start..];
        let trimmed_after_use = after_use.trim_start_matches(char::is_whitespace);
        if trimmed_after_use.len() == after_use.len() {
            continue;
        }

        let module_start = after_use_start + after_use.len() - trimmed_after_use.len();
        let Some(after_module) = source[module_start..].strip_prefix(module) else {
            continue;
        };
        if after_module.chars().next().is_some_and(is_module_identifier_char) {
            continue;
        }

        let arguments_start = module_start + module.len();
        let statement_end = source[arguments_start..].char_indices().find_map(|(relative, ch)| {
            let index = arguments_start + relative;
            (ch == ';' && is_code_position(source, index)).then_some(index)
        });
        let Some(statement_end) = statement_end else { continue };

        if predicate(source[arguments_start..statement_end].trim()) {
            return true;
        }
        search_start = statement_end + 1;
    }

    false
}

fn looks_like_version_argument(argument: &str) -> bool {
    let version = argument.strip_prefix('v').unwrap_or(argument);
    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
}

fn strip_optional_version_argument(arguments: &str) -> &str {
    let arguments = arguments.trim();
    let first_end = arguments.find(char::is_whitespace).unwrap_or(arguments.len());
    let first = &arguments[..first_end];
    if looks_like_version_argument(first) { arguments[first_end..].trim_start() } else { arguments }
}

fn module_was_used_before(source: &str, position: usize, module: &str) -> bool {
    any_module_use_before(source, position, module, |_| true)
}

fn module_imported_symbol_before(
    source: &str,
    position: usize,
    module: &str,
    symbol: &str,
) -> bool {
    any_module_use_before(source, position, module, |arguments| {
        let stripped = strip_line_comments_outside_lists(arguments);
        let arguments = strip_optional_version_argument(&stripped);
        if arguments.is_empty() {
            return true;
        }

        arguments
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .any(|word| word == symbol || word == ":all")
    })
}

/// Drop `#`-to-end-of-line comments that sit outside bracketed import lists,
/// so `use Path::Tiny # load defaults\n;` still reads as a default import
/// while `#` stays literal inside `qw( ... )` lists.
fn strip_line_comments_outside_lists(arguments: &str) -> String {
    let mut stripped = String::with_capacity(arguments.len());
    let mut depth = 0usize;
    for ch in arguments.chars() {
        match ch {
            '(' => {
                depth += 1;
                stripped.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                stripped.push(ch);
            }
            '#' if depth == 0 => break,
            _ => stripped.push(ch),
        }
    }
    stripped
}

fn binding_at_position<'a>(
    symbol_table: &'a SymbolTable,
    receiver: &str,
    scope_id: usize,
    position: usize,
) -> Option<&'a Symbol> {
    let name = receiver.strip_prefix('$')?;
    let candidates = symbol_table.find_symbol(name, scope_id, SymbolKind::scalar());
    let mut visible = candidates.into_iter().filter(|symbol| symbol.location.start <= position);
    let first = visible.next()?;
    let defining_scope = first.scope_id;
    std::iter::once(first)
        .chain(visible)
        .filter(|symbol| symbol.scope_id == defining_scope)
        .max_by_key(|symbol| symbol.location.start)
}

fn latest_assignment_for_binding<'a>(
    symbol_table: &SymbolTable,
    source: &'a str,
    receiver: &str,
    binding: &Symbol,
    cursor_scope_id: usize,
    end: usize,
) -> Option<&'a str> {
    let scope_end = symbol_table
        .scopes
        .get(&binding.scope_id)
        .map(|scope| scope.location.end)
        .filter(|scope_end| *scope_end > binding.location.start)
        .unwrap_or(end)
        .min(end);
    let search_start = binding.location.start.min(scope_end);
    let mut expression = None;

    for (offset, _) in source[search_start..scope_end].match_indices(receiver) {
        let receiver_pos = search_start + offset;
        if !is_code_position(source, receiver_pos) {
            continue;
        }
        let occurrence_scope =
            scope_distance::scope_at_position(symbol_table, source, receiver_pos);
        let Some(occurrence_binding) =
            binding_at_position(symbol_table, receiver, occurrence_scope, receiver_pos)
        else {
            continue;
        };
        if occurrence_binding.scope_id != binding.scope_id
            || occurrence_binding.location.start != binding.location.start
        {
            continue;
        }
        if assignment_is_in_unrelated_subroutine(symbol_table, occurrence_scope, cursor_scope_id) {
            continue;
        }

        let after_receiver = source[receiver_pos + receiver.len()..].trim_start();
        let Some((assignment, compound)) = assignment_after_receiver(after_receiver) else {
            if is_list_assignment_target(after_receiver) {
                // A list assignment also replaces the receiver's value, even though
                // the assignment operator is not immediately after the scalar.
                expression = Some("");
            }
            continue;
        };
        if compound {
            // Compound assignment changes the receiver's value, so an earlier
            // constructor assignment is no longer reliable type evidence.
            expression = Some("");
            continue;
        }
        let statement_end = assignment.find(';').unwrap_or(assignment.len());
        expression = Some(assignment[..statement_end].trim());
    }

    expression
}

fn assignment_is_in_unrelated_subroutine(
    symbol_table: &SymbolTable,
    occurrence_scope_id: usize,
    cursor_scope_id: usize,
) -> bool {
    let mut current = occurrence_scope_id;
    while let Some(scope) = symbol_table.scopes.get(&current) {
        if scope.kind == perl_semantic_analyzer::symbol::ScopeKind::Subroutine {
            let mut cursor_scope = cursor_scope_id;
            while let Some(cursor) = symbol_table.scopes.get(&cursor_scope) {
                if cursor.id == scope.id {
                    return false;
                }
                let Some(parent) = cursor.parent else {
                    break;
                };
                cursor_scope = parent;
            }
            return true;
        }
        let Some(parent) = scope.parent else {
            break;
        };
        current = parent;
    }
    false
}

fn assignment_after_receiver(after_receiver: &str) -> Option<(&str, bool)> {
    for operator in [
        "**=", "<<=", ">>=", "&&=", "||=", "//=", ".=", "x=", "+=", "-=", "*=", "/=", "%=", "&=",
        "|=", "^=", "=",
    ] {
        if let Some(rhs) = after_receiver.strip_prefix(operator) {
            if operator == "=" && rhs.chars().next().is_some_and(|c| matches!(c, '=' | '>' | '~')) {
                return None;
            }
            return Some((rhs, operator != "="));
        }
    }
    None
}

fn is_list_assignment_target(after_receiver: &str) -> bool {
    let Some(equal_pos) = after_receiver.find('=') else {
        return false;
    };
    let left_hand_side = after_receiver[..equal_pos].trim();
    left_hand_side.starts_with(',') || left_hand_side.starts_with(')')
}

fn expression_calls_static_method(expression: &str, module: &str, method: &str) -> bool {
    let Some(after_module) = expression.strip_prefix(module) else {
        return false;
    };
    let Some(after_arrow) = after_module.trim_start().strip_prefix("->") else {
        return false;
    };
    let Some(after_method) = after_arrow.trim_start().strip_prefix(method) else {
        return false;
    };

    match after_method.chars().next() {
        None => true,
        Some('(') => call_arguments_end_expression(after_method),
        Some(c) if c.is_whitespace() => call_ends_at_indirect_arguments(after_method),
        _ => false,
    }
}

fn expression_calls_function(expression: &str, function: &str) -> bool {
    let Some(after_function) = expression.strip_prefix(function) else {
        return false;
    };

    match after_function.chars().next() {
        Some('(') => call_arguments_end_expression(after_function),
        Some(c) if c.is_whitespace() => call_ends_at_indirect_arguments(after_function),
        _ => false,
    }
}

/// Arming evidence for an indirect (whitespace-separated) argument form:
/// parenthesized arguments must close and end the expression, while paren-less
/// arguments terminate with the expression itself.
fn call_ends_at_indirect_arguments(after_name: &str) -> bool {
    let argument = after_name.trim_start();
    if argument.starts_with('(') {
        return call_arguments_end_expression(argument);
    }
    !argument.is_empty() && !argument.starts_with("=>")
}

/// Whether a call's argument list both closes and ends the expression: after
/// the balanced close parenthesis (quote/escape aware) only whitespace may
/// follow. A method-call chain continuing past the call
/// (`path("x")->stringify`) produces a derived plain value, so it rejects and
/// factory evidence never arms a catalog for the wrong receiver type.
fn call_arguments_end_expression(after_name: &str) -> bool {
    let after_name = after_name.trim_start();
    if !after_name.starts_with('(') {
        return after_name.is_empty();
    }

    let mut depth = 0usize;
    let mut escaped = false;
    let mut quote = None;
    for (index, byte) in after_name.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return after_name[index + 1..].trim().is_empty();
                }
            }
            _ => {}
        }
    }
    false
}

fn expression_calls_constructor(expression: &str, module: &str) -> bool {
    let Some(after_module) = expression.strip_prefix(module) else {
        return false;
    };
    let Some(after_arrow) = after_module.trim_start().strip_prefix("->") else {
        return false;
    };
    let Some(after_new) = after_arrow.trim_start().strip_prefix("new") else {
        return false;
    };

    call_arguments_end_expression(after_new)
}

/// Infer a `Path::Tiny` receiver type from imported factory evidence.
///
/// Claim bound (#13192): factory evidence is gated on an active `Path::Tiny`
/// import, including version-only (`use Path::Tiny 0.150;`) and symbol-list
/// (`use Path::Tiny qw(path);`) forms, and takes priority over constructor
/// evidence and naming heuristics. Exact same-name scalar bindings only;
/// aliases fail closed.
fn infer_imported_factory_receiver_type(
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
) -> Option<&'static str> {
    let receiver = context.receiver_prefix().trim_end_matches("->");
    let source_before_cursor = source.get(..context.position).unwrap_or(source);
    let receiver_pos = source_before_cursor.rfind(receiver)?;
    let binding =
        binding_at_position(symbol_table, receiver, context.cursor_scope_id, receiver_pos)?;
    let expression = latest_assignment_for_binding(
        symbol_table,
        source,
        receiver,
        binding,
        context.cursor_scope_id,
        receiver_pos + receiver.len(),
    )?;

    if module_was_used_before(source, context.position, "Path::Tiny")
        && ((module_imported_symbol_before(source, context.position, "Path::Tiny", "path")
            && expression_calls_function(expression, "path"))
            || ["new", "cwd", "rootdir", "tempfile", "tempdir"]
                .into_iter()
                .any(|method| expression_calls_static_method(expression, "Path::Tiny", method)))
    {
        return Some("Path::Tiny");
    }

    None
}

/// Infer an HTTP client receiver type from the binding's latest constructor
/// assignment.
///
/// Claim bound: this is a bounded local bridge over exact same-name scalar
/// assignments only. `Path::Tiny` factory evidence is owned exclusively by
/// [`infer_imported_factory_receiver_type`], which runs first at dispatch;
/// this inference covers the remaining constructor-built clients
/// (`HTTP::Tiny`, `LWP::UserAgent`) through the hardened
/// ends-at-close-paren boundary matcher. Aliases (`my $alias = $http`) and
/// reblessed/derived namespaces are intentionally unresolved and fail closed
/// (no completion); parser/semantic-flow backing arrives with #13244, which
/// also owns migrating this inference onto the canonical workspace facts.
fn infer_imported_constructor_receiver_type(
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
) -> Option<&'static str> {
    let receiver = context.receiver_prefix().trim_end_matches("->");
    let source_before_cursor = source.get(..context.position).unwrap_or(source);
    let receiver_pos = source_before_cursor.rfind(receiver)?;
    let binding =
        binding_at_position(symbol_table, receiver, context.cursor_scope_id, receiver_pos)?;
    let expression = latest_assignment_for_binding(
        symbol_table,
        source,
        receiver,
        binding,
        context.cursor_scope_id,
        receiver_pos + receiver.len(),
    )?;

    ["HTTP::Tiny", "LWP::UserAgent"].into_iter().find(|&module| {
        module_was_used_before(source, context.position, module)
            && expression_calls_constructor(expression, module)
    })
}

fn imported_static_methods(
    prefix: &str,
    source: &str,
    position: usize,
) -> Option<&'static [(&'static str, &'static str)]> {
    let module = static_receiver_module(prefix)?;
    if !module_was_used_before(source, position, module) {
        return None;
    }

    match module {
        "HTTP::Tiny" => Some(HTTP_TINY_STATIC_METHODS),
        "Mojo::Pg" => Some(MOJO_PG_METHODS),
        "Mojo::mysql" => Some(MOJO_MYSQL_METHODS),
        "Path::Tiny" => Some(PATH_TINY_STATIC_METHODS),
        _ => None,
    }
}

fn known_instance_methods(
    receiver_type: Option<&str>,
) -> Option<&'static [(&'static str, &'static str)]> {
    match receiver_type {
        Some("HTTP::Tiny") => Some(HTTP_TINY_METHODS),
        Some("LWP::UserAgent") => Some(LWP_USER_AGENT_METHODS),
        Some("Path::Tiny") => Some(PATH_TINY_METHODS),
        _ => None,
    }
}

/// Build rich documentation for a Moo/Moose accessor from its symbol attributes.
///
/// Attributes are stored as `key=value` strings (e.g. `"is=ro"`, `"isa=Str"`).
/// This function formats them into a human-readable documentation string that
/// surfaces the attribute metadata prominently.
fn moo_accessor_documentation(name: &str, attributes: &[String]) -> String {
    let isa = moo_accessor_value(attributes, "isa");
    let access = moo_accessor_value(attributes, "is").map(moo_access_mode);
    let required = moo_accessor_value(attributes, "required").map(moo_truthy);
    let predicate = moo_accessor_method_name(name, attributes, "predicate", "has_");
    let builder = moo_accessor_method_name(name, attributes, "builder", "_build_");
    let clearer = moo_accessor_method_name(name, attributes, "clearer", "clear_");
    let reader = moo_accessor_value(attributes, "reader");
    let writer = moo_accessor_value(attributes, "writer");
    let accessor = moo_accessor_value(attributes, "accessor");
    let lazy = moo_accessor_value(attributes, "lazy").map(moo_truthy);
    let default = moo_accessor_value(attributes, "default");

    let mut doc = format!("Moo/Moose accessor `{name}`\n\n**Attribute**: `{name}`");

    if let Some(isa) = isa {
        doc.push_str(&format!("\n**Type**: `{isa}`"));
    }
    if let Some(access) = access {
        doc.push_str(&format!("\n**Access**: {access}"));
    }
    if let Some(required) = required {
        doc.push_str(&format!("\n**Required**: {required}"));
    }
    if let Some(predicate) = predicate {
        doc.push_str(&format!("\n**Predicate**: `{predicate}`"));
    }
    if let Some(builder) = builder {
        doc.push_str(&format!("\n**Builder**: `{builder}`"));
    }
    if let Some(clearer) = clearer {
        doc.push_str(&format!("\n**Clearer**: `{clearer}`"));
    }
    if let Some(reader) = reader {
        doc.push_str(&format!("\n**Reader**: `{reader}`"));
    }
    if let Some(writer) = writer {
        doc.push_str(&format!("\n**Writer**: `{writer}`"));
    }
    if let Some(accessor) = accessor {
        doc.push_str(&format!("\n**Accessor**: `{accessor}`"));
    }
    if let Some(lazy) = lazy {
        doc.push_str(&format!("\n**Lazy**: {lazy}"));
    }
    if let Some(default) = default {
        doc.push_str(&format!("\n**Default**: `{default}`"));
    }

    let extras: Vec<String> = attributes
        .iter()
        .filter_map(|attr| {
            let (key, _) = attr.split_once('=')?;
            if matches!(
                key,
                "isa"
                    | "is"
                    | "required"
                    | "predicate"
                    | "builder"
                    | "clearer"
                    | "reader"
                    | "writer"
                    | "accessor"
                    | "lazy"
                    | "default"
            ) {
                None
            } else {
                Some(attr.clone())
            }
        })
        .collect();
    if !extras.is_empty() {
        doc.push_str(&format!("\n**Options**: {}", extras.join(", ")));
    }

    doc
}

fn moo_accessor_value<'a>(attributes: &'a [String], key: &str) -> Option<&'a str> {
    attributes.iter().find_map(|attr| {
        let (attr_key, value) = attr.split_once('=')?;
        if attr_key == key { Some(value) } else { None }
    })
}

fn moo_access_mode(value: &str) -> String {
    match value {
        "ro" => "read-only".to_string(),
        "rw" => "read-write".to_string(),
        "rwp" => "read-write private".to_string(),
        "lazy" => "lazy".to_string(),
        other => other.to_string(),
    }
}

fn moo_truthy(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => "yes".to_string(),
        "0" | "false" | "no" => "no".to_string(),
        other => other.to_string(),
    }
}

fn moo_accessor_method_name(
    name: &str,
    attributes: &[String],
    key: &str,
    default_prefix: &str,
) -> Option<String> {
    let value = moo_accessor_value(attributes, key)?;
    if moo_is_truthy(value) {
        Some(format!("{default_prefix}{name}"))
    } else {
        Some(value.to_string())
    }
}

fn moo_is_truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

/// Add method completions
pub fn add_method_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
) {
    let mut seen: HashSet<&str> = HashSet::new();

    // Prefer discovered in-file methods first (including synthesized framework accessors).
    let method_prefix = context.prefix.rsplit("->").next().unwrap_or(&context.prefix);
    for (name, symbols) in &symbol_table.symbols {
        let is_callable = symbols
            .iter()
            .any(|symbol| matches!(symbol.kind, SymbolKind::Subroutine | SymbolKind::Method));
        if !is_callable {
            continue;
        }

        if !method_prefix.is_empty() && !name.starts_with(method_prefix) {
            continue;
        }

        // Check if this is a synthesized Moo/Moose accessor (declaration == "has")
        let callable_symbol = symbols
            .iter()
            .find(|symbol| matches!(symbol.kind, SymbolKind::Subroutine | SymbolKind::Method));

        let is_moo_accessor =
            callable_symbol.and_then(|s| s.declaration.as_deref()).is_some_and(|d| d == "has");

        let (detail, documentation) = if is_moo_accessor {
            let attrs = callable_symbol.map(|s| s.attributes.as_slice()).unwrap_or(&[]);
            ("Moo/Moose accessor".to_string(), Some(moo_accessor_documentation(name, attrs)))
        } else {
            let doc = symbols.iter().find_map(|symbol| symbol.documentation.clone());
            ("method".to_string(), doc)
        };

        if seen.insert(name.as_str()) {
            completions.push(CompletionItem {
                label: Cow::Owned(name.clone()),
                kind: super::items::CompletionItemKind::Function,
                detail: Some(Cow::Owned(detail)),
                documentation: documentation.map(Cow::Owned),
                insert_text: Some(Cow::Owned(format!("{}()", name))),
                sort_text: Some(Cow::Owned(format!("1_{}", name))),
                filter_text: Some(Cow::Owned(name.clone())),
                additional_edits: vec![],
                text_edit_range: Some((context.method_text_edit_start(source), context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }

    // Exact imported factory evidence, then constructor evidence, take
    // priority over naming heuristics.
    let receiver_type = infer_imported_factory_receiver_type(context, source, symbol_table)
        .or_else(|| infer_imported_constructor_receiver_type(context, source, symbol_table))
        .map(str::to_owned)
        .or_else(|| infer_receiver_type(context, source));

    let static_api_methods =
        imported_static_methods(context.receiver_prefix(), source, context.position);
    let instance_api_methods = known_instance_methods(receiver_type.as_deref());

    // Choose methods based on inferred type
    let methods: Vec<(&str, &str)> = if let Some(methods) = static_api_methods {
        let mut methods = methods.to_vec();
        methods.extend_from_slice(GENERIC_OBJECT_METHODS);
        methods
    } else if let Some(methods) = instance_api_methods {
        let mut methods = methods.to_vec();
        methods.extend_from_slice(GENERIC_OBJECT_METHODS);
        methods
    } else {
        match receiver_type.as_deref() {
            Some("DBI::db") => DBI_DB_METHODS.to_vec(),
            Some("DBI::st") => DBI_ST_METHODS.to_vec(),
            _ => GENERIC_OBJECT_METHODS.to_vec(),
        }
    };

    for (method, desc) in methods {
        let is_static_api_method = static_api_methods
            .is_some_and(|catalog| catalog.iter().any(|(name, _)| *name == method));
        let is_instance_api_method = instance_api_methods
            .is_some_and(|catalog| catalog.iter().any(|(name, _)| *name == method));
        if (is_static_api_method || is_instance_api_method)
            && !method_prefix.is_empty()
            && !method.starts_with(method_prefix)
        {
            continue;
        }
        if seen.insert(method) {
            completions.push(CompletionItem {
                label: Cow::Owned(method.to_string()),
                kind: super::items::CompletionItemKind::Function,
                detail: Some(Cow::Borrowed("method")),
                documentation: Some(Cow::Owned(desc.to_string())),
                insert_text: Some(Cow::Owned(format!("{}()", method))),
                sort_text: Some(Cow::Owned(format!("2_{}", method))),
                filter_text: Some(Cow::Owned(method.to_string())),
                additional_edits: vec![],
                text_edit_range: Some((context.method_text_edit_start(source), context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }

    // If we have a DBI type, also add common methods at lower priority
    if receiver_type.as_deref() == Some("DBI::db") || receiver_type.as_deref() == Some("DBI::st") {
        for (method, desc) in [
            ("isa", "Check if object is of given class"),
            ("can", "Check if object can call method"),
        ] {
            if seen.insert(method) {
                completions.push(CompletionItem {
                    label: Cow::Owned(method.to_string()),
                    kind: super::items::CompletionItemKind::Function,
                    detail: Some(Cow::Borrowed("method")),
                    documentation: Some(Cow::Owned(desc.to_string())),
                    insert_text: Some(Cow::Owned(format!("{}()", method))),
                    sort_text: Some(Cow::Owned(format!("9_{}", method))), // Lower priority
                    filter_text: Some(Cow::Owned(method.to_string())),
                    additional_edits: vec![],
                    text_edit_range: Some((
                        context.method_text_edit_start(source),
                        context.position,
                    )),
                    commit_characters: None,
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_regions_stay_pod_until_exact_cut() {
        let begin_uncut = "=begin comment\ndocs\n=end comment\nmy $code = 1;";
        assert!(is_in_pod(begin_uncut, begin_uncut.len()));
        assert!(!is_code_position(begin_uncut, begin_uncut.find("my $code").unwrap()));

        let begin_cut = "=begin comment\ndocs\n=end comment\n=cut\nmy $code = 1;";
        assert!(!is_in_pod(begin_cut, begin_cut.len()));
        assert!(is_code_position(begin_cut, begin_cut.find("my $code").unwrap()));

        let for_body = "=for comment\ndocs\n$code";
        assert!(is_in_pod(for_body, for_body.len()));

        let for_blank_line = "=for comment\ndocs\n\nmy $code = 1;";
        assert!(is_in_pod(for_blank_line, for_blank_line.len()));
        assert!(!is_code_position(for_blank_line, for_blank_line.find("my $code").unwrap()));
    }

    #[test]
    fn pod_state_ignores_heredoc_directives_and_invalid_indentation() {
        let source =
            "my $text = <<'END';\n=begin comment\n=for comment\n=end comment\nEND\nmy $code = 1;";
        assert!(!is_in_pod(source, source.len()));
        assert!(is_code_position(source, source.find("my $code").unwrap()));

        let indented = "  =pod\nnot documentation\nmy $code = 1;";
        assert!(!is_in_pod(indented, indented.len()));
        assert!(is_code_position(indented, indented.find("my $code").unwrap()));
    }

    #[test]
    fn targetless_begin_starts_pod_until_cut() {
        let source = "=begin\nnot documentation\nmy $code = 1;";
        assert!(is_in_pod(source, source.len()));
        assert!(!is_code_position(source, source.find("my $code").unwrap()));

        let cut_source = "=begin\nnot documentation\n=cut\nmy $code = 1;";
        assert!(!is_in_pod(cut_source, cut_source.len()));
        assert!(is_code_position(cut_source, cut_source.find("my $code").unwrap()));
    }
}
