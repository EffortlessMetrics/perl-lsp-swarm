//! Mojo::Base `has` attribute-declaration extraction (#9682).
//!
//! Extracts the statically supported `Mojo::Base` attribute grammar from an
//! AST into [`MojoBaseAttributeDeclaration`] carriers for the
//! registry-activated minting in
//! `perl_semantic_facts::framework_adapters::mojo_base_facts`. This is pure
//! source observation: extraction knows the reviewed `has` grammar, it does
//! **not** decide activation — an object fact exists only after the registry
//! adapter minted it over an exact #9681 activation. A `has` call is never
//! activation evidence on its own.
//!
//! Supported forms (reviewed `Mojo::Base::attr` profile):
//!
//! ```perl
//! has 'name';
//! has name => 'default';
//! has name => sub { ... };
//! has [qw(host port)];
//! has [qw(host port)] => 'default';
//! ```
//!
//! **Known unmodeled spelling.** A bare `qw` list with no brackets and no
//! parentheses — `has qw(name default);` — is not extracted, and unlike every
//! other unsupported form here it yields no typed boundary either. The parser
//! does not bind the `qw` list to the `has` bareword: it emits two sibling
//! statements (a bare `Identifier` and a free-standing `ArrayLiteral`), so no
//! `has` declaration shape is present to observe. Recognizing it would mean
//! stitching two statements back together in this extractor to compensate for
//! a parse shape, which belongs upstream in the parser rather than here. The
//! spelling is legal Perl (`qw` flattens, so it means name plus default, not
//! two attributes) but is rare in reviewed `Mojo::Base` source, which spells a
//! multi-attribute declaration `has [qw(...)]`. Tracked as #14808; see
//! `a_bare_qw_list_without_brackets_is_a_known_unmodeled_spelling`.
//!
//! `Mojo::Base` binds the first operand to the attribute name (or an array
//! reference of names) and the optional second operand to the default, so
//! `has 'a', 'b';` declares the attribute `a` with default `'b'` — it is not
//! two attributes. Every generated accessor is read-write and a write returns
//! the invocant; those semantics live on the facts side.
//!
//! An attribute name must be one `Mojo::Base` would accept. `attr` croaks
//! `Attribute "NAME" invalid` for anything outside `/^[a-zA-Z_]\w*$/`, so a
//! rejected spelling (`'0'`, `'9lives'`, `'has-dash'`) becomes a typed
//! malformed selection rather than an accessor that cannot exist at runtime.
//! That pattern is applied verbatim, so it is exact rather than approximated:
//! it is asymmetric (the first character is the literal ASCII class
//! `[a-zA-Z_]`, later characters are `\w`), which makes `'aé'` valid and
//! `'éleading'` not.
//!
//! Package scoping mirrors the #9681 activation walk: an unqualified file
//! defaults to `main`, bare `package X;` switches the current package for
//! following statements, and a lexical block restores the enclosing package
//! state afterwards.
//!
//! A declaration counts only where it runs exactly once, before the run phase.
//! A `has` inside a subroutine body runs at call time; one under runtime
//! control flow in either spelling — a block form (`if (...) { has 'x'; }`) or
//! a postfix modifier (`has 'x' if $flag;`) — runs only when that construct
//! does; one inside `defer { ... }` runs at scope exit; one inside an `END`
//! block runs at process shutdown, after the program the accessor would serve
//! has finished. None of those declares a class attribute.
//!
//! Each extracted declaration also carries *when* it runs, as a
//! [`MojoBaseExecutionPhase`]. That is not the same question as whether it
//! runs: an ordinary statement executes after the whole file is compiled,
//! while a phaser executes during compilation. Two minting decisions turn on
//! it — whether source order against the activating import matters, and
//! whether a same-named explicit `sub` has a determinate winner — and both
//! have opposite answers for the two phases.
//!
//! Phase blocks are the exception to that containment, because Perl schedules
//! them independently of their lexical position. Verified against `perl`
//! itself: a `BEGIN` still runs when nested inside a false conditional, a loop,
//! a subroutine body, or an `END` block, and the same holds for `UNITCHECK`,
//! `CHECK` and `INIT`. All four complete before the run phase, so an accessor
//! installed in one exists for the whole program wherever it is written. An
//! early phaser therefore *replaces* the enclosing execution context rather
//! than inheriting it; `END` nested in `END` still defers, since the rule is
//! the phaser's own schedule rather than the mere presence of a phaser.
//!
//! The parser shapes three different trees for the reviewed forms, all
//! handled here:
//!
//! - `has NAME [, DEFAULT]` is an ordinary `has(...)` function call;
//! - `has [LIST]` parses as an index expression over the `has` bareword
//!   (`Binary { op: "[]" }`), because the bracket follows the identifier;
//! - `has [LIST] => DEFAULT` wraps that index expression as the key of the
//!   hash literal's first pair; trailing `%kv` options become its later pairs,
//!   so `has [qw(a b)] => undef, weak => 1;` is one declaration with an option
//!   list rather than an unrecognised shape.

use crate::analysis::dancer2_handler_targets::SubroutineTargetIndex;
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::mojo_base_facts::{
    MojoBaseAttributeDeclaration, MojoBaseAttributeDefault, MojoBaseAttributeName,
    MojoBaseExecutionPhase, MojoBaseExplicitMethodState,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor, SourceGeneration};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// The `Mojo::Base` attribute-declaration keyword.
const HAS_KEYWORD: &str = "has";

/// Extract every supported `Mojo::Base` `has` attribute declaration from
/// `ast`, in source order.
///
/// Each declaration carries its owning package, the `has` statement's
/// source-order index, and — for an array-reference name list — the position
/// of the name inside that list, so one statement's names never collide.
/// `generation` is the source generation `ast` was parsed from and is retained
/// on every declaration, so minting can refuse a carrier from an older parse
/// instead of restamping it as current.
///
/// Declarations are emitted for every package in the file. Restricting them
/// to an activated package is the minting side's contract, not extraction's:
/// this function deliberately reports `has` calls it observed without
/// claiming they generate anything.
///
/// Only declarations that run exactly once, before the run phase, are
/// extracted. A `has` call inside a subroutine body, a conditional, a loop, an
/// `eval`/`try` block, or an `END` block does not declare a class attribute —
/// it runs when (and if) that construct runs, or after the program has
/// finished — so extracting it would claim an accessor exists on paths where
/// the call never executes. A bare lexical block is not control flow and is
/// still extracted, and an early phaser (`BEGIN`, `UNITCHECK`, `CHECK`,
/// `INIT`) is extracted wherever it appears, because Perl runs it on schedule
/// regardless of what encloses it.
#[must_use]
pub fn extract_mojo_base_attribute_declarations(
    ast: &Node,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MojoBaseAttributeDeclaration> {
    let subroutines = SubroutineTargetIndex::build(ast, file_id);
    let mut nested_subroutines = HashSet::new();
    // Start from the same implicit `main` the declaration walk uses. Starting
    // from `None` would silently record nothing in an unqualified file, and a
    // missed collision mints the member with no boundary — asserting there is
    // no conflict when there is one.
    collect_nested_package_subroutines(ast, &mut Some("main".to_string()), &mut nested_subroutines);
    let mut state = WalkState {
        file_id,
        generation,
        subroutines: &subroutines,
        nested_subroutines: &nested_subroutines,
        next_declaration_index: 0,
        declarations: Vec::new(),
        deferred: false,
        phase: MojoBaseExecutionPhase::Run,
    };
    // An unqualified file's caller package is `main` in Perl, matching the
    // #9681 activation walk.
    let mut current_package: Option<String> = Some("main".to_string());
    state.walk(ast, &mut current_package);
    state.declarations
}

struct WalkState<'a> {
    file_id: FileId,
    generation: SourceGeneration,
    subroutines: &'a SubroutineTargetIndex,
    /// Package subroutines declared inside another subroutine's body, which
    /// [`SubroutineTargetIndex`] deliberately does not index.
    nested_subroutines: &'a HashSet<(String, String)>,
    next_declaration_index: u32,
    declarations: Vec<MojoBaseAttributeDeclaration>,
    /// Whether the current position runs later than, or less often than, the
    /// package's own load.
    ///
    /// Set by a sub body, runtime control flow, or an `END` block; cleared
    /// again by an early phaser, which Perl schedules independently of
    /// whatever encloses it. A declaration counts as a class attribute only
    /// while this is false.
    deferred: bool,
    /// When the current position executes, relative to compilation.
    phase: MojoBaseExecutionPhase,
}

/// Whether this operator only evaluates its right operand conditionally.
///
/// A bare `do { ... }` block is deliberately not treated as control flow: on
/// its own it runs exactly once, like any other statement. It becomes
/// conditional only through the operator that encloses it, which is what these
/// arms model.
fn is_short_circuit_operator(op: &str) -> bool {
    matches!(op, "&&" | "||" | "//" | "and" | "or")
}

/// Whether this node owns runtime control flow, so statements inside it do not
/// run unconditionally at package load.
///
/// A bare lexical block is deliberately absent: `{ has 'x'; }` at package level
/// executes exactly once, like any other package statement.
///
/// Phase blocks are deliberately absent too. They are not conditional — they
/// are *scheduled*, on a timetable independent of their lexical position — so
/// [`WalkState::walk`] handles them separately rather than treating them as
/// containment.
fn owns_runtime_control_flow(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
            | NodeKind::Given { .. }
            | NodeKind::When { .. }
            | NodeKind::Eval { .. }
            | NodeKind::Try { .. }
            // `defer { ... }` runs at scope exit, so a `has` inside it has not
            // declared anything for the code that precedes that exit.
            | NodeKind::Defer { .. }
            // Postfix modifiers are control flow too: `has 'name' if $flag;`
            // and `has 'name' for @list;` run conditionally exactly like the
            // block forms above.
            | NodeKind::StatementModifier { .. }
    )
}

/// Whether `Mojo::Base` would install an accessor for this attribute name.
///
/// `Mojo::Base::attr` refuses any name outside `/^[a-zA-Z_]\w*$/` with
/// `Attribute "NAME" invalid` (verified against the upstream `Mojo/Base.pm`
/// source), so a name it rejects declares no accessor at all — publishing one
/// would be a member that cannot exist at runtime. This rejects a falsy `'0'`,
/// a digit-leading `'9lives'`, and a hyphenated `'has-dash'` alike rather than
/// special-casing any single spelling.
///
/// The two halves of that pattern are deliberately asymmetric. The first
/// character is the literal class `[a-zA-Z_]`, so it stays ASCII — `'éleading'`
/// is rejected. Every later character is `\w`, which Perl evaluates with
/// Unicode semantics, so `'aé'`, `'naïve_name'` and `'café2'` are all names
/// `Mojo::Base` accepts and this must not refuse.
///
/// The check is the upstream pattern itself rather than a hand-rolled
/// character predicate. `regex`'s `\w` is
/// `[\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}]` — the same class Perl
/// uses — so applying `^[a-zA-Z_]\w*$` verbatim is exact rather than an
/// approximation, and stays correct without a second definition of `\w` to
/// keep in sync.
///
/// Two earlier hand-rolled attempts were each wrong in a different direction:
/// `is_ascii_alphanumeric` refused `'aé'`, and `char::is_alphanumeric` accepted
/// `'a²'` (Unicode `\p{No}`, which `\w` excludes). The pattern accepts `'aé'`,
/// `'a٣'` (non-ASCII `\p{Nd}`), a combining mark, connector punctuation and a
/// join control, and refuses `'0'`, `'9lives'`, `'has-dash'`, `'éleading'`,
/// `'a²'` and `'a¼'` — all pinned below.
///
/// If the pattern somehow fails to compile the name is refused. That is an
/// instrument failure, and refusing errs toward omitting a real accessor
/// rather than publishing one that cannot exist.
fn is_valid_attribute_name(name: &str) -> bool {
    static ATTRIBUTE_NAME_RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    ATTRIBUTE_NAME_RE
        .get_or_init(|| Regex::new(r"^[a-zA-Z_]\w*$"))
        .as_ref()
        .is_ok_and(|pattern| pattern.is_match(name))
}

impl WalkState<'_> {
    fn walk(&mut self, node: &Node, current_package: &mut Option<String>) {
        match &node.kind {
            NodeKind::Program { statements } => {
                // File scope: a bare `package X;` persists for the rest of the
                // file.
                for statement in statements {
                    self.walk(statement, current_package);
                }
                return;
            }
            NodeKind::Block { statements } => {
                // A lexical block scopes statement-form `package X;`
                // declarations: walk with a block-local copy so the enclosing
                // package state is restored afterwards.
                let mut block_package = current_package.clone();
                for statement in statements {
                    self.walk(statement, &mut block_package);
                }
                return;
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                if let NodeKind::Block { statements } = &block.kind {
                    let mut package_scope = Some(name.clone());
                    for statement in statements {
                        self.walk(statement, &mut package_scope);
                    }
                }
                return;
            }
            NodeKind::Package { name, block: None, .. } => {
                *current_package = Some(name.clone());
            }
            // A phase block runs on its own schedule, which Perl fixes at
            // compile time regardless of what lexically encloses it: verified
            // against `perl`, a `BEGIN` still runs inside a false conditional,
            // a loop, a sub body, and an `END` block. So a phaser *replaces*
            // the surrounding execution context rather than inheriting it.
            //
            // `BEGIN`, `UNITCHECK`, `CHECK` and `INIT` all complete before the
            // run phase, so an accessor installed in one exists for the whole
            // program and is an ordinary class member — the context clears.
            // `END` runs at process shutdown, after the program the accessor
            // would serve has finished, so it exists for no part of the run
            // and the context defers.
            NodeKind::PhaseBlock { phase, .. } => {
                // `BEGIN` runs at its own position during compilation, so an
                // import written below it has not happened yet. `UNITCHECK`,
                // `CHECK` and `INIT` are scheduled to run once compilation has
                // finished, so they see the whole file's imports and
                // subroutines — for this producer's purposes they behave like
                // an ordinary statement. `END` runs at shutdown and declares
                // nothing. Any other spelling is not a phaser this profile
                // knows, so it defers rather than being assumed harmless.
                let (deferred, phase) = match phase.as_str() {
                    "BEGIN" => (false, MojoBaseExecutionPhase::CompileImmediate),
                    "UNITCHECK" | "CHECK" | "INIT" => (false, MojoBaseExecutionPhase::PostCompile),
                    _ => (true, self.phase),
                };
                let enclosing_phase = self.phase;
                self.phase = phase;
                self.walk_deferring(node, current_package, deferred);
                self.phase = enclosing_phase;
                return;
            }
            // `Mojo::Base` attributes are declared at package level. A `has`
            // call inside a sub body executes at call time and declares no
            // class attribute; a `has` under a conditional, loop, or eval/try
            // runs only when that construct runs. Both are descended anyway
            // rather than skipped, because a phaser nested inside either one
            // still runs on schedule and does declare an attribute.
            NodeKind::Subroutine { .. } => {
                self.walk_deferring(node, current_package, true);
                return;
            }
            _ if owns_runtime_control_flow(node) => {
                self.walk_deferring(node, current_package, true);
                return;
            }
            // A block passed to a function is treated as a callback body:
            // `map`, `grep`, `sort` and the List::Util family all run it once
            // per element, so a `has` inside one runs zero or many times
            // rather than exactly once.
            //
            // **Documented limitation.** Whether braces after a function name
            // are a deferred callback or an immediately-evaluated hash
            // constructor depends on the callee's prototype, verified under
            // `perl`: `f { ... }` on a plain sub evaluates the braces at once
            // and passes a HASH, while `g(&@) { ... }` defers them. The parser
            // emits the identical `FunctionCall` → `Block` shape for both, and
            // the prototype is not in this AST — the sub may be imported from
            // another file — so the two cannot be told apart here.
            //
            // Deferring both is the deliberate choice, because the two errors
            // are not equally bad. Deferring an immediate hash omits an
            // accessor that exists; descending a real callback publishes one
            // that does not, and this producer's documented bias is to omit
            // rather than invent. Keying on a name list of known callbacks
            // would invert that, over-reporting for every user-defined
            // `(&@)` function.
            // A ternary's branches run only on their side of the condition;
            // the condition itself runs unconditionally.
            NodeKind::Ternary { condition, then_expr, else_expr } => {
                self.walk(condition, current_package);
                let enclosing = self.deferred;
                self.deferred = true;
                self.walk(then_expr, current_package);
                self.walk(else_expr, current_package);
                self.deferred = enclosing;
                return;
            }
            // A short-circuit operator evaluates its left operand
            // unconditionally and its right operand only if the left permits.
            // `do { ... }` is how a `has` reaches either side as a statement.
            NodeKind::Binary { op, left, right } if is_short_circuit_operator(op) => {
                self.walk(left, current_package);
                let enclosing = self.deferred;
                self.deferred = true;
                self.walk(right, current_package);
                self.deferred = enclosing;
                return;
            }
            NodeKind::FunctionCall { .. } => {
                for child in node.children() {
                    if matches!(child.kind, NodeKind::Block { .. }) {
                        let enclosing = self.deferred;
                        self.deferred = true;
                        self.walk(child, current_package);
                        self.deferred = enclosing;
                    } else {
                        self.walk(child, current_package);
                    }
                }
                return;
            }
            // A collected `has` statement is fully consumed here: descending
            // into its operands would re-observe them as ordinary source. In a
            // deferred position nothing is collected, so the statement is left
            // to ordinary descent — its operands are literals either way.
            NodeKind::ExpressionStatement { expression }
                if !self.deferred
                    && self.collect_declaration(expression, current_package.as_deref()) =>
            {
                return;
            }
            _ => {}
        }
        for child in node.children() {
            self.walk(child, current_package);
        }
    }

    /// Whether an explicit source method of this name exists in `package`.
    ///
    /// [`SubroutineTargetIndex`] answers this for ordinary declarations, and
    /// owns the declarator and typeglob rules. It deliberately does not index
    /// subroutines declared inside another subroutine's body, because its own
    /// contract is about statically resolvable handler targets — but Perl
    /// installs `sub outer { sub inner { ... } }` into the package at compile
    /// time all the same, so `inner` really can shadow an accessor. Missing it
    /// would mint the member with no collision boundary, asserting there is no
    /// conflict when there is one.
    ///
    /// The supplementary set covers exactly that gap rather than replacing the
    /// index, so the declarator and slot-mutation rules stay in one place.
    fn collides_with_explicit_method(&self, name: &str, package: Option<&str>) -> bool {
        if self.subroutines.resolve(name, package).is_some() {
            return true;
        }
        package.is_some_and(|package| {
            self.nested_subroutines.contains(&(package.to_string(), name.to_string()))
        })
    }

    /// Walk `node`'s children with the deferred-execution flag set to
    /// `deferred`, restoring the caller's value afterwards.
    fn walk_deferring(
        &mut self,
        node: &Node,
        current_package: &mut Option<String>,
        deferred: bool,
    ) {
        let enclosing = self.deferred;
        self.deferred = deferred;
        for child in node.children() {
            self.walk(child, current_package);
        }
        self.deferred = enclosing;
    }

    /// Collect one `has` declaration from a statement expression.
    ///
    /// Returns whether the expression was a `has` declaration (and therefore
    /// must not be descended into as ordinary source).
    fn collect_declaration(&mut self, expression: &Node, package: Option<&str>) -> bool {
        let Some(parsed) = parse_has_expression(expression) else {
            return false;
        };
        let declaration_index = self.next_declaration_index;
        self.next_declaration_index += 1;
        let declaration_anchor =
            anchor(expression.location.start, expression.location.end, self.file_id);
        for (name_index, (name, name_node)) in parsed.names.into_iter().enumerate() {
            let name_anchor = anchor(name_node.0, name_node.1, self.file_id);
            let explicit_method = match name.literal() {
                Some(literal) if self.collides_with_explicit_method(literal, package) => {
                    MojoBaseExplicitMethodState::Collides
                }
                _ => MojoBaseExplicitMethodState::None,
            };
            self.declarations.push(MojoBaseAttributeDeclaration {
                declaration_index,
                name_index: span_u32(name_index),
                file_id: self.file_id,
                package: package.map(ToString::to_string),
                declaration_anchor,
                name_anchor,
                name,
                default: parsed.default.clone(),
                explicit_method,
                execution_phase: self.phase,
                unmodeled_options: parsed.unmodeled_options.clone(),
                source_generation: self.generation.clone(),
            });
        }
        true
    }
}

/// One parsed `has` statement: its name selections with their source ranges,
/// plus the shared default evidence.
struct ParsedHas {
    names: Vec<(MojoBaseAttributeName, (usize, usize))>,
    default: MojoBaseAttributeDefault,
    unmodeled_options: Vec<String>,
}

/// Recognize the three parser shapes of a reviewed `has` statement.
fn parse_has_expression(expression: &Node) -> Option<ParsedHas> {
    match &expression.kind {
        // `has NAME;` / `has NAME => DEFAULT;` / `has(NAME, DEFAULT)`
        NodeKind::FunctionCall { name, args } if name == HAS_KEYWORD => {
            let (name_operand, rest) = args.split_first()?;
            let (default, unmodeled_options) = default_and_options(rest);
            Some(ParsedHas { names: names_from_operand(name_operand), default, unmodeled_options })
        }
        // `has [LIST];` — the bracket follows the bareword, so the parser
        // shapes it as an index expression rather than a call.
        NodeKind::Binary { .. } => {
            let list = has_index_list(expression)?;
            Some(ParsedHas {
                names: names_from_operand(list),
                default: MojoBaseAttributeDefault::Absent,
                unmodeled_options: Vec::new(),
            })
        }
        // `has [LIST] => DEFAULT;` — the index expression becomes the key of a
        // one-pair hash literal.
        NodeKind::HashLiteral { pairs } => {
            // Trailing `%kv` options land in the same hash literal, so
            // `has [qw(a b)] => undef, weak => 1;` arrives as several pairs.
            // `Mojo::Base::attr` binds `($self, $attrs, $value, %kv)` and
            // generates every listed accessor regardless, so the extra pairs
            // are option keys rather than a reason to reject the declaration.
            let ((key, value), options) = pairs.split_first()?;
            let list = has_index_list(key)?;
            Some(ParsedHas {
                names: names_from_operand(list),
                default: classify_default(value),
                unmodeled_options: options.iter().map(|(key, _)| option_key_name(key)).collect(),
            })
        }
        _ => None,
    }
}

/// The list operand of a `has [LIST]` index expression, when this node is one.
fn has_index_list(node: &Node) -> Option<&Node> {
    let NodeKind::Binary { op, left, right } = &node.kind else {
        return None;
    };
    if op != "[]" {
        return None;
    }
    let NodeKind::Identifier { name } = &left.kind else {
        return None;
    };
    if name != HAS_KEYWORD {
        return None;
    }
    Some(right)
}

/// Name selections contributed by one `has` name operand.
///
/// An array-reference operand contributes one selection per element; every
/// other operand contributes exactly one.
fn names_from_operand(operand: &Node) -> Vec<(MojoBaseAttributeName, (usize, usize))> {
    match &operand.kind {
        NodeKind::ArrayLiteral { elements } if !elements.is_empty() => elements
            .iter()
            .map(|element| (classify_name(element), (element.location.start, element.location.end)))
            .collect(),
        // An empty list declares nothing; keep it an explicit malformed
        // selection rather than silently dropping the statement.
        NodeKind::ArrayLiteral { .. } => vec![(
            MojoBaseAttributeName::Malformed {
                reason: "empty attribute-name list declares no attribute".to_string(),
            },
            (operand.location.start, operand.location.end),
        )],
        _ => vec![(classify_name(operand), (operand.location.start, operand.location.end))],
    }
}

/// Classify one attribute-name operand.
///
/// Only a static string spelling names a method. An interpolated spelling
/// whose value is computed, a variable, and every other expression stay
/// explicit dynamic boundaries: a guessed accessor name would be a fabricated
/// member.
fn classify_name(node: &Node) -> MojoBaseAttributeName {
    match &node.kind {
        NodeKind::String { value, interpolated } => {
            if *interpolated && interpolated_value_is_dynamic(value) {
                return MojoBaseAttributeName::Dynamic {
                    reason: "interpolated attribute name is computed at runtime".to_string(),
                };
            }
            match unquote(value) {
                // Escapes are not evaluated here, so the runtime name may
                // differ from the source bytes.
                Some(name) if name.contains('\\') => MojoBaseAttributeName::Malformed {
                    reason: "attribute name contains unevaluated escape sequences".to_string(),
                },
                Some(name) if !is_valid_attribute_name(&name) => MojoBaseAttributeName::Malformed {
                    reason: format!("Mojo::Base rejects the attribute name `{name}`"),
                },
                Some(name) => MojoBaseAttributeName::Literal(name),
                None => {
                    MojoBaseAttributeName::Malformed { reason: "empty attribute name".to_string() }
                }
            }
        }
        NodeKind::Identifier { name } if is_valid_attribute_name(name) => {
            MojoBaseAttributeName::Literal(name.clone())
        }
        NodeKind::Identifier { name } => MojoBaseAttributeName::Malformed {
            reason: format!("Mojo::Base rejects the attribute name `{name}`"),
        },
        NodeKind::Variable { .. } => MojoBaseAttributeName::Dynamic {
            reason: "attribute name comes from a variable".to_string(),
        },
        _ => MojoBaseAttributeName::Dynamic {
            reason: "attribute name is a computed expression".to_string(),
        },
    }
}

/// Default and option evidence contributed by the operands following the
/// name.
///
/// `Mojo::Base::attr` binds `($self, $attrs, $value, %kv)`: the operand after
/// the name is the default, and anything after that is a key/value option
/// list. The corpus spells `has app => undef, weak => 1;`, so trailing pairs
/// are ordinary supported syntax — not extra operands — even though this
/// profile does not model what each option means. Option keys are returned so
/// the fact side can limit the reader without disturbing the accessor
/// identity. An odd trailing operand cannot be a `%kv` list at all and stays
/// an explicit unsupported boundary.
fn default_and_options(rest: &[Node]) -> (MojoBaseAttributeDefault, Vec<String>) {
    let Some((default, options)) = rest.split_first() else {
        return (MojoBaseAttributeDefault::Absent, Vec::new());
    };
    if options.is_empty() {
        return (classify_default(default), Vec::new());
    }
    // An odd tail is not a rejected declaration. `%kv` is an ordinary hash
    // assignment, so Perl binds the dangling key to `undef` (with a warning)
    // and `attr` still generates the accessor with the default it was given —
    // verified by calling `attr`'s binding directly. Discarding the default as
    // unsupported would make a determinate reader falsely unknown, so the
    // parity only affects the option list, and `step_by(2)` already keeps the
    // dangling key.
    let keys = options.iter().step_by(2).map(option_key_name).collect();
    (classify_default(default), keys)
}

/// Recorded spelling of one `%kv` option key.
///
/// Shared by both declaration shapes that can carry options — the flat operand
/// list (`has app => undef, weak => 1;`) and the array-reference form
/// (`has [qw(a b)] => undef, weak => 1;`) — so one spelling cannot be recorded
/// two different ways depending on which branch parsed it.
fn option_key_name(key: &Node) -> String {
    match &key.kind {
        NodeKind::String { value, .. } => unquote(value).unwrap_or_else(|| "<empty>".to_string()),
        NodeKind::Identifier { name } => name.clone(),
        _ => "<computed>".to_string(),
    }
}

/// Classify one default operand.
///
/// `Mojo::Base` admits a constant value or a code reference that builds the
/// value lazily; it croaks on any other reference. An explicit `undef`
/// default is indistinguishable from no default at runtime (the accessor
/// stores undef either way), so it is reported as absent rather than as a
/// value-shaped constant.
fn classify_default(node: &Node) -> MojoBaseAttributeDefault {
    match &node.kind {
        NodeKind::Undef => MojoBaseAttributeDefault::Absent,
        NodeKind::Number { .. } => MojoBaseAttributeDefault::Constant,
        NodeKind::String { value, interpolated } => {
            if *interpolated && interpolated_value_is_dynamic(value) {
                MojoBaseAttributeDefault::Dynamic {
                    reason: "interpolated default is computed at runtime".to_string(),
                }
            } else {
                MojoBaseAttributeDefault::Constant
            }
        }
        NodeKind::Subroutine { name: None, .. } => MojoBaseAttributeDefault::LazyBuilder,
        NodeKind::ArrayLiteral { .. } | NodeKind::HashLiteral { .. } => {
            MojoBaseAttributeDefault::Unsupported {
                reason: "Mojo::Base rejects a non-code reference default at runtime".to_string(),
            }
        }
        NodeKind::Variable { .. } => MojoBaseAttributeDefault::Dynamic {
            reason: "default comes from a variable".to_string(),
        },
        _ => MojoBaseAttributeDefault::Dynamic {
            reason: "default is a computed expression".to_string(),
        },
    }
}

fn span_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn anchor(start: usize, end: usize, file_id: FileId) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(start as u64)), file_id, span_u32(start), span_u32(end))
}

/// Strip one matched pair of surrounding quotes, if present.
///
/// The parser retains the raw token spelling for quoted strings and drops the
/// quotes for fat-comma autoquoted barewords and `qw` words, so both shapes
/// reach here.
fn unquote(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| trimmed.strip_prefix('"').and_then(|value| value.strip_suffix('"')))
        .unwrap_or(trimmed);
    if stripped.is_empty() { None } else { Some(stripped.to_string()) }
}

/// Whether an interpolated string operand is statically a computed value.
///
/// Perl interpolation only occurs through `$`/`@` sigils followed by an
/// identifier or index, so a trailing sigil stays static.
fn interpolated_value_is_dynamic(value: &str) -> bool {
    crate::analysis::dancer2_routes::interpolated_value_is_dynamic(value)
}

/// Split a declared subroutine name into its owning package and bare slot.
///
/// `sub name` belongs to whatever package is current; `sub App::name` names
/// `App` explicitly and ignores the current package; a leading `::` is `main`.
/// Returns `None` when no package can be established.
fn split_subroutine_name(name: &str, current_package: Option<&str>) -> Option<(String, String)> {
    match name.rfind("::") {
        Some(index) => {
            let package = &name[..index];
            let bare = &name[index + 2..];
            if bare.is_empty() {
                return None;
            }
            let package = if package.is_empty() { "main" } else { package };
            Some((package.to_string(), bare.to_string()))
        }
        None => Some((current_package?.to_string(), name.to_string())),
    }
}

/// Collect every named package subroutine, including those nested inside
/// another subroutine's body.
///
/// Perl installs a named `sub` into the package symbol table at compile time
/// wherever it is written, so `sub outer { sub inner { ... } }` defines
/// `inner` at package level — verified under `perl`. Only `my`/`state` subs
/// are lexical and excluded, matching the declarator rule
/// [`SubroutineTargetIndex`] already applies.
fn collect_nested_package_subroutines(
    node: &Node,
    current_package: &mut Option<String>,
    found: &mut HashSet<(String, String)>,
) {
    match &node.kind {
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut scope = Some(name.clone());
            collect_nested_package_subroutines(block, &mut scope, found);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = Some(name.clone());
        }
        NodeKind::Block { statements } => {
            let mut block_package = current_package.clone();
            for statement in statements {
                collect_nested_package_subroutines(statement, &mut block_package, found);
            }
            return;
        }
        NodeKind::Subroutine { name: Some(name), declarator, .. } => {
            let package_scoped = matches!(declarator.as_deref(), None | Some("our"));
            if package_scoped {
                // `sub App::name` names its own package, whatever package is
                // current. Recording the full spelling as the bare slot would
                // make the collision lookup for `name` in `App` miss it, and a
                // missed collision mints the member with no boundary at all.
                if let Some((package, bare)) =
                    split_subroutine_name(name, current_package.as_deref())
                {
                    found.insert((package, bare));
                }
            }
        }
        _ => {}
    }
    for child in node.children() {
        collect_nested_package_subroutines(child, current_package, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::must;

    fn declarations(code: &str) -> Vec<MojoBaseAttributeDeclaration> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_mojo_base_attribute_declarations(&ast, FileId(1), SourceGeneration::known("gen-1"))
    }

    fn names(code: &str) -> Vec<String> {
        declarations(code)
            .iter()
            .filter_map(|declaration| declaration.name.literal().map(ToString::to_string))
            .collect()
    }

    #[test]
    fn quoted_and_autoquoted_names_are_literal() {
        assert_eq!(names("package App;\nhas 'name';\nhas other => 1;\n"), ["name", "other"]);
    }

    #[test]
    fn an_array_reference_declares_one_attribute_per_name() {
        let found = declarations("package App;\nhas [qw(host port)];\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name.literal(), Some("host"));
        assert_eq!(found[1].name.literal(), Some("port"));
        assert_eq!(
            found[0].declaration_index, found[1].declaration_index,
            "one statement is one declaration index"
        );
        assert_eq!((found[0].name_index, found[1].name_index), (0, 1));
    }

    #[test]
    fn an_array_reference_with_a_default_keeps_both() {
        let found = declarations("package App;\nhas [qw(a b)] => 'shared';\n");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.default == MojoBaseAttributeDefault::Constant));
        assert_eq!(names("package App;\nhas [qw(a b)] => 'shared';\n"), ["a", "b"]);
    }

    #[test]
    fn a_second_operand_is_the_default_not_a_second_attribute() {
        // `Mojo::Base::attr` binds one name and one default, so `has 'a', 'b'`
        // is the attribute `a` defaulting to `'b'`.
        let found = declarations("package App;\nhas 'a', 'b';\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.literal(), Some("a"));
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Constant);
    }

    #[test]
    fn a_sub_default_is_a_lazy_builder() {
        let found = declarations("package App;\nhas config => sub { {} };\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::LazyBuilder);
    }

    #[test]
    fn an_undef_default_reports_as_absent() {
        let found = declarations("package App;\nhas 'name' => undef;\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Absent);
    }

    #[test]
    fn a_non_code_reference_default_is_unsupported() {
        let found = declarations("package App;\nhas 'list' => [];\n");
        assert!(matches!(found[0].default, MojoBaseAttributeDefault::Unsupported { .. }));
    }

    #[test]
    fn computed_names_and_defaults_stay_typed_boundaries() {
        let dynamic_name = declarations("package App;\nhas $field => 1;\n");
        assert!(matches!(dynamic_name[0].name, MojoBaseAttributeName::Dynamic { .. }));
        let dynamic_default = declarations("package App;\nhas 'name' => $value;\n");
        assert!(matches!(dynamic_default[0].default, MojoBaseAttributeDefault::Dynamic { .. }));
    }

    #[test]
    fn an_interpolated_name_is_dynamic_but_a_plain_one_is_literal() {
        let interpolated = declarations("package App;\nhas \"pre$suffix\";\n");
        assert!(matches!(interpolated[0].name, MojoBaseAttributeName::Dynamic { .. }));
        let plain = declarations("package App;\nhas \"plain\";\n");
        assert_eq!(plain[0].name.literal(), Some("plain"));
    }

    #[test]
    fn packages_scope_each_declaration() {
        let found = declarations("package App;\nhas 'a';\npackage Other;\nhas 'b';\n");
        assert_eq!(found[0].package.as_deref(), Some("App"));
        assert_eq!(found[1].package.as_deref(), Some("Other"));
    }

    #[test]
    fn a_lexical_block_restores_the_enclosing_package() {
        let found = declarations("package Outer;\n{ package Inner; has 'inner'; }\nhas 'outer';\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("Inner"));
        assert_eq!(found[1].package.as_deref(), Some("Outer"));
    }

    #[test]
    fn an_unqualified_file_defaults_to_main() {
        assert_eq!(declarations("has 'name';\n")[0].package.as_deref(), Some("main"));
    }

    #[test]
    fn a_has_call_inside_a_sub_body_is_not_a_class_attribute() {
        assert!(declarations("package App;\nsub build { has 'runtime'; }\n").is_empty());
    }

    #[test]
    fn trailing_option_pairs_are_bound_as_options_not_extra_operands() {
        // Verbatim from the bundled corpus
        // (test_corpus/real_projects/mojolicious_skeleton/lib/Mojolicious/
        // Controller.pm): `Mojo::Base::attr` binds ($self, $attrs, $value,
        // %kv), so `weak => 1` is a supported option, not a malformed extra
        // operand.
        let found = declarations("package App;\nhas app => undef, weak => 1;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.literal(), Some("app"));
        assert_eq!(
            found[0].default,
            MojoBaseAttributeDefault::Absent,
            "the default operand is `undef`, which stores undef either way"
        );
        assert_eq!(found[0].unmodeled_options, ["weak"], "the option key is recorded, not dropped");
    }

    #[test]
    fn an_odd_trailing_operand_still_leaves_the_default_intact() {
        // `%kv` is an ordinary hash assignment, so an odd tail binds the last
        // key to `undef` (with a warning) rather than rejecting the call —
        // confirmed by exercising `attr`'s binding directly under `perl`. The
        // parity limits the option list, not the default, so reporting the
        // default as unsupported would make a determinate reader falsely
        // unknown.
        let found = declarations("package App;\nhas name => 'v', weak;\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Constant);
        assert_eq!(found[0].unmodeled_options, ["weak"]);
    }

    #[test]
    fn a_plain_declaration_records_no_options() {
        let found = declarations("package App;\nhas name => 'v';\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Constant);
        assert!(found[0].unmodeled_options.is_empty());
    }

    #[test]
    fn runtime_control_flow_does_not_declare_a_class_attribute() {
        // A `has` under a conditional or loop runs only when that construct
        // runs, so claiming an unconditional accessor would be an overclaim.
        assert!(declarations("package App;\nif ($c) { has 'cond'; }\n").is_empty());
        assert!(declarations("package App;\nfor (1..2) { has 'loop'; }\n").is_empty());
        assert!(declarations("package App;\nwhile ($c) { has 'spin'; }\n").is_empty());
        assert!(declarations("package App;\neval { has 'risky'; };\n").is_empty());
        // Control: a bare lexical block is not control flow and still counts.
        assert_eq!(names("package App;\n{ has 'bare'; }\n"), ["bare"]);
    }

    #[test]
    fn a_postfix_modifier_is_control_flow_too() {
        // `has 'name' if $flag;` runs conditionally exactly like its block
        // form, so it declares no unconditional class attribute.
        assert!(declarations("package App;\nhas 'cond' if $enabled;\n").is_empty());
        assert!(declarations("package App;\nhas 'each' for 1..2;\n").is_empty());
        assert!(declarations("package App;\nhas 'unless' unless $skip;\n").is_empty());
        // Control: the same declaration without a modifier still counts.
        assert_eq!(names("package App;\nhas 'cond';\n"), ["cond"]);
    }

    #[test]
    fn names_mojo_base_would_reject_declare_no_attribute() {
        // `Mojo::Base::attr` croaks `Attribute "NAME" invalid` for anything
        // outside /^[a-zA-Z_]\w*$/, so publishing a member for one would be an
        // accessor that cannot exist at runtime.
        for rejected in ["'0'", "'9lives'", "'has-dash'", "'with space'", "'a.b'"] {
            let code = format!("package App;\nhas {rejected};\n");
            let found = declarations(&code);
            assert_eq!(found.len(), 1, "the declaration is still observed: {rejected}");
            assert!(
                matches!(found[0].name, MojoBaseAttributeName::Malformed { .. }),
                "{rejected} must not become a literal accessor name"
            );
        }
        // Controls: names Mojo::Base accepts stay literal.
        for accepted in ["'ok_name'", "'_leading'", "'a9'", "'CamelCase'"] {
            let code = format!("package App;\nhas {accepted};\n");
            assert!(
                declarations(&code)[0].name.literal().is_some(),
                "{accepted} is a valid Mojo::Base attribute name"
            );
        }
    }

    #[test]
    fn unicode_continuation_characters_are_valid_attribute_names() {
        // Perl evaluates `\w` with Unicode semantics, so these are names
        // `Mojo::Base` installs accessors for — refusing them would omit a real
        // accessor.
        for accepted in [
            "'aé'",
            "'naïve_name'",
            "'café2'",
            // Non-ASCII decimal digit (U+0663, `\p{Nd}`) — in Perl's `\w`.
            "'a\u{663}'",
            // Combining mark (U+0301, `\p{M}`) — in Perl's `\w`.
            "'a\u{301}'",
            // Connector punctuation (U+203F, `\p{Pc}`) — in Perl's `\w`.
            "'a\u{203F}'",
        ] {
            let code = format!("package App;\nhas {accepted};\n");
            assert!(
                declarations(&code)[0].name.literal().is_some(),
                "{accepted} is a runtime-valid Mojo::Base attribute name"
            );
        }
        // Unicode numeric symbols outside `\p{Nd}` — superscripts, fractions —
        // are NOT in Perl's `\w`, so accepting them would publish an accessor
        // `Mojo::Base` refuses. `char::is_alphanumeric` would accept them.
        for rejected in ["'a\u{b2}'", "'a\u{bc}'", "'a\u{b3}'"] {
            let code = format!("package App;\nhas {rejected};\n");
            assert!(
                matches!(declarations(&code)[0].name, MojoBaseAttributeName::Malformed { .. }),
                "{rejected} carries a Unicode numeric symbol outside Perl's `\\w`"
            );
        }
        // The first character is the literal class `[a-zA-Z_]`, not `\w`, so a
        // Unicode leading character stays malformed.
        let leading = declarations("package App;\nhas 'éleading';\n");
        assert!(
            matches!(leading[0].name, MojoBaseAttributeName::Malformed { .. }),
            "a non-ASCII first character is outside `[a-zA-Z_]`"
        );
    }

    #[test]
    fn declarations_carry_the_extraction_generation() {
        let found = declarations("package App;\nhas 'name';\n");
        assert_eq!(found[0].source_generation, SourceGeneration::known("gen-1"));
    }

    #[test]
    fn a_bare_qw_list_without_brackets_is_a_known_unmodeled_spelling() {
        // Pins the documented limitation rather than asserting it is correct:
        // the parser emits `has` and the `qw` list as two sibling statements,
        // so no declaration shape reaches this extractor. The bracketed
        // spelling immediately below is the control proving the extractor
        // itself handles `qw` words fine — the gap is the unbracketed parse,
        // not `qw` support.
        assert!(declarations("package App;\nhas qw(name);\n").is_empty());
        assert!(declarations("package App;\nhas qw(name default);\n").is_empty());
        assert_eq!(names("package App;\nhas [qw(name other)];\n"), ["name", "other"]);
    }

    #[test]
    fn a_bare_has_bareword_declares_nothing() {
        assert!(declarations("package App;\nhas;\n").is_empty());
    }

    #[test]
    fn a_same_named_explicit_sub_is_recorded_as_a_collision() {
        let found = declarations("package App;\nhas 'name';\nsub name { 'explicit' }\n");
        assert_eq!(found[0].explicit_method, MojoBaseExplicitMethodState::Collides);
        let clean = declarations("package App;\nhas 'name';\nsub other { 1 }\n");
        assert_eq!(clean[0].explicit_method, MojoBaseExplicitMethodState::None);
    }

    #[test]
    fn the_declaration_anchor_covers_the_real_has_statement() {
        let code = "package App;\nhas 'name';\n";
        let found = declarations(code);
        let anchor = found[0].declaration_anchor;
        assert!(
            code[(anchor.start_byte as usize)..(anchor.end_byte as usize)].contains("has 'name'"),
            "the generator anchor must cover the declaring statement"
        );
        let name_anchor = found[0].name_anchor;
        assert_eq!(
            &code[(name_anchor.start_byte as usize)..(name_anchor.end_byte as usize)],
            "'name'",
            "the name anchor must cover the name operand"
        );
    }

    #[test]
    fn declaration_indices_follow_source_order() {
        let found = declarations("package App;\nhas 'a';\nhas 'b';\nhas 'c';\n");
        let indices: Vec<u32> = found.iter().map(|d| d.declaration_index).collect();
        assert_eq!(indices, [0, 1, 2]);
    }
}
