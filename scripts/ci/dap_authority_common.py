"""Shared types and manifest validation for the DAP authority gate."""

from __future__ import annotations

import hashlib
import json
import re
import urllib.parse
from collections import Counter
from pathlib import Path
from typing import Any, Mapping

MANIFEST_SCHEMA = "dap_protocol_authority.v1"
RECEIPT_SCHEMA = "dap_protocol_authority_receipt.v1"
MAX_SCHEMA_BYTES = 1_048_576
REQUIRED_DEFINITIONS = ("ProtocolMessage", "Request", "Response", "Event")
REQUIRED_FIELDS = {
    "ProtocolMessage": {"seq", "type"},
    "Request": {"seq", "type", "command"},
    "Response": {"seq", "type", "request_seq", "success", "command"},
    "Event": {"seq", "type", "event"},
}
DOC_PATHS = (
    Path("docs/reference/DAP_PROTOCOL_SCHEMA.md"),
    Path("book/src/dap/protocol-schema.md"),
)
DISPATCH_PATH = Path("crates/perl-dap/src/debug_adapter/dispatch.rs")
DEBUG_ADAPTER_ROOT = Path("crates/perl-dap/src/debug_adapter")
PEER_DISPATCH_PATHS = (
    Path("crates/perl-dap/src/backend/peer_bridge.rs"),
    Path("crates/perl-dap/src/backend/peer_launch.rs"),
)
# The pinned compatibility-fallback bodies (#9527, fail-closed split #9069,
# setExpression floor #9568): setExpression — a catalog route unavailable in
# the peer frontends — is refused first with the deterministic
# SET_EXPRESSION_UNSUPPORTED_MESSAGE from the single #9568 authority (a
# generic "unavailable" refusal would not pin the advertised capability and
# the admission gate to one source); any other catalog route that exists but
# is unavailable in the peer frontend must fail closed (success: false, no
# body) — a lenient ack would report success for work no backend performed —
# while a genuinely unknown command keeps the mirror-MVP acknowledgement so a
# client is never wedged. Changing the shape of this arm is a deliberate
# contract edit that updates these constants.
EXPECTED_PEER_FALLBACKS = {
    PEER_DISPATCH_PATHS[0]: (
        "if matches!( DapRequestRoute::from_command(command), "
        "Some(DapRequestRoute::SetExpression) ) { "
        "if crate::backend::capabilities::refuse_set_expression( "
        "self.advertised_set_expression(), ) { "
        "out.push(self.response( request_seq, command, false, None, "
        "Some( crate::backend::capabilities::SET_EXPRESSION_UNSUPPORTED_MESSAGE "
        ".to_string(), ), )); } else { "
        "out.push( self.response( request_seq, command, false, None, "
        'Some( "setExpression: external-peer delegation is not implemented" '
        ".to_string(), ), ), ); } } "
        "else if DapRequestRoute::from_command(command).is_some() { "
        'tracing::warn!(command, "peer bridge: request is unavailable in this frontend"); '
        "out.push(self.response( request_seq, command, false, None, "
        'Some("request is unavailable in the external peer frontend".to_string()), )); '
        "} else { "
        'tracing::warn!(command, "peer bridge: unhandled DAP request"); '
        "out.push(self.response(request_seq, command, true, None, None)); }"
    ),
    PEER_DISPATCH_PATHS[1]: (
        "if matches!( DapRequestRoute::from_command(command), "
        "Some(DapRequestRoute::SetExpression) ) { "
        "if crate::backend::capabilities::refuse_set_expression( "
        "crate::backend::capabilities::MIRROR_ADVERTISES_SET_EXPRESSION, ) { "
        "out.push(self.response( request_seq, command, false, None, "
        "Some( crate::backend::capabilities::SET_EXPRESSION_UNSUPPORTED_MESSAGE "
        ".to_string(), ), )); } else { "
        "out.push( self.response( request_seq, command, false, None, "
        'Some( "setExpression: mirror-mode delegation is not implemented" '
        ".to_string(), ), ), ); } } "
        "else if DapRequestRoute::from_command(command).is_some() { "
        'tracing::warn!( command, "mirror bridge: request is unavailable in this frontend" ); '
        "out.push(self.response( request_seq, command, false, None, "
        'Some("request is unavailable in the mirror peer frontend".to_string()), )); '
        "} else { "
        'tracing::warn!(command, "mirror bridge: unhandled DAP request"); '
        "out.push(self.response(request_seq, command, true, None, None)); }"
    ),
}
FORBIDDEN_DOC_PHRASES = (
    "JSON-RPC 2.0",
    "Schema Definitions Complete",
    "specification:complete",
)
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
# The one executable request authority (#9527). Rows in this table expand
# into both the typed inventory and the `dispatch_request` match arms, so a
# row is production routing rather than a parallel list describing it.
REQUEST_TABLE_INVOCATION_RE = re.compile(r"\bdap_request_table\s*!\s*\{")
REQUEST_TABLE_ROW_RE = re.compile(
    r"(?P<class>standard|extension)\s+"
    r"(?P<availability>all_frontends|native_only)\s+"
    r"(?P<variant>[A-Z][A-Za-z0-9]*)\s+"
    r'"(?P<command>[A-Za-z][A-Za-z0-9._/-]*)"\s*=>\s*'
    r"(?P<handler>[a-z_][a-z0-9_]*)\s*\(\s*(?:arguments\s*)?\)\s*,"
)
# Any string-literal match arm outside the table would be a second executable
# authority. This is a breadth net over the rest of the file; the exclusivity
# proof itself is structural — see `validate_generated_dispatch`.
STRAY_MATCH_ARM_RE = re.compile(r'"(?P<name>[^"\n]*)"\s*(?:if\b[^=\n]*?)?=>')
REQUEST_CLASSES = {"standard", "extension"}

MACRO_DEFINITION_RE = re.compile(r"\bmacro_rules\s*!\s*dap_request_table\b")
ANY_MACRO_DEFINITION_RE = re.compile(r"\bmacro_rules\s*!\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)")
# Only these macros may be defined in the dispatch source. A new generator
# macro here could expand into routing the named-function checks never see,
# so the set is closed and a new one has to be reviewed in deliberately.
PERMITTED_MACRO_DEFINITIONS = frozenset(
    {
        "dap_request_class",
        "dap_request_availability",
        "dap_request_peer_available",
        "dap_dispatch_call",
        "dap_request_table",
    }
)
DISPATCH_FN_RE = re.compile(r"\bfn\s+dispatch_request\b")
PRODUCTION_DISPATCH_FN_RE = re.compile(
    r"\bfn\s+(?:dispatch|dispatch_request|handle_request)\s*\(",
)
# The public `dispatch` wrapper is the capability-admission seam.  The
# table-owned route body is kept in the private `dispatch_unchecked` method so
# callers cannot bypass admission while this authority still pins the exact
# request-table shape.
PEER_DISPATCH_FN_RE = re.compile(r"\b(?:pub\s+)?fn\s+dispatch(?:_unchecked)?\s*\(")
PEER_FLOOR_FN_RE = re.compile(r"\b(?:pub\s+)?fn\s+dispatch_with_capability_floor\s*\(")
PEER_ROUTE_MATCH_RE = re.compile(
    r"\bmatch\s+DapRequestRoute::from_command\s*\(\s*command\s*\)\s*"
    r"\.filter\s*\(\s*DapRequestRoute::available_in_peer_frontends\s*\)"
)
SUPPORTED_COMMANDS_DEFINITION_RE = re.compile(r"\bconst\s+SUPPORTED_COMMANDS\b")
REQUEST_ROWS_DEFINITION_RE = re.compile(r"\bconst\s+DAP_REQUEST_ROWS\b")
# Constructs that could introduce a route inside the generated dispatch body.
# The body is a fixed, tiny shape, so this is an allow-list check rather than
# a hunt for known-bad spellings: anything that can branch, return early, or
# name a wire string is rejected outright.
FORBIDDEN_DISPATCH_BODY_KEYWORDS = ("if", "else", "return", "while", "loop", "for")
# The `command` identifier, ignoring `$command` (the macro row capture) and
# any longer identifier that merely contains the word.
COMMAND_IDENT_RE = re.compile(r"(?<![\w$])command(?!\w)")
# The generated match must scrutinise the command itself, never a value
# derived from it: `match self.normalize(command)` would keep every
# structural rule while remapping wire names away from the rows.
EXPECTED_SCRUTINEE = "command"
# The one generated row arm is pinned just like the fallback below. Merely
# counting its arrow leaves the handler side open to an imported macro, and
# permits a cfg attribute to remove every executable arm while the inventory
# continues to report the table rows.
EXPECTED_ROW_REPETITION = (
    "$( $command => dap_dispatch_call!( self, $handler, seq, request_seq, "
    "arguments, $arity ), )*"
)
EXPECTED_REQUEST_ROWS_PROJECTION = (
    'pub(crate) const DAP_REQUEST_ROWS: &[DapRequestRow] = &[ $( DapRequestRow { '
    'row_id: concat!("dap.request.", $command), command: $command, class: '
    'dap_request_class!($class), availability: '
    'dap_request_availability!($availability), }, )* ];'
)
EXPECTED_SUPPORTED_COMMANDS_PROJECTION = (
    "pub(crate) const SUPPORTED_COMMANDS: [&str; DAP_REQUEST_ROWS.len()] = "
    "[$($command),*];"
)
EXPECTED_ROUTE_ENUM_PROJECTION = "pub(crate) enum DapRequestRoute { $($variant),* }"
EXPECTED_ROUTE_LOOKUP_PROJECTION = (
    "impl DapRequestRoute { "
    "pub(crate) fn from_command(wire_command: &str) -> Option<Self> { "
    "match wire_command { $($command => Some(Self::$variant),)* _ => None, } } "
    "pub(crate) const fn available_in_peer_frontends(&self) -> bool { "
    "match self { $(Self::$variant => "
    "dap_request_peer_available!($availability),)* } } }"
)
EXPECTED_SMALL_MACROS = {
    "dap_request_class": (
        "macro_rules! dap_request_class { (standard) => { "
        "DapRequestClass::Standard }; (extension) => { "
        "DapRequestClass::Extension }; }"
    ),
    "dap_request_availability": (
        "macro_rules! dap_request_availability { (all_frontends) => { "
        "DapRequestAvailability::AllFrontends }; (native_only) => { "
        "DapRequestAvailability::NativeOnly }; }"
    ),
    "dap_request_peer_available": (
        "macro_rules! dap_request_peer_available { (all_frontends) => { true }; "
        "(native_only) => { false }; }"
    ),
}
EXPECTED_ARGUMENTS_DISPATCH_ARM = (
    "($adapter:expr, $handler:ident, $seq:expr, $request_seq:expr, "
    "$arguments:expr, (arguments)) => { "
    "$adapter.$handler($seq, $request_seq, $arguments) };"
)
EXPECTED_NO_ARGUMENTS_DISPATCH_ARM = (
    "($adapter:expr, $handler:ident, $seq:expr, $request_seq:expr, "
    "$arguments:expr, ()) => { $adapter.$handler($seq, $request_seq) };"
)
# The unknown-command arm is pinned by exact normalised text rather than by
# an allow-list of sub-expressions. Allow-listing sub-expressions is
# position-blind: `_ => self.route_unknown(Self::unknown_command_message(
# command))` contains only permitted fragments yet still hands
# command-derived data to a helper that can route outside the table. The
# fallback is small and fully owned by this macro, so equality is the honest
# check — changing it is a deliberate edit that updates this constant too.
EXPECTED_FALLBACK = (
    "_ => DapMessage::Response { seq, request_seq, success: false, "
    "command: command.to_string(), body: None, "
    "message: Some(Self::unknown_command_message(command)), },"
)
RUST_ATTRIBUTE_RE = re.compile(r"#\s*!?\s*\[")


def _normalise_tokens(text: str) -> str:
    return " ".join(text.split())


STRING_CONTENT_MASK = "\x01"
# A Rust character literal, including escapes (`'\''`, `'\u{1F600}'`). A
# lifetime (`'static`, `'a`) has no closing quote and must not match, or the
# scanner would treat the rest of the file as literal content.
CHAR_LITERAL_RE = re.compile(r"'(?:\\.[^']*|[^'\\])'")


def _raw_string_end(text: str, index: int) -> tuple[int, int] | None:
    """If a raw string starts at `index`, return its (open_end, close_start).

    Handles `r"..."` and `r#*"..."#*`. Raw strings have no escapes, so `//`
    or `"` inside one must not be read as code.
    """
    if text[index] != "r":
        return None
    cursor = index + 1
    hashes = 0
    while cursor < len(text) and text[cursor] == "#":
        hashes += 1
        cursor += 1
    if cursor >= len(text) or text[cursor] != '"':
        return None
    open_end = cursor + 1
    terminator = '"' + "#" * hashes
    close = text.find(terminator, open_end)
    if close == -1:
        raise AuthorityError("unterminated raw string in the DAP dispatch source")
    return open_end, close


def scan_rust_source(text: str) -> tuple[str, str]:
    """Return `(stripped, masked)` views of Rust source, equal in length.

    `stripped` has comments blanked but string contents intact, so wire names
    stay readable. `masked` additionally replaces every string's *contents*
    with a sentinel, so a brace, an arrow, or a nested quote inside a string
    can never be read as code. Both views keep the original offsets of the
    text they retain, so a match found in one can be sliced out of the other.
    """
    stripped: list[str] = []
    masked: list[str] = []
    index = 0
    length = len(text)

    def emit(chunk: str, mask: str | None = None) -> None:
        stripped.append(chunk)
        masked.append(chunk if mask is None else mask)

    while index < length:
        char = text[index]

        raw = _raw_string_end(text, index) if char == "r" else None
        if raw is not None:
            open_end, close = raw
            hashes = open_end - index - 2
            emit(text[index:open_end])
            emit(
                text[open_end:close],
                "".join(
                    "\n" if c == "\n" else STRING_CONTENT_MASK for c in text[open_end:close]
                ),
            )
            terminator_end = close + 1 + hashes
            emit(text[close:terminator_end])
            index = terminator_end
            continue

        if char == "'":
            literal = CHAR_LITERAL_RE.match(text, index)
            if literal is not None:
                # `'"'` would otherwise open a string and swallow the rest of
                # the file; a lifetime falls through and is emitted as-is.
                body_start, body_end = index + 1, literal.end() - 1
                emit(text[index:body_start])
                emit(
                    text[body_start:body_end],
                    STRING_CONTENT_MASK * (body_end - body_start),
                )
                emit(text[body_end : literal.end()])
                index = literal.end()
                continue

        if char == '"':
            emit(char)
            index += 1
            content_start = index
            while index < length:
                current = text[index]
                if current == "\\" and index + 1 < length:
                    index += 2
                    continue
                if current == '"':
                    break
                index += 1
            content = text[content_start:index]
            emit(
                content,
                "".join("\n" if c == "\n" else STRING_CONTENT_MASK for c in content),
            )
            if index < length:
                emit(text[index])
                index += 1
            continue

        if text.startswith("//", index):
            end = text.find("\n", index)
            end = length if end == -1 else end
            emit(" " * (end - index))
            index = end
            continue

        if text.startswith("/*", index):
            depth = 1
            start = index
            index += 2
            while index < length and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            comment = text[start:index]
            # Preserve newlines so reported line numbers stay accurate.
            emit("".join("\n" if c == "\n" else " " for c in comment))
            continue

        emit(char)
        index += 1

    return "".join(stripped), "".join(masked)


def strip_rust_comments(text: str) -> str:
    """Comment-free view of Rust source with string contents intact."""
    return scan_rust_source(text)[0]


def _balanced(masked: str, open_index: int, opener: str = "{", closer: str = "}") -> int:
    """Return the index of the delimiter closing the one at `open_index`.

    `masked` must be the string-masked view, so a delimiter inside a comment
    or a string literal cannot unbalance the scan.
    """
    depth = 0
    for index in range(open_index, len(masked)):
        char = masked[index]
        if char == opener:
            depth += 1
        elif char == closer:
            depth -= 1
            if depth == 0:
                return index
    raise AuthorityError(f"unbalanced {opener!r} in the DAP dispatch source")


def _balanced_block(masked: str, open_index: int) -> int:
    return _balanced(masked, open_index)


def validate_generated_dispatch(text: str, masked: str) -> None:
    """Prove the request table is the only syntax that can define routing.

    A detector that recognises particular arm spellings can always be spelled
    around — a braced arm, a match guard, a helper return, or a pre-match
    `if command == "…"` are all routes with no common shape. So instead of
    enumerating bad shapes, this pins the structure: `dispatch_request` must
    be defined exactly once, only inside the `dap_request_table!` macro
    definition, and its body must contain nothing capable of routing beyond
    the row repetition and the unknown-command fallback.
    """
    # Multiplicity matters, not just membership: Rust lets a later
    # `macro_rules!` shadow an earlier one of the same name, so a second
    # `dap_dispatch_call` placed before the table invocation would silently
    # change every generated route while keeping an approved name.
    defined_macros = Counter(
        match.group("name") for match in ANY_MACRO_DEFINITION_RE.finditer(masked)
    )
    unexpected = sorted(set(defined_macros) - PERMITTED_MACRO_DEFINITIONS)
    if unexpected:
        raise AuthorityError(
            f"unexpected macro definitions in the dispatch source: {unexpected}; "
            "a generator macro here could expand into routing this gate does "
            "not analyse, so new ones must be reviewed in explicitly"
        )
    shadowed = sorted(name for name, count in defined_macros.items() if count > 1)
    if shadowed:
        raise AuthorityError(
            f"macro definitions redefined in the dispatch source: {shadowed}; "
            "a later definition shadows the earlier one and can change every "
            "generated route while keeping an approved name"
        )
    missing = sorted(PERMITTED_MACRO_DEFINITIONS - set(defined_macros))
    if missing:
        raise AuthorityError(
            f"expected macro definitions are absent from the dispatch source: "
            f"{missing}; routing must be generated by the reviewed macros"
        )

    # Attributes can change whether a generator, the generated function, or
    # one of its arms exists for a target/profile. Until rows carry explicit
    # cfg metadata, the authority is deliberately unconditional. Reject both
    # attributes within a macro definition and attributes attached directly
    # to a definition from the preceding item boundary.
    macro_matches = list(ANY_MACRO_DEFINITION_RE.finditer(masked))
    for definition in macro_matches:
        macro_body_open = masked.index("{", definition.end())
        macro_body_close = _balanced(masked, macro_body_open)
        previous_item_end = max(
            masked.rfind("}", 0, definition.start()),
            masked.rfind(";", 0, definition.start()),
        )
        governed = masked[previous_item_end + 1 : macro_body_close + 1]
        if RUST_ATTRIBUTE_RE.search(governed):
            name = definition.group("name")
            raise AuthorityError(
                f"attributes are not permitted on or inside {name}; request "
                "rows do not yet represent cfg/profile conditions"
            )
        macro_source = _normalise_tokens(text[definition.start() : macro_body_close + 1])
        name = definition.group("name")
        expected_macro = EXPECTED_SMALL_MACROS.get(name)
        if expected_macro is not None and macro_source != expected_macro:
            raise AuthorityError(f"{name} no longer matches its reviewed typed mapping")
        if name == "dap_dispatch_call":
            if (
                macro_source.count("=>") != 3
                or EXPECTED_ARGUMENTS_DISPATCH_ARM not in macro_source
                or EXPECTED_NO_ARGUMENTS_DISPATCH_ARM not in macro_source
                or macro_source.count("$adapter.$handler(") != 2
                or "compile_error!" not in macro_source
            ):
                raise AuthorityError(
                    "dap_dispatch_call no longer maps the two reviewed arities "
                    "directly to the row handler"
                )

    definitions = list(MACRO_DEFINITION_RE.finditer(masked))
    if len(definitions) != 1:
        raise AuthorityError(
            f"found {len(definitions)} dap_request_table macro definitions; "
            "exactly one may define the dispatch body"
        )
    macro_open = masked.index("{", definitions[0].end())
    macro_close = _balanced(masked, macro_open)

    # The inventory and every identity consumer must be projections of the
    # same row tokens, not merely equal values today. Otherwise a hand-written
    # constant can restore the split authority while value-based Rust tests
    # and the extracted inventory remain green.
    macro_source = _normalise_tokens(text[macro_open + 1 : macro_close])
    if (
        EXPECTED_ROUTE_ENUM_PROJECTION not in macro_source
        or EXPECTED_ROUTE_LOOKUP_PROJECTION not in macro_source
    ):
        raise AuthorityError(
            "DapRequestRoute is not the pinned command/availability projection "
            "of dap_request_table row tokens"
        )
    projections = (
        (
            "DAP_REQUEST_ROWS",
            REQUEST_ROWS_DEFINITION_RE,
            EXPECTED_REQUEST_ROWS_PROJECTION,
        ),
        (
            "SUPPORTED_COMMANDS",
            SUPPORTED_COMMANDS_DEFINITION_RE,
            EXPECTED_SUPPORTED_COMMANDS_PROJECTION,
        ),
    )
    for name, definition_re, expected in projections:
        occurrences = list(definition_re.finditer(masked))
        if len(occurrences) != 1:
            raise AuthorityError(
                f"found {len(occurrences)} {name} definitions; exactly one generated "
                "projection may own request identity"
            )
        if not macro_open < occurrences[0].start() < macro_close:
            raise AuthorityError(
                f"{name} is defined outside dap_request_table; it would be an "
                "independently editable request authority"
            )
        if expected not in macro_source:
            raise AuthorityError(
                f"{name} is not the pinned projection of dap_request_table row tokens"
            )

    functions = list(DISPATCH_FN_RE.finditer(masked))
    if len(functions) != 1:
        raise AuthorityError(
            f"found {len(functions)} dispatch_request definitions; the request "
            "table must be the only syntax that defines the dispatch body"
        )
    function = functions[0]
    if not macro_open < function.start() < macro_close:
        raise AuthorityError(
            "dispatch_request is defined outside the dap_request_table macro; "
            "routing must be generated from the table, not written by hand"
        )

    params_open = masked.index("(", function.end())
    params_close = _balanced(masked, params_open, "(", ")")
    body_open = masked.index("{", params_close)
    body_close = _balanced(masked, body_open)
    body = masked[body_open + 1 : body_close]

    # Exactly two arrows: the `$command` row repetition and the `_` fallback.
    arrows = body.count("=>")
    if arrows != 2:
        raise AuthorityError(
            f"generated dispatch body has {arrows} match arms; expected exactly "
            "the row repetition and the unknown-command fallback"
        )
    if len(re.findall(r"\bmatch\b", body)) != 1:
        raise AuthorityError(
            "generated dispatch body must contain exactly one match on the command"
        )
    for keyword in FORBIDDEN_DISPATCH_BODY_KEYWORDS:
        if re.search(rf"\b{keyword}\b", body):
            raise AuthorityError(
                f"generated dispatch body contains {keyword!r}; a route must not "
                "be reachable by branching around the request table"
            )
    if '"' in body:
        raise AuthorityError(
            "generated dispatch body contains a string literal; wire names may "
            "only come from request-table rows"
        )

    # The command value must not escape the match. Both permitted regions are
    # located by position and pinned by exact text, then excised; any
    # remaining `command` is a path that could delegate routing to code the
    # table does not generate.
    match_keyword = re.search(r"\bmatch\b", body)
    if match_keyword is None:
        raise AuthorityError("generated dispatch body has no match on the command")
    scrutinee_open = body.index("{", match_keyword.end())
    scrutinee = _normalise_tokens(body[match_keyword.end() : scrutinee_open])
    if scrutinee != EXPECTED_SCRUTINEE:
        raise AuthorityError(
            f"generated dispatch matches on {scrutinee!r}, not the command itself; "
            "a derived scrutinee could remap wire names away from the table rows"
        )
    match_close = _balanced(body, scrutinee_open)
    arms = body[scrutinee_open + 1 : match_close]

    fallback_start = arms.find("_ =>")
    if fallback_start == -1:
        raise AuthorityError("generated dispatch body has no unknown-command fallback")
    row_repetition = _normalise_tokens(arms[:fallback_start])
    if row_repetition != EXPECTED_ROW_REPETITION:
        raise AuthorityError(
            "generated request arm does not match the pinned table-to-handler "
            f"expansion; got {row_repetition[:120]!r}. Request rows must route "
            "through dap_dispatch_call without attributes or external expansion"
        )
    fallback = _normalise_tokens(arms[fallback_start:])
    if fallback != EXPECTED_FALLBACK:
        raise AuthorityError(
            "generated unknown-command fallback does not match the pinned "
            f"response; got {fallback[:120]!r}. The fallback must not delegate "
            "command-dependent work outside the table"
        )

    residue = (
        body[: match_keyword.start()]
        + arms[:fallback_start]
        + body[match_close + 1 :]
    )
    escaped = COMMAND_IDENT_RE.search(residue)
    if escaped is not None:
        raise AuthorityError(
            "generated dispatch body uses `command` outside the match scrutinee "
            "and the pinned fallback; command-dependent routing must not be "
            "delegated outside the table"
        )


def parse_request_table(source: str) -> list[dict[str, str]]:
    """Parse the executable request table into deterministic rows.

    Fails closed on a missing, duplicated, malformed, or partially parsed
    table, and on any request route outside it.
    """
    text, masked = scan_rust_source(source)

    invocations = list(REQUEST_TABLE_INVOCATION_RE.finditer(masked))
    if not invocations:
        raise AuthorityError(
            "cannot locate the executable dap_request_table! invocation; "
            "production request identity must come from routed rows"
        )
    if len(invocations) > 1:
        raise AuthorityError(
            f"found {len(invocations)} dap_request_table! invocations; "
            "exactly one executable request authority may exist"
        )

    previous_item_end = max(
        masked.rfind("}", 0, invocations[0].start()),
        masked.rfind(";", 0, invocations[0].start()),
    )
    if RUST_ATTRIBUTE_RE.search(masked[previous_item_end + 1 : invocations[0].start()]):
        raise AuthorityError(
            "attributes are not permitted on the dap_request_table invocation; "
            "request rows do not yet represent cfg/profile conditions"
        )

    validate_generated_dispatch(text, masked)

    open_index = invocations[0].end() - 1
    close_index = _balanced_block(masked, open_index)
    body = text[open_index + 1 : close_index]

    # No string-literal match arm may exist outside the table. Scanning the
    # masked view means an arm spelled inside a string literal is not code
    # and cannot raise here, while a real arm in any Rust shape does.
    outside_masked = (
        masked[:open_index]
        + "".join("\n" if c == "\n" else " " for c in masked[open_index : close_index + 1])
        + masked[close_index + 1 :]
    )
    stray = STRAY_MATCH_ARM_RE.search(outside_masked)
    if stray is not None:
        line = outside_masked.count("\n", 0, stray.start()) + 1
        name = text[stray.start("name") : stray.end("name")]
        raise AuthorityError(
            f"request route outside the table at {DISPATCH_PATH.as_posix()}:{line}: "
            f"{name!r}; every routed request must be a dap_request_table! row"
        )

    rows: list[dict[str, str]] = []
    consumed = 0
    for match in REQUEST_TABLE_ROW_RE.finditer(body):
        if body[consumed : match.start()].strip():
            residue = body[consumed : match.start()].strip()
            raise AuthorityError(
                f"unparsed content in the request table: {residue[:80]!r}"
            )
        rows.append(
            {
                "row_id": f"dap.request.{match.group('command')}",
                "command": match.group("command"),
                "class": match.group("class"),
                "availability": match.group("availability"),
                "variant": match.group("variant"),
                "handler": match.group("handler"),
            }
        )
        consumed = match.end()
    if body[consumed:].strip():
        raise AuthorityError(
            f"unparsed content in the request table: {body[consumed:].strip()[:80]!r}"
        )
    if not rows:
        raise AuthorityError("the executable request table is empty")

    commands = [row["command"] for row in rows]
    if len(set(commands)) != len(commands):
        raise AuthorityError("the executable request table contains duplicate wire names")
    handlers = [row["handler"] for row in rows]
    if len(set(handlers)) != len(handlers):
        raise AuthorityError("two request rows route to the same handler")
    variants = [row["variant"] for row in rows]
    if len(set(variants)) != len(variants):
        raise AuthorityError("the executable request table contains duplicate route variants")
    for row in rows:
        if row["class"] not in REQUEST_CLASSES:
            raise AuthorityError(
                f"request row {row['command']!r} has unknown class {row['class']!r}"
            )
    return rows


def parse_peer_dispatch_routes(source: str, owner: Path) -> set[str]:
    """Return the catalog variants explicitly handled by one peer frontend."""
    text, masked = scan_rust_source(source)
    functions = list(PEER_DISPATCH_FN_RE.finditer(masked))
    route_functions = [function for function in functions if "dispatch_unchecked" in function.group(0)]
    if route_functions:
        public_functions = [
            function
            for function in functions
            if "dispatch_unchecked" not in function.group(0)
        ]
        if len(public_functions) != 1:
            raise AuthorityError(
                f"found {len(public_functions)} public dispatch wrappers in {owner}; expected one"
            )
        wrapper = public_functions[0]
        wrapper_open = masked.index("{", masked.index(")", wrapper.end() - 1))
        wrapper_close = _balanced(masked, wrapper_open)
        wrapper_body = _normalise_tokens(masked[wrapper_open + 1 : wrapper_close])
        admission_tokens = (
            "self.dispatch_with_capability_floor(",
            "self.secondary_floor_response(",
        )
        if not any(token in wrapper_body for token in admission_tokens):
            raise AuthorityError(
                f"{owner} public dispatch does not delegate through the capability floor"
            )
        if "self.dispatch_with_capability_floor(" in wrapper_body:
            floor_functions = list(PEER_FLOOR_FN_RE.finditer(masked))
            if len(floor_functions) != 1:
                raise AuthorityError(
                    f"found {len(floor_functions)} capability-floor dispatch functions in {owner}; expected one"
                )
            floor = floor_functions[0]
            floor_open = masked.index("{", masked.index(")", floor.end() - 1))
            floor_close = _balanced(masked, floor_open)
            floor_body = masked[floor_open + 1 : floor_close]
            if "self.secondary_floor_response(" not in floor_body:
                raise AuthorityError(
                    f"{owner} capability-floor dispatch does not apply the capability floor"
                )
        functions = route_functions
    if len(functions) != 1:
        raise AuthorityError(
            f"found {len(functions)} production dispatch functions in {owner}; expected one"
        )
    params_open = masked.index("(", functions[0].end() - 1)
    params_close = _balanced(masked, params_open, "(", ")")
    body_open = masked.index("{", params_close)
    body_close = _balanced(masked, body_open)
    body = text[body_open + 1 : body_close]
    body_masked = masked[body_open + 1 : body_close]
    previous_item_end = max(
        masked.rfind("}", 0, functions[0].start()),
        masked.rfind(";", 0, functions[0].start()),
    )
    if RUST_ATTRIBUTE_RE.search(masked[previous_item_end + 1 : functions[0].start()]):
        raise AuthorityError(
            f"attributes are not permitted on the production dispatch function in {owner}"
        )
    lookup = PEER_ROUTE_MATCH_RE.search(body_masked)
    if lookup is None:
        raise AuthorityError(
            f"{owner} does not route through the canonical DapRequestRoute lookup"
        )
    match_open = body_masked.index("{", lookup.end())
    match_close = _balanced(body_masked, match_open)
    if _normalise_tokens(body[: lookup.start()]) != "let mut out = Vec::new();":
        raise AuthorityError(
            f"{owner} contains routing-capable syntax before the canonical request match"
        )
    if _normalise_tokens(body[match_close + 1 :]) != (
        "out.extend(self.poll_events()); out"
    ):
        raise AuthorityError(
            f"{owner} contains routing-capable syntax after the canonical request match"
        )
    arms_text = body[match_open + 1 : match_close]
    arms_masked = body_masked[match_open + 1 : match_close]
    stray = STRAY_MATCH_ARM_RE.search(body_masked)
    if stray is not None:
        raise AuthorityError(
            f"{owner} contains a string-literal request arm outside the canonical catalog"
        )
    # Read only top-level match patterns. A variant mentioned harmlessly in an
    # arm body must not compensate for deleting its executable route.
    patterns: list[str] = []
    boundary = 0
    braces = parentheses = brackets = 0
    index = 0
    while index < len(arms_masked):
        char = arms_masked[index]
        if char == "{":
            braces += 1
        elif char == "}":
            braces -= 1
            # A block-valued match arm may omit its trailing comma. Once its
            # outer block closes, the next top-level token starts a new arm.
            if braces == parentheses == brackets == 0:
                boundary = index + 1
        elif char == "(":
            parentheses += 1
        elif char == ")":
            parentheses -= 1
        elif char == "[":
            brackets += 1
        elif char == "]":
            brackets -= 1
        elif (
            char == "="
            and index + 1 < len(arms_masked)
            and arms_masked[index + 1] == ">"
            and braces == parentheses == brackets == 0
        ):
            patterns.append(_normalise_tokens(arms_text[boundary:index]))
            index += 1
        elif char == "," and braces == parentheses == brackets == 0:
            boundary = index + 1
        index += 1
    if not patterns or patterns[-1] != "None | Some(_)":
        raise AuthorityError(
            f"{owner} has no explicit final compatibility fallback arm; "
            f"top-level patterns={patterns}"
        )
    fallback_match = re.search(r"None\s*\|\s*Some\s*\(\s*_\s*\)\s*=>\s*\{", arms_masked)
    if fallback_match is None:
        raise AuthorityError(f"{owner} has no parseable compatibility fallback body")
    fallback_open = fallback_match.end() - 1
    fallback_close = _balanced(arms_masked, fallback_open)
    fallback_body = _normalise_tokens(arms_text[fallback_open + 1 : fallback_close])
    expected_fallback = EXPECTED_PEER_FALLBACKS.get(owner)
    if expected_fallback is None or fallback_body != expected_fallback:
        raise AuthorityError(
            f"{owner} compatibility fallback no longer matches its explicit "
            "dynamic_compatibility_ack_success_empty policy"
        )
    variants: list[str] = []
    for pattern in patterns[:-1]:
        exact = re.fullmatch(
            r"Some\s*\(\s*DapRequestRoute::(?P<variant>[A-Z][A-Za-z0-9]*)\s*\)",
            pattern,
        )
        if exact is None:
            raise AuthorityError(
                f"{owner} request pattern {pattern!r} is guarded, conditional, "
                "or broader than one typed catalog variant"
            )
        variants.append(exact.group("variant"))
    if len(set(variants)) != len(variants):
        raise AuthorityError(f"{owner} handles a catalog route variant more than once")
    return set(variants)


def production_dispatch_sources(root: Path) -> set[Path]:
    """Discover convention-named dispatch owners alongside the exact pinned owners.

    This is a guard for the current source graph, not proof that an arbitrarily
    named future ingress is discoverable. Adding a differently named frontend
    requires extending the explicit owner contract.
    """
    source_root = root / "crates/perl-dap/src"
    owners: set[Path] = set()
    if not source_root.is_dir():
        raise AuthorityError(f"missing perl-dap production source root: {source_root}")
    for path in source_root.rglob("*.rs"):
        _, masked = scan_rust_source(read_text(path, "DAP production source"))
        if PRODUCTION_DISPATCH_FN_RE.search(masked):
            owners.add(path.relative_to(root))
    return owners
SEND_EVENT_CALL_RE = re.compile(r"\bself\.send_event\s*\(")
SEND_EVENT_LITERAL_RE = re.compile(r'\s*"([A-Za-z][A-Za-z0-9]*)"')
DEFINITION_REF_PREFIX = "#/definitions/"

# Closed vocabularies for the versioned custom-family section (#10138).
FAMILY_CLASSIFICATIONS = {"custom_dap_extension"}
FAMILY_CAPABILITY_MODES = {"unadvertised-until-r04", "advertised-namespaced"}
FAMILY_NEGOTIATION_POLICIES = {
    "unknown_version_policy": {"reject-closed"},
    "unknown_variant_policy": {"reject-closed"},
    "unknown_field_policy": {"reject-closed", "tolerate-ignored"},
}
FAMILY_BOUND_KEYS = (
    "max_request_bytes",
    "max_identity_chars",
    "max_digest_chars",
    "max_reasons",
    "max_reason_chars",
    "max_detail_chars",
    "max_retained_operations",
)


def namespaced_family_name(name: str) -> bool:
    """A custom family name is a non-empty namespace, one `/`, and a
    non-empty local name — the collision-resistant shape required by
    ADR-0046 §6 and mirrored in crates/perl-dap/src/reload/surface.rs."""
    separator = name.find("/")
    if separator <= 0 or separator == len(name) - 1:
        return False
    namespace, local = name.split("/", 1)
    return bool(namespace.strip()) and bool(local.strip())


class AuthorityError(RuntimeError):
    """A fail-closed authority validation error."""


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AuthorityError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read {path}: {exc}") from exc


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_text(path: Path, context: str) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing {context}: {path}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read {context} {path}: {exc}") from exc


def object_value(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise AuthorityError(f"{context} must be a JSON object")
    return value


def string_value(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise AuthorityError(f"{context} must be a non-empty string")
    return value


def array_value(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise AuthorityError(f"{context} must be a JSON array")
    return value


def manifest_rows(manifest: Mapping[str, Any], key: str) -> list[Mapping[str, Any]]:
    return [
        object_value(item, f"manifest.{key}[{index}]")
        for index, item in enumerate(array_value(manifest.get(key), f"manifest.{key}"))
    ]


def _validate_pin_url(url: str, repository: str, commit: str, path: str) -> None:
    parsed = urllib.parse.urlparse(url)
    expected_path = f"/{repository}/{commit}/{path}"
    if parsed.scheme != "https":
        raise AuthorityError("upstream raw URL must use HTTPS")
    if parsed.netloc != "raw.githubusercontent.com":
        raise AuthorityError("upstream raw URL must use raw.githubusercontent.com")
    if parsed.path != expected_path:
        raise AuthorityError(
            f"upstream raw URL is not bound to the declared repository/commit/path: {parsed.path}"
        )
    if parsed.params or parsed.query or parsed.fragment:
        raise AuthorityError("upstream raw URL must not contain parameters, query, or fragment")


def validate_manifest(raw: Any, *, require_sha256: bool) -> Mapping[str, Any]:
    manifest = object_value(raw, "authority manifest")
    if manifest.get("schema_version") != MANIFEST_SCHEMA:
        raise AuthorityError(
            f"authority manifest schema must be {MANIFEST_SCHEMA!r}, "
            f"got {manifest.get('schema_version')!r}"
        )

    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    repository = string_value(upstream.get("repository"), "manifest.upstream.repository")
    commit = string_value(upstream.get("commit"), "manifest.upstream.commit")
    path = string_value(upstream.get("path"), "manifest.upstream.path")
    blob_sha1 = string_value(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")
    raw_url = string_value(upstream.get("raw_url"), "manifest.upstream.raw_url")

    if repository != "microsoft/debug-adapter-protocol":
        raise AuthorityError(f"unexpected upstream repository: {repository}")
    if path != "debugAdapterProtocol.json":
        raise AuthorityError(f"unexpected upstream schema path: {path}")
    if HEX40.fullmatch(commit) is None:
        raise AuthorityError("upstream commit must be a lowercase 40-character Git SHA")
    if HEX40.fullmatch(blob_sha1) is None:
        raise AuthorityError("upstream Git blob SHA must be lowercase 40-character hexadecimal")
    _validate_pin_url(raw_url, repository, commit, path)

    expected_sha256 = upstream.get("sha256")
    if expected_sha256 is None:
        if require_sha256:
            raise AuthorityError("upstream SHA-256 is not pinned")
    elif not isinstance(expected_sha256, str) or HEX64.fullmatch(expected_sha256) is None:
        raise AuthorityError("upstream SHA-256 must be null or lowercase 64-character hexadecimal")

    base = object_value(manifest.get("base_protocol"), "manifest.base_protocol")
    if base.get("name") != "Debug Adapter Protocol":
        raise AuthorityError("base protocol name must be 'Debug Adapter Protocol'")
    if base.get("transport") != "Content-Length framed JSON":
        raise AuthorityError("base protocol transport must be 'Content-Length framed JSON'")
    if base.get("json_rpc") is not False:
        raise AuthorityError("DAP must not be classified as JSON-RPC")
    declared_defs = array_value(
        base.get("required_definitions"), "base_protocol.required_definitions"
    )
    if declared_defs != list(REQUIRED_DEFINITIONS):
        raise AuthorityError(
            f"base protocol definitions must be ordered as {list(REQUIRED_DEFINITIONS)!r}"
        )

    seen_extensions: set[tuple[str, str]] = set()
    inline_values_found = False
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        name = string_value(extension.get("wire_name"), f"project_extensions[{index}].wire_name")
        kind = string_value(extension.get("kind"), f"project_extensions[{index}].kind")
        if kind not in {"request", "event"}:
            raise AuthorityError(
                f"project_extensions[{index}].kind must be 'request' or 'event', got {kind!r}"
            )
        identity = (kind, name)
        if identity in seen_extensions:
            raise AuthorityError(f"duplicate project extension identity: {kind}:{name}")
        seen_extensions.add(identity)
        if extension.get("classification") != "extension":
            raise AuthorityError(f"project extension {kind}:{name} is not classified as extension")
        string_value(extension.get("version"), f"project_extensions[{index}].version")
        string_value(extension.get("owner"), f"project_extensions[{index}].owner")
        inline_values_found |= identity == ("request", "inlineValues")
    if not inline_values_found:
        raise AuthorityError("custom inlineValues request is missing from the extension inventory")

    configurations = manifest_rows(manifest, "project_configuration")
    if not configurations:
        raise AuthorityError("adapter configuration inventory must not be empty")
    for index, configuration in enumerate(configurations):
        string_value(configuration.get("surface"), f"project_configuration[{index}].surface")
        if configuration.get("classification") != "adapter-configuration":
            raise AuthorityError(
                f"project_configuration[{index}] must be classified as adapter-configuration"
            )
        string_value(configuration.get("owner"), f"project_configuration[{index}].owner")

    families = manifest_rows(manifest, "project_families")
    if not families:
        raise AuthorityError("custom family inventory must not be empty")
    extension_identities: set[tuple[str, str]] = set()
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        extension_identities.add(
            (
                string_value(extension.get("kind"), f"project_extensions[{index}].kind"),
                string_value(
                    extension.get("wire_name"), f"project_extensions[{index}].wire_name"
                ),
            )
        )
    seen_families: set[str] = set()
    for index, family in enumerate(families):
        where = f"project_families[{index}]"
        name = string_value(family.get("family"), f"{where}.family")
        if not namespaced_family_name(name):
            raise AuthorityError(
                f"{where}.family must be a non-empty 'namespace/name' pair, got {name!r}"
            )
        if name in seen_families:
            raise AuthorityError(f"duplicate project family record: {name}")
        seen_families.add(name)
        request_name = string_value(family.get("request_name"), f"{where}.request_name")
        if not namespaced_family_name(request_name):
            raise AuthorityError(
                f"{where}.request_name must be namespaced like the family, got {request_name!r}"
            )
        for event_entry in array_value(family.get("event_names"), f"{where}.event_names"):
            event = string_value(event_entry, f"{where}.event_names entry")
            if not namespaced_family_name(event):
                raise AuthorityError(
                    f"{where} event {event!r} must be namespaced; a bare standard event "
                    "name can never belong to a custom family"
                )
        if ("request", request_name) in extension_identities:
            raise AuthorityError(
                f"{where} request {request_name!r} duplicates a project extension identity; "
                "a registered family request stays here until it is dispatched, at which "
                "point it must graduate to project_extensions and leave the family record"
            )
        if family.get("classification") not in FAMILY_CLASSIFICATIONS:
            raise AuthorityError(
                f"{where}.classification must be one of {sorted(FAMILY_CLASSIFICATIONS)}, "
                f"got {family.get('classification')!r}"
            )
        version = family.get("version")
        if not isinstance(version, int) or isinstance(version, bool) or version < 1:
            raise AuthorityError(f"{where}.version must be an integer >= 1")
        if family.get("capability_advertisement") not in FAMILY_CAPABILITY_MODES:
            raise AuthorityError(
                f"{where}.capability_advertisement must be one of "
                f"{sorted(FAMILY_CAPABILITY_MODES)}; a standard DAP capability spelling is "
                "never valid"
            )
        if not isinstance(family.get("dispatched"), bool):
            raise AuthorityError(f"{where}.dispatched must be a boolean")
        if not isinstance(family.get("backed"), bool):
            raise AuthorityError(f"{where}.backed must be a boolean")
        string_value(family.get("owner"), f"{where}.owner")
        string_value(family.get("contract"), f"{where}.contract")
        negotiation = object_value(family.get("negotiation"), f"{where}.negotiation")
        string_value(negotiation.get("mode"), f"{where}.negotiation.mode")
        string_value(negotiation.get("selection"), f"{where}.negotiation.selection")
        string_value(negotiation.get("session_binding"), f"{where}.negotiation.session_binding")
        string_value(negotiation.get("restart_effect"), f"{where}.negotiation.restart_effect")
        for policy, vocabulary in FAMILY_NEGOTIATION_POLICIES.items():
            if negotiation.get(policy) not in vocabulary:
                raise AuthorityError(
                    f"{where}.negotiation.{policy} must be one of {sorted(vocabulary)}"
                )
        identity = object_value(family.get("identity_policy"), f"{where}.identity_policy")
        for field in (
            "subject_shape",
            "raw_client_input",
            "correlation",
            "terminal_vocabulary",
            "possibly_applied_boundary",
        ):
            string_value(identity.get(field), f"{where}.identity_policy.{field}")
        bounds = object_value(family.get("bounds"), f"{where}.bounds")
        for key in FAMILY_BOUND_KEYS:
            value = bounds.get(key)
            if not isinstance(value, int) or isinstance(value, bool) or value < 1:
                raise AuthorityError(f"{where}.bounds.{key} must be a positive integer")
        string_value(family.get("redaction"), f"{where}.redaction")
        string_value(family.get("cancellation"), f"{where}.cancellation")
        if family.get("standard_dap_exclusion") is not True:
            raise AuthorityError(f"{where}.standard_dap_exclusion must be true")
        for field in (
            "schema",
            "typescript_projection",
            "rust_contract",
            "vectors",
            "generator_check",
        ):
            string_value(family.get(field), f"{where}.{field}")

    return manifest


def git_blob_sha1(data: bytes) -> str:
    header = f"blob {len(data)}\0".encode("ascii")
    return hashlib.sha1(header + data).hexdigest()  # noqa: S324 - Git object identity
