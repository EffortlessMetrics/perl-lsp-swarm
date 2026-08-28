//! Method completion for Perl
//!
//! Provides context-aware method completion including DBI and common client APIs.

use super::lexical_context::{is_in_comment, is_in_heredoc, is_in_pod, is_in_regex, is_in_string};
use super::scope_distance;
use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
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
        || is_in_pod(source, position))
}

fn binding_at_position<'a>(
    symbol_table: &'a SymbolTable,
    receiver: &str,
    scope_id: usize,
    position: usize,
) -> Option<&'a Symbol> {
    let name = receiver.strip_prefix('$')?;
    let candidates = symbol_table.find_symbol(name, scope_id, SymbolKind::scalar());
    let defining_scope = candidates.first()?.scope_id;
    candidates
        .into_iter()
        .filter(|symbol| symbol.scope_id == defining_scope && symbol.location.start <= position)
        .max_by_key(|symbol| symbol.location.start)
}

fn latest_assignment_for_binding<'a>(
    symbol_table: &SymbolTable,
    source: &'a str,
    receiver: &str,
    binding: &Symbol,
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

        let after_receiver = source[receiver_pos + receiver.len()..].trim_start();
        let Some((assignment, compound)) = assignment_after_receiver(after_receiver) else {
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

fn assignment_after_receiver(after_receiver: &str) -> Option<(&str, bool)> {
    for operator in [
        "**=", "<<=", ">>=", "&&=", "||=", "//=", ".=", "+=", "-=", "*=", "/=", "%=", "&=", "|=",
        "^=", "=",
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

    after_new.chars().next().is_none_or(|c| c == '(' || c.is_whitespace())
}

fn infer_imported_constructor_receiver_type(
    context: &CompletionContext,
    source: &str,
    symbol_table: &SymbolTable,
    used_modules: &HashSet<String>,
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
        receiver_pos + receiver.len(),
    )?;

    ["HTTP::Tiny", "LWP::UserAgent"].into_iter().find(|&module| {
        used_modules.contains(module) && expression_calls_constructor(expression, module)
    })
}

fn imported_static_methods(
    prefix: &str,
    used_modules: &HashSet<String>,
) -> Option<&'static [(&'static str, &'static str)]> {
    let module = static_receiver_module(prefix)?;
    if !used_modules.contains(module) {
        return None;
    }

    match module {
        "HTTP::Tiny" => Some(HTTP_TINY_STATIC_METHODS),
        "Mojo::Pg" => Some(MOJO_PG_METHODS),
        "Mojo::mysql" => Some(MOJO_MYSQL_METHODS),
        _ => None,
    }
}

fn known_instance_methods(
    receiver_type: Option<&str>,
) -> Option<&'static [(&'static str, &'static str)]> {
    match receiver_type {
        Some("HTTP::Tiny") => Some(HTTP_TINY_METHODS),
        Some("LWP::UserAgent") => Some(LWP_USER_AGENT_METHODS),
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
    used_modules: &HashSet<String>,
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

    // Exact imported constructor evidence takes priority over naming heuristics.
    let receiver_type =
        infer_imported_constructor_receiver_type(context, source, symbol_table, used_modules)
            .map(str::to_owned)
            .or_else(|| infer_receiver_type(context, source));

    let static_api_methods = imported_static_methods(context.receiver_prefix(), used_modules);
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
    fn pod_for_is_one_paragraph_and_begin_ends_at_matching_end() {
        let begin = "=begin comment\ndocs\n=end comment\nmy $code = 1;";
        assert!(!is_in_pod(begin, begin.len()));

        let for_body = "=for comment\ndocs\n$code";
        assert!(is_in_pod(for_body, for_body.len()));

        let for_code = "=for comment\ndocs\n\nmy $code = 1;";
        assert!(!is_in_pod(for_code, for_code.len()));
        assert!(is_code_position(for_code, for_code.find("my $code").unwrap()));
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
    fn malformed_begin_does_not_suppress_following_code() {
        let source = "=begin\nnot documentation\nmy $code = 1;";
        assert!(!is_in_pod(source, source.len()));
        assert!(is_code_position(source, source.find("my $code").unwrap()));
    }
}
