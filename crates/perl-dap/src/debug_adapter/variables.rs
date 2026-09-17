//! Variable inspection: variable display, scope variables, set variable.

use super::{
    CachedVariable, DEBUGGER_QUERY_WAIT_MS, DapMessage, DebugAdapter, DebugState, HashMap,
    SetVariableArguments, SetVariableResponseBody, Value, VariableCacheKind, VariablesArguments,
    is_valid_set_variable_name, json, lock_or_recover, parse_dap_arguments, slice_variables,
};
use crate::parse_origin::{DebuggerOutputOrigin, ParseIdentity};
use crate::value_format::ValueFormatPolicy;
#[cfg(test)]
use perl_tdd_support::must_some;

impl DebugAdapter {
    /// Handle variables request
    pub fn handle_variables(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: VariablesArguments = match parse_dap_arguments(arguments) {
            Ok(a) => a,
            Err(message) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "variables".to_string(),
                    body: None,
                    message: Some(message),
                };
            }
        };

        // One typed presentation policy for the whole response: projected from
        // retained typed facts at the response boundary, never by reparsing
        // cached display strings, and never leaking into row identity (#9588).
        let format_policy = ValueFormatPolicy::from_options(args.format.as_ref());

        if args.start.is_some_and(|start| start < 0) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "variables".to_string(),
                body: None,
                message: Some("Invalid start: must be >= 0".to_string()),
            };
        }

        if args.count.is_some_and(|count| count < 0) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "variables".to_string(),
                body: None,
                message: Some("Invalid count: must be >= 0".to_string()),
            };
        }

        // Clamp i64 → i32 safely: values outside [1, i32::MAX] cannot encode a valid scope ref
        // (scope encoding is frame_id * 10 + {1,2,3}, all positive). Negative, zero, or
        // out-of-i32-range refs return protocol-safe empty per DAP spec — success=true, variables=[].
        // We check the raw i64 first to catch huge positive overflow before saturation would
        // hide it (i64::MAX saturates to i32::MAX, which is a non-zero i32 and would pass
        // a simple `== 0` check — wrong). Refs in (0, i32::MAX] are passed to i64_to_i32_saturating.
        let variables_ref_raw = args.variables_reference;
        if variables_ref_raw <= 0 || variables_ref_raw > i32::MAX as i64 {
            // Out-of-range: return protocol-safe empty response immediately.
            // totalVariables is omitted (not 0) — DAP spec says omit optional fields
            // when the value is not meaningful (invalid ref has no defined total).
            return DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "variables".to_string(),
                body: Some(json!({ "variables": [] })),
                message: None,
            };
        }
        let variables_ref = Self::i64_to_i32_saturating(variables_ref_raw);

        let start = args.start.unwrap_or(0) as usize;
        let count = args.count.map(|v| v as usize).unwrap_or(256).clamp(1, 1024);

        // Stale-ref guard: if a session exists but the debugger is not stopped, the cache
        // has been cleared (variable_cache.clear() is called on every continue/step). Any
        // variablesReference the client holds from the previous stop is stale. Querying
        // the debugger while it is running would hang or produce garbage. Return the
        // protocol-safe honest empty immediately.
        {
            let session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            if let Some(ref session) = *session_guard
                && session.state != DebugState::Stopped
            {
                // Not stopped: variable refs are stale. Omit totalVariables —
                // we have no meaningful count when not paused.
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "variables".to_string(),
                    body: Some(json!({ "variables": [] })),
                    message: None,
                };
            }

            // Once the active session has been cleared, every non-cache
            // reference is stale.  Do not consult recent debugger output: it
            // belongs to an older session and can make an unknown reference
            // appear to have live children.
            if session_guard.is_none() {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "variables".to_string(),
                    body: Some(json!({ "variables": [] })),
                    message: None,
                };
            }
        }

        // Scope references are bound to the exact current stopped frame.  Check
        // this before cache lookup, parsing, or debugger I/O so old, wrong-frame,
        // Package, and Globals references cannot be revived by representative
        // data or a stale cache entry.
        {
            use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
            if let Some(VariableReference::Scope { frame_id, kind }) =
                VariableReference::decode(variables_ref)
            {
                let exact_current = self.exact_current_stopped_frame_id(i64::from(frame_id));
                if exact_current != Some(frame_id)
                    || matches!(kind, ScopeKind::Package | ScopeKind::Globals)
                {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: true,
                        command: "variables".to_string(),
                        body: Some(json!({ "variables": [] })),
                        message: None,
                    };
                }
            }
        }

        // Arguments are captured from the verbose stack trace, not queried from the
        // debugger.  Keep this path before the generic scope routing so a client cannot
        // accidentally turn an Arguments reference into a package/global query.
        {
            use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
            if let Some(VariableReference::Scope { frame_id, kind: ScopeKind::Arguments }) =
                VariableReference::decode(variables_ref)
            {
                let arguments = lock_or_recover(&self.session, "debug_adapter.session")
                    .as_ref()
                    .and_then(|session| session.stack_frame_arguments.get(&frame_id))
                    .cloned()
                    .unwrap_or_default();
                let total = arguments.len();
                let variables = arguments
                    .into_iter()
                    .enumerate()
                    .skip(start)
                    .take(count)
                    .map(|(index, value)| crate::types::Variable {
                        name: format!("arg{index}"),
                        value,
                        type_: None,
                        variables_reference: 0,
                        named_variables: None,
                        indexed_variables: None,
                        evaluate_name: None,
                    })
                    .collect::<Vec<_>>();
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "variables".to_string(),
                    body: Some(json!({
                        "variables": variables,
                        "totalVariables": total as i64,
                    })),
                    message: None,
                };
            }
        }

        // AC8.4: Render scalars/arrays/hashes with lazy child expansion.
        let mut parsed_from_output = Vec::new();
        let mut parsed_child_cache = HashMap::new();
        let mut parsed_full_roots = Vec::new();
        let mut used_session_cache = false;
        let mut cached_total: Option<usize> = None;
        let mut cached_page = None;
        let mut framed_scope_query = None;
        let mut captured_frame_id = None;
        let mut captured_locals_identity = None;

        {
            let mut session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            if let Some(session) = session_guard.as_mut() {
                if let Some(vars) = session.variable_cache.get_page(variables_ref, start, count) {
                    used_session_cache = true;
                    cached_total = session.variable_cache.root_count(variables_ref);
                    cached_page = Some(vars);
                } else {
                    use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
                    if matches!(
                        VariableReference::decode(variables_ref),
                        Some(VariableReference::EvalResult { .. })
                            | Some(VariableReference::Child { .. })
                    ) {
                        return DapMessage::Response {
                            seq,
                            request_seq,
                            success: true,
                            command: "variables".to_string(),
                            body: Some(json!({ "variables": [] })),
                            message: None,
                        };
                    }
                    let scope_kind = match VariableReference::decode(variables_ref) {
                        Some(VariableReference::Scope { frame_id, kind }) => {
                            captured_frame_id = Some(frame_id);
                            Some(kind)
                        }
                        _ => None,
                    };
                    if scope_kind.is_none() {
                        return DapMessage::Response {
                            seq,
                            request_seq,
                            success: true,
                            command: "variables".to_string(),
                            body: Some(json!({ "variables": [] })),
                            message: None,
                        };
                    }
                    if scope_kind == Some(ScopeKind::Locals) {
                        let frame_is_current = captured_frame_id.is_some_and(|frame_id| {
                            session.state == DebugState::Stopped
                                && session
                                    .stack_frames
                                    .first()
                                    .is_some_and(|frame| frame.id == frame_id)
                        });
                        if !frame_is_current {
                            return DapMessage::Response {
                                seq,
                                request_seq,
                                success: true,
                                command: "variables".to_string(),
                                body: Some(json!({ "variables": [] })),
                                message: None,
                            };
                        }
                        let stopped_generation = session.stopped_generation;
                        let broker_generation = self.operation_broker.current_session_generation();
                        if let Some(stdin) = session.process.stdin.as_mut() {
                            let commands = vec![Self::build_locals_b_eval_cmd()];
                            match self.send_framed_debugger_query_bound(
                                stdin,
                                &commands,
                                DEBUGGER_QUERY_WAIT_MS * 8,
                                Some(stopped_generation),
                                Some(broker_generation),
                            ) {
                                Ok((operation, begin, end)) => {
                                    framed_scope_query = Some((operation, begin, end));
                                    captured_locals_identity =
                                        Some((stopped_generation, broker_generation));
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "Failed to send framed locals command; locals unavailable");
                                }
                            }
                        }
                    }
                }
            }
        }

        let had_framed_scope_query = framed_scope_query.is_some();
        if let Some((operation, begin, end)) = framed_scope_query {
            let framed_scope_lines =
                self.capture_framed_debugger_output_for_operation(&operation, &begin, &end);
            if let Some(lines) = framed_scope_lines.as_ref() {
                let (framed_vars, framed_child_cache) = Self::parse_scope_variables_from_lines(
                    lines,
                    variables_ref,
                    0,
                    1024,
                    DebuggerOutputOrigin::DebuggerControlPayload,
                    ParseIdentity::new().with_operation_id_from_i64(request_seq),
                );
                if !framed_vars.is_empty() {
                    parsed_full_roots = framed_vars;
                    parsed_child_cache = framed_child_cache;
                }
            }
        } else if let Some(vars) = cached_page {
            parsed_from_output = vars;
        } else {
            // Scope admission is current-frame-only. Without a framed response,
            // return unavailable rather than parsing unrelated recent output.
            parsed_from_output = Vec::new();
        }

        if had_framed_scope_query {
            parsed_from_output = slice_variables(&parsed_full_roots, start, count);
        }

        // Capture total count before pagination (pre-slice length) for the DAP totalVariables
        // field. Priority: fresh parse (parsed_full_roots) > cache hit (cached_total) > unknown.
        // totalVariables is only emitted when we have a reliable full count; it is omitted
        // (not null) otherwise, per the DAP spec's optional-field semantics.
        let total_variables: Option<i64> = if !parsed_full_roots.is_empty() {
            // Fresh parse: total is the full root list length before pagination.
            Some(parsed_full_roots.len() as i64)
        } else {
            // Cache hit (the cache stores the original full list, so root_count is reliable)
            // maps to Some; the fallback/unknown path stays None and omits the field.
            cached_total.map(|n| n as i64)
        };

        // Project the response rows under this request's format policy. Cached
        // and freshly parsed rows both carry typed facts; identity fields come
        // from the cached row and the display value is recomputed from typed
        // facts only. The fallback scope is unavailable (honest empty), so it
        // projects as untyped rows. (#9588)
        let cached_rows: Vec<CachedVariable> = if parsed_from_output.is_empty() {
            Self::fallback_scope_variables(variables_ref, start, count)
                .into_iter()
                .map(CachedVariable::untyped)
                .collect()
        } else {
            parsed_from_output
        };
        let variables: Vec<crate::types::Variable> = cached_rows
            .iter()
            .map(|cached| format_policy.project_variable(&cached.row, cached.typed.as_ref()))
            .collect();

        // Fresh roots and children share one stop/session acceptance boundary.
        if !used_session_cache && !parsed_full_roots.is_empty() {
            let mut session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            let accepted = match (session_guard.as_mut(), captured_locals_identity) {
                (Some(session), Some((stopped, broker_generation)))
                    if session.state == DebugState::Stopped
                        && session.stopped_generation == stopped
                        && session.stack_frames.first().map(|frame| frame.id)
                            == captured_frame_id =>
                {
                    self.operation_broker
                        .accept_if_current(broker_generation, || {
                            session.variable_cache.upsert(
                                variables_ref,
                                VariableCacheKind::Root,
                                parsed_full_roots,
                            );
                            for (reference, children) in parsed_child_cache {
                                session.variable_cache.upsert(
                                    reference,
                                    VariableCacheKind::Child,
                                    children,
                                );
                            }
                            let _ = session.variable_cache.get_page(variables_ref, start, count);
                        })
                        .is_ok()
                }
                _ => false,
            };
            if !accepted {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "variables".to_string(),
                    body: Some(json!({ "variables": [] })),
                    message: None,
                };
            }
        }

        // Build response body. totalVariables is optional per DAP spec — omit the field
        // entirely when the value is not known, rather than emitting `null`.  The json!()
        // macro serializes Option::None as JSON null (not field-absent), so we build the
        // body with the field only when we have a reliable count.
        let mut body = json!({ "variables": variables });
        if let Some(total) = total_variables {
            body["totalVariables"] = json!(total);
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "variables".to_string(),
            body: Some(body),
            message: None,
        }
    }

    /// Build the Perl eval command used to introspect lexical (`my`) variables
    /// via the B module.
    ///
    /// Arrays (`@foo`) and hashes (`%foo`) are emitted as opaque `ARRAY(0x0)` /
    /// `HASH(0x0)` markers — the same format the `V` command uses for package
    /// variables. They are deliberately *not* expanded here.
    ///
    /// Recovering the live aggregate (e.g. through `B::SV::object_2svref` and a
    /// serializer) would enumerate the value during a nominally read-only
    /// `variables` request. For tied, magical, blessed, or overloaded aggregates
    /// that runs debuggee code (`FETCHSIZE`, `FETCH`, `FIRSTKEY`, `NEXTKEY`,
    /// overloaded stringification), and root-width/serializer-depth caps do not
    /// bound cumulative nodes, bytes, or query duration. Safe bounded lexical
    /// collection snapshots are owned by #7358; until that contract exists this
    /// path stays honest about not having observed the contents.
    ///
    /// The same no-user-code contract applies to scalars (#9590's public stdio
    /// canaries caught the old read chain violating it): the value is read
    /// through raw slot flags only — references render their address via
    /// `overload::StrVal` (which bypasses overloaded stringification), strings
    /// via the PV slot when `SVf_POK` is set, integers via IV, floats via NV,
    /// and anything carrying no readable slot (a tied proxy, a magical SV) as
    /// `undef`. The previous `$s->SV->PV`/`->IV` chain invoked tied `FETCH` and
    /// overloaded `""`/numification on magical values, executing debuggee code
    /// during a read-only inspection; it also lost NV-only scalars entirely
    /// (they dumped as `undef`). Reading the referent of a reference is still
    /// not done here.
    pub(super) fn build_locals_b_eval_cmd() -> String {
        format!(
            concat!(
                "p eval {{ require B; require overload; ",
                "my $cv=$DB::sub?B::svref_2object(\\&{{$DB::sub}}):B::main_cv(); ",
                "my $pl=$cv->PADLIST; ",
                "my @nm=$pl->NAMES->ARRAY; ",
                "my @va=$pl->ARRAY; ",
                "my $fi={frame}; ",
                "my @pds=(@va>1+$fi)?$va[-(1+$fi)]->ARRAY:(@va>1)?$va[-1]->ARRAY:(); ",
                "my $o=''; ",
                "for my $i (0..$#nm) {{ ",
                "  my $n=$nm[$i]; ",
                "  next if ref($n) eq 'B::SPECIAL'; ",
                "  my $pv=eval{{$n->PVX}}//''; ",
                "  next unless $pv=~/^[\\$\\@%]/; ",
                "  my $s=$i<@pds?$pds[$i]:undef; ",
                "  next unless defined $s; ",
                "  my $rt=ref($s); ",
                "  my $v; ",
                "  if ($rt eq 'B::AV') {{ $v='ARRAY(0x0)' }} ",
                "  elsif ($rt eq 'B::HV') {{ $v='HASH(0x0)' }} ",
                "  else {{ ",
                "    my $f=eval{{$s->FLAGS}}//0; ",
                "    if ($f & B::SVf_ROK()) {{ ",
                // Read the RV's referent, not the pad slot: `$a = \$x; $b = $a`
                // share one referent but sit in distinct slots, so stringifying
                // the slot would give aliases different addresses. The referent
                // address also matches the pre-#9590 IV display exactly.
                "      my $qr=eval{{$s->RV->object_2svref}}; ",
                "      my $sv=defined $qr?overload::StrVal($qr):''; ",
                "      if ($sv=~/0x([0-9a-fA-F]+)/) {{ no warnings 'portable'; $v=hex($1) }} ",
                "      else {{ $v='REF' }} ",
                "    }} ",
                "    elsif ($f & B::SVf_POK()) {{ $v=eval{{$s->PV}}//'undef' }} ",
                "    elsif ($f & B::SVf_IOK()) {{ $v=eval{{$s->IV}}//'undef' }} ",
                "    elsif ($f & B::SVf_NOK()) {{ $v=eval{{$s->NV}}//'undef' }} ",
                "    else {{ $v='undef' }} ",
                "  }} ",
                // A flagged (wide) PV must not reach perl5db's print: the resulting
                // "Wide character in print" warning echoes this whole eval back
                // into the control stream on every locals request and can blow
                // the bounded capture window. Encode in place to the identical
                // UTF-8 bytes: unlike utf8::downgrade this always succeeds (it
                // never fails on code points above 255) and never emits raw
                // Latin-1 bytes that would break the control reader's UTF-8
                // line decoding.
                "  if (defined $v && !ref($v) && utf8::is_utf8($v)) {{ utf8::encode($v) }} ",
                "  $o.=\"$pv = $v\\n\" ",
                "}} $o }}",
            ),
            frame = 0,
        )
    }

    /// Handle setVariable request
    pub(super) fn handle_set_variable(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        // #8354 fail-closed gate. The refusal is a function of the advertised
        // capability alone, so advertisement and enforcement cannot disagree.
        // It fires before `parse_dap_arguments`, name/value screening,
        // reference lookup, and any broker/debugger traffic: while the
        // capability is closed, no parser, resolver, coordinator, or transport
        // call may run and no reference or retained state may change — valid
        // and hostile input get the identical early unsupported response.
        if crate::backend::capabilities::refuse_set_variable(
            crate::backend::capabilities::advertises_set_variable(),
        ) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(
                    crate::backend::capabilities::SET_VARIABLE_UNSUPPORTED_MESSAGE.to_string(),
                ),
            };
        }
        let args: SetVariableArguments = match parse_dap_arguments(arguments) {
            Ok(a) => a,
            Err(message) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "setVariable".to_string(),
                    body: None,
                    message: Some(message),
                };
            }
        };
        // `format` affects the response rendering only; the assigned data below
        // is always the admitted client `value` (#9588; #8364/#9070 own
        // admission and read-back).
        let format_policy = ValueFormatPolicy::from_options(args.format.as_ref());

        let variables_ref = args.variables_reference;
        if variables_ref <= 0 {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some("Missing variablesReference".to_string()),
            };
        }

        let name = args.name.trim().to_string();
        let value = args.value.trim().to_string();
        let name = name.as_str();
        let value = value.as_str();

        if name.is_empty() {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some("Missing variable name".to_string()),
            };
        }

        if value.is_empty() {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some("Missing variable value".to_string()),
            };
        }

        if name.contains('\n')
            || name.contains('\r')
            || value.contains('\n')
            || value.contains('\r')
        {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some("Variable name/value cannot contain newlines".to_string()),
            };
        }

        if !is_valid_set_variable_name(name) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(format!(
                    "Invalid variable name `{name}` for setVariable (expected Perl sigil-prefixed variable)"
                )),
            };
        }

        if contains_unquoted_statement_separator(value) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(
                    "setVariable: unsafe value rejected: statement separators are not allowed"
                        .to_string(),
                ),
            };
        }

        if !is_safe_set_variable_value(value) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(
                    "setVariable: unsafe value rejected: only literal or simple variable-reference values are allowed"
                        .to_string(),
                ),
            };
        }

        let output_frame_markers = if let Some(ref mut session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
        {
            if let Some(stdin) = session.process.stdin.as_mut() {
                // Frame assignment + read-back so output parsing is deterministic.
                let commands = vec![format!("p {name} = {value}"), format!("p {name}")];
                match self.send_framed_debugger_query(stdin, &commands, DEBUGGER_QUERY_WAIT_MS * 8)
                {
                    Ok((operation, begin, end)) => Some((operation, begin, end)),
                    Err(error) => {
                        return DapMessage::Response {
                            seq,
                            request_seq,
                            success: false,
                            command: "setVariable".to_string(),
                            body: None,
                            message: Some(format!("Failed to send setVariable command: {error}")),
                        };
                    }
                }
            } else {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "setVariable".to_string(),
                    body: None,
                    message: Some("No debugger session active".to_string()),
                };
            }
        } else if let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(format!(
                    "setVariable is unavailable for processId attach (PID {pid}) without an active debugger transport"
                )),
            };
        } else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some("No debugger session".to_string()),
            };
        };

        // Correlate the read-back against the variable being set, not an empty subject.
        // The commands sent are `p {name} = {value}` then `p {name}`; an empty subject can
        // never equal a parsed assignment name, and the `continue` guarding the literal
        // branch would then discard such a line outright (#7275).
        let parsed = output_frame_markers
            .as_ref()
            .and_then(|(operation, begin, end)| {
                self.capture_framed_debugger_output_for_operation(operation, begin, end)
            })
            .and_then(|lines| {
                Self::parse_evaluate_result_from_lines(
                    &lines,
                    name,
                    true,
                    DebuggerOutputOrigin::DebuggerControlPayload,
                    ParseIdentity::new().with_operation_id_from_i64(request_seq),
                )
            });

        let Some((default_value, rendered_type, typed)) = parsed else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setVariable".to_string(),
                body: None,
                message: Some(format!(
                    "setVariable read-back for `{name}` produced no parseable output"
                )),
            };
        };

        // The cached placeholder keeps the policy-neutral rendering plus typed
        // facts, so a later `variables` expansion projects under its own
        // request's format (#9588). The response value is projected from typed
        // facts under this request's policy.
        let variables_reference =
            self.allocate_evaluate_result_ref(name, &default_value, &rendered_type, typed.clone());
        let set_var_body = SetVariableResponseBody {
            value: format_policy.project_display(&default_value, typed.as_ref()),
            type_: Some(rendered_type),
            variables_reference,
        };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "setVariable".to_string(),
            body: serde_json::to_value(&set_var_body).ok(),
            message: None,
        }
    }
}

pub(super) fn contains_unquoted_statement_separator(value: &str) -> bool {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            ';' if !in_single_quote && !in_double_quote => return true,
            _ => {}
        }
    }

    false
}

fn is_safe_set_variable_value(value: &str) -> bool {
    let value = value.trim();
    value == "undef"
        || is_quoted_literal(value)
        || is_numeric_literal(value)
        || is_valid_set_variable_name(value)
}

fn is_quoted_literal(value: &str) -> bool {
    let Some(quote) = value.chars().next().filter(|ch| *ch == '\'' || *ch == '"') else {
        return false;
    };

    let mut escaped = false;
    for (idx, ch) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            return idx + ch.len_utf8() == value.len();
        }
    }

    false
}

fn is_numeric_literal(value: &str) -> bool {
    let normalized: String = value.chars().filter(|ch| *ch != '_').collect();
    let has_digit = normalized.chars().any(|ch| ch.is_ascii_digit());
    let allowed_chars = normalized
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | 'e' | 'E'));

    has_digit && allowed_chars && normalized.parse::<f64>().is_ok()
}

// ---------------------------------------------------------------------------
// Hazard-class invariant tests (inline lib tests for patch coverage)
//
// These cover three hazard classes from SPEC_UPDATE_CHECKLIST §8:
//   1. Protocol-safety  — invalid/unknown/stale ref → success=true, variables=[]
//   2. Bounds/overflow  — extreme i64 values → checked, no panic/wrap
//   3. ID/ref-space     — eval_ref range (1_000_000+, from #1219) passes through
//                         the invalid-ref guard without being misrouted/rejected
// ---------------------------------------------------------------------------

#[cfg(test)]
mod hazard_invariant_tests {
    use super::*;
    use crate::debug_adapter::DebugAdapter;
    use serde_json::json;

    fn adapter() -> DebugAdapter {
        DebugAdapter::new()
    }

    fn variables_body_is_empty(adapter: &mut DebugAdapter, variables_ref: i64) -> bool {
        let msg = adapter.handle_request(
            1,
            "variables",
            Some(json!({ "variablesReference": variables_ref })),
        );
        match msg {
            DapMessage::Response { success, body, .. } => {
                if !success {
                    return false;
                }
                let vars = body
                    .as_ref()
                    .and_then(|b| b.get("variables"))
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(usize::MAX);
                vars == 0
            }
            _ => false,
        }
    }

    fn variables_success(adapter: &mut DebugAdapter, variables_ref: i64) -> bool {
        match adapter.handle_request(
            1,
            "variables",
            Some(json!({ "variablesReference": variables_ref })),
        ) {
            DapMessage::Response { success, .. } => success,
            _ => false,
        }
    }

    // --- Protocol-safety: ref=0 → empty ---
    #[test]
    fn ref_zero_is_protocol_safe_empty() {
        let mut a = adapter();
        assert!(variables_body_is_empty(&mut a, 0), "ref=0 must return empty variables array");
    }

    // --- Protocol-safety: ref=-1 → empty (negative refs are invalid) ---
    #[test]
    fn ref_negative_is_protocol_safe_empty() {
        let mut a = adapter();
        for bad in [-1_i64, -100, i32::MIN as i64, i64::MIN] {
            assert!(
                variables_body_is_empty(&mut a, bad),
                "ref={bad} (negative) must return empty variables array"
            );
        }
    }

    // --- Bounds/overflow: i64::MAX → no panic, returns empty (> i32::MAX rejected) ---
    #[test]
    fn ref_i64_max_no_panic_returns_empty() {
        let mut a = adapter();
        // i64::MAX saturates to i32::MAX under i64_to_i32_saturating; raw-i64 check rejects it first.
        assert!(
            variables_body_is_empty(&mut a, i64::MAX),
            "ref=i64::MAX must return empty (out-of-i32-range guard)"
        );
        // Just above i32::MAX is also rejected.
        assert!(
            variables_body_is_empty(&mut a, i32::MAX as i64 + 1),
            "ref=i32::MAX+1 must return empty (out-of-range)"
        );
    }

    // --- Bounds/overflow: i32::MAX is in-range (allowed through, no panic) ---
    #[test]
    fn ref_i32_max_no_panic() {
        let mut a = adapter();
        // i32::MAX passes the raw-i64 guard and goes through normal path without session.
        // It must not panic — success=true is required.
        assert!(
            variables_success(&mut a, i32::MAX as i64),
            "ref=i32::MAX must succeed (in-range, no session → honest empty)"
        );
    }

    // --- ID/ref-space: eval_ref range (1_000_000+) is NOT rejected by invalid-ref guard ---
    //
    // #1219 allocates eval refs starting at 1_000_000 (base 1_000_000 + counter).
    // Those refs must pass through the invalid-ref check (0 < 1_000_000 <= i32::MAX) and
    // reach the normal cache-lookup path. Without a session they return honest-empty.
    // Crucially: they must NOT be misrouted as "invalid" just because they look large.
    #[test]
    fn eval_ref_range_passes_invalid_ref_guard() {
        let mut a = adapter();
        // These are valid eval refs from #1219 — not rejected, return success=true.
        for eval_ref in [1_000_000_i64, 1_000_001, 1_000_100, 1_999_999] {
            assert!(
                variables_success(&mut a, eval_ref),
                "eval_ref={eval_ref} (from #1219 range) must not be rejected by invalid-ref guard"
            );
        }
    }

    // --- ID/ref-space: scope refs (frame_id*10 + {1,2,3}) coexist with eval refs ---
    //
    // Scope ref range: frame_id in [0, ~200M] * 10 + {1,2,3} — well below 1_000_000 for
    // reasonable frame counts (< 100_000 frames → refs < 1_000_003). Eval refs start at
    // 1_000_000. Both ranges must produce success=true without session (honest empty).
    #[test]
    fn scope_ref_and_eval_ref_ranges_do_not_collide_for_small_frame_ids() {
        let mut a = adapter();
        // Scope refs for frame_id <=99_999 are in [1, 999_993]; eval refs from #1219
        // start at 1_000_000 — no overlap. Encoding invariant documented in #901/#1219.
        let max_scope_ref: i64 = 99_999 * 10 + 3; // = 999_993
        let min_eval_ref: i64 = 1_000_000;
        // Verify both values PASS the invalid-ref guard ([1, i32::MAX]) and return success.
        // This tests actual guard behavior — the range-non-collision is a precondition
        // documented above, not an assertion on the code under test.
        assert!(
            variables_success(&mut a, max_scope_ref),
            "max scope ref ({max_scope_ref}) must pass invalid-ref guard and succeed"
        );
        assert!(
            variables_success(&mut a, min_eval_ref),
            "min eval ref ({min_eval_ref}) must pass invalid-ref guard and succeed"
        );
    }

    // --- Protocol-safety: never-allocated ref (valid range, but unknown to cache) → no crash ---
    #[test]
    fn never_allocated_ref_in_valid_range_no_crash() {
        let mut a = adapter();
        // A ref that looks like a scope ref but was never actually allocated — no session,
        // so it goes through the no-session path. Must not panic.
        for stale in [11_i64, 12, 13, 21, 22, 23, 999] {
            assert!(
                variables_success(&mut a, stale),
                "never-allocated scope ref={stale} must succeed (no crash, honest empty)"
            );
        }
    }

    #[test]
    fn package_globals_and_noncurrent_scope_refs_are_rejected_before_query()
    -> Result<(), Box<dyn std::error::Error>> {
        if std::process::Command::new("perl").arg("-e").arg("1").output().is_err() {
            return Err("Perl is required for the locals lifecycle proof".into());
        }
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
        use crate::types::StackFrame;

        let mut a = adapter();
        a.seed_stopped_session_with_frames_for_test(vec![StackFrame::new(
            7,
            "main::run",
            crate::types::Source {
                name: Some("test.pl".to_string()),
                path: "/tmp/test.pl".to_string(),
                source_reference: None,
            },
            1,
        )]);

        let before_queries = a.debugger_query_count_for_test();
        for (frame_id, kind) in [
            (7, ScopeKind::Package),
            (7, ScopeKind::Globals),
            (8, ScopeKind::Locals),
            (8, ScopeKind::Arguments),
        ] {
            let wire = must_some(VariableReference::Scope { frame_id, kind }.encode());
            assert!(
                variables_body_is_empty(&mut a, i64::from(wire)),
                "unadmitted {kind:?} scope for frame {frame_id} must be honest empty"
            );
        }
        assert_eq!(
            a.debugger_query_count_for_test(),
            before_queries,
            "unadmitted scope references must perform zero framed debugger queries"
        );
        Ok(())
    }

    #[test]
    fn cleared_session_does_not_revive_stale_scope_from_recent_output()
    -> Result<(), Box<dyn std::error::Error>> {
        if std::process::Command::new("perl").arg("-e").arg("1").output().is_err() {
            return Ok(());
        }
        let mut a = adapter();
        a.seed_stopped_session_with_frames_for_test(vec![]);
        a.push_recent_output_line_for_test("$stale = from-an-older-session");
        a.clear_active_session_state();

        assert!(
            variables_body_is_empty(&mut a, 11),
            "a scope ref must stay empty after its session is cleared"
        );
        Ok(())
    }

    #[test]
    fn unknown_reference_does_not_parse_recent_output() -> Result<(), Box<dyn std::error::Error>> {
        if std::process::Command::new("perl").arg("-e").arg("1").output().is_err() {
            return Ok(());
        }
        let mut a = adapter();
        a.seed_stopped_session_with_frames_for_test(vec![]);
        a.push_recent_output_line_for_test("$stale = from-an-unknown-reference");

        // 999_999 is in the valid wire range but is not a cache or scope
        // reference, so recent output must not be treated as its children.
        assert!(
            variables_body_is_empty(&mut a, 999_999),
            "an unknown reference must not be correlated with recent output"
        );
        Ok(())
    }

    // --- Fix #1338: stale EvalResult ref with Stopped session -> early short-circuit ---
    //
    // This lib test covers the new early-return branch in handle_variables() added by
    // fix #1338 (cache-miss branch, EvalResult short-circuit in variables.rs).
    //
    // Path exercised:
    //   1. Session IS Stopped -> passes the Running-state guard (lines 73-92 pre-fix)
    //   2. Cache miss for eval_ref wire -> enters else branch
    //   3. decode yields EvalResult -> short-circuit, return honest empty immediately
    //
    // Without the fix, control falls through to parse_scope_variables_from_output
    // (75ms detour via wait_for_debugger_output_window) before returning empty via
    // fallback_scope_variables. The short-circuit removes the delay and bogus routing.
    //
    // Skip when perl is not on PATH (seed_stopped_session_with_frames_for_test
    // spawns perl -e 1 as a no-op child process).
    #[test]
    fn fix_1338_stale_eval_ref_stopped_session_short_circuits_to_honest_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        // Skip if perl is not available on PATH.
        if std::process::Command::new("perl").arg("-e").arg("1").output().is_err() {
            return Ok(());
        }
        let mut a = adapter();
        // Seed a Stopped session so the Running-state guard does not trigger.
        // This exercises the cache-miss path and the new EvalResult short-circuit.
        a.seed_stopped_session_with_frames_for_test(vec![]);

        // EvalResult band wire values: stale after resume (not in cache).
        for eval_ref_wire in [1_000_000_i64, 1_000_001, 1_000_003, 1_100_000] {
            assert!(
                variables_body_is_empty(&mut a, eval_ref_wire),
                "fix #1338: stopped session + stale eval_ref={eval_ref_wire} must return honest empty"
            );
        }
        Ok(())
    }

    // --- Guard test: cached EvalResult is NOT short-circuited by the fix #1338 early return ---
    //
    // This is the scoping guard for the fix: a VALID EvalResult ref that IS in the cache
    // must be served via the cache-hit path (line 102) and return its children — it must
    // NOT be swallowed by the early-return short-circuit (which fires only on cache miss).
    //
    // Without this guard, a regression could incorrectly apply the early return to ALL
    // EvalResult refs (cached or not), causing legitimate variable expansion to return
    // empty. This test would fail immediately in that case.
    //
    // Skip when perl is not on PATH.
    #[test]
    fn fix_1338_cached_eval_result_is_served_not_short_circuited() -> Result<(), String> {
        // Skip if perl is not available on PATH.
        if std::process::Command::new("perl").arg("-e").arg("1").output().is_err() {
            return Ok(());
        }
        use crate::debug_adapter::var_ref::VariableReference;
        use crate::types::Variable;

        let mut a = adapter();
        a.seed_stopped_session_with_frames_for_test(vec![]);

        // An EvalResult wire value that IS in cache (simulates a fresh evaluate result
        // before resume — the client holds the ref and sends a variables request while
        // the session is still stopped at the same breakpoint).
        let eval_ref_wire: i32 = must_some(VariableReference::EvalResult { counter: 42 }.encode());
        assert!(
            (1_000_000..=1_999_999_999).contains(&eval_ref_wire),
            "setup: must be in EvalResult band"
        );

        // Seed a cached entry so the cache-hit path fires.
        let cached_var = Variable {
            name: "".to_string(),
            value: "42".to_string(),
            type_: Some("SCALAR".to_string()),
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            evaluate_name: None,
        };
        a.seed_eval_result_cache_for_test(eval_ref_wire, vec![cached_var]);

        // Must return the cached children (non-empty), NOT the early-return empty.
        // If the early return incorrectly fired here, this assertion would fail.
        let msg = a.handle_request(
            1,
            "variables",
            Some(serde_json::json!({ "variablesReference": eval_ref_wire as i64 })),
        );
        match msg {
            DapMessage::Response { success, body, .. } => {
                assert!(success, "cached EvalResult must succeed");
                let vars = body
                    .as_ref()
                    .and_then(|b| b.get("variables"))
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                assert_eq!(
                    vars, 1,
                    "cached EvalResult must return its 1 cached child; got {vars} (early return was applied incorrectly)"
                );
            }
            other => return Err(format!("expected Response, got: {other:?}")),
        }

        Ok(())
    }

    // --- build_locals_b_eval_cmd: Perl command template tests ---
    //
    // These tests verify that the B-module eval command always uses the
    // innermost lexical pad and includes B::AV / B::HV type detection.
    // No live Perl or debugger session is needed — these are pure string tests.

    #[test]
    fn build_locals_b_eval_cmd_frame0_uses_innermost_pad() {
        let cmd = DebugAdapter::build_locals_b_eval_cmd();
        // The Perl code uses a $fi variable and falls back to $va[-1] when the PADLIST
        // has no slot N pads back.  The current-frame offset is always zero and the primary index is
        // $va[-(1+0)] = $va[-1], which is the innermost pad.
        assert!(cmd.contains("my $fi=0;"), "current-frame locals must embed $fi=0: {cmd}");
        assert!(
            cmd.contains("$va[-(1+$fi)]"),
            "Perl code must use $va[-(1+$fi)] for frame-offset indexing: {cmd}"
        );
    }

    #[test]
    fn build_locals_b_eval_cmd_contains_av_hv_detection() {
        let cmd = DebugAdapter::build_locals_b_eval_cmd();
        // B::AV detection for array variables (@foo).
        assert!(cmd.contains("'B::AV'"), "Perl code must check for B::AV (array variables): {cmd}");
        // B::HV detection for hash variables (%foo).
        assert!(cmd.contains("'B::HV'"), "Perl code must check for B::HV (hash variables): {cmd}");
        // Arrays must produce an ARRAY(0x0) value parseable by VariableParser.
        assert!(
            cmd.contains("ARRAY(0x0)"),
            "Perl code must format array vars as ARRAY(0x0): {cmd}"
        );
        // Hashes must produce a HASH(0x0) value parseable by VariableParser.
        assert!(cmd.contains("HASH(0x0)"), "Perl code must format hash vars as HASH(0x0): {cmd}");
    }

    /// A read-only `variables` request must not enumerate or serialize live
    /// aggregates. Bounded lexical collection snapshots are owned by #7358; until
    /// that contract lands, re-introducing an unbudgeted traversal here is a
    /// regression, so the command template must stay free of it. The one
    /// admitted use of `object_2svref` (#9590) is the ROK scalar branch: the
    /// reference is handed straight to `overload::StrVal` — the documented
    /// hook-free stringifier — to read the address without invoking overloaded
    /// `""` (which the raw `->IV` numification path executes on AMG-carrying
    /// references). It must never feed a traversal.
    #[test]
    fn build_locals_b_eval_cmd_does_not_enumerate_live_collections() {
        let cmd = DebugAdapter::build_locals_b_eval_cmd();
        for forbidden in ["Data::Dumper", "Dumper(", "keys %$", "@$r"] {
            assert!(
                !cmd.contains(forbidden),
                "lexical introspection must not use {forbidden} \
                 (unbudgeted traversal / debuggee side effects, see #7358): {cmd}"
            );
        }
        assert_eq!(
            cmd.matches("object_2svref").count(),
            1,
            "object_2svref may only appear in the ROK scalar branch: {cmd}"
        );
        assert!(
            cmd.contains("B::SVf_ROK()"),
            "the object_2svref read must be gated on SVf_ROK: {cmd}"
        );
        assert!(
            cmd.contains("overload::StrVal($qr)"),
            "the obtained reference must be consumed only by overload::StrVal: {cmd}"
        );
        let rok_branch = cmd
            .split("B::SVf_ROK()")
            .nth(1)
            .unwrap_or_default()
            .split("elsif")
            .next()
            .unwrap_or_default();
        assert!(
            rok_branch.contains("$s->RV->object_2svref") && rok_branch.contains("overload::StrVal"),
            "object_2svref must stay inside the ROK branch and address the referent, not the pad slot: {cmd}"
        );
        assert!(
            cmd.contains("$v='ARRAY(0x0)'") && cmd.contains("$v='HASH(0x0)'"),
            "aggregate pad entries must keep their opaque non-enumerated markers: {cmd}"
        );
    }

    /// The child-reference codec and child cache serve pages beyond the first 256
    /// entries whenever a *parseable* one-line aggregate literal reaches the
    /// parser. This proves the codec repair independently of how such a line is
    /// produced — the lexical `B` path deliberately does not produce one (#7358).
    #[test]
    fn parsed_array_literal_preserves_a_deep_page() {
        let values = (1..=500).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let lines = vec![format!("@big = [{values}]")];
        let (roots, child_cache) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            0,
            1024,
            DebuggerOutputOrigin::FixtureOrInstrumentInput,
            ParseIdentity::new(),
        );
        let root = must_some(roots.iter().find(|variable| variable.row.name == "@big"));
        assert_eq!(root.row.indexed_variables, Some(500));
        assert!(root.row.variables_reference > 0);

        let children = must_some(child_cache.get(&root.row.variables_reference));
        assert_eq!(children.len(), 500);
        assert_eq!(children[250].row.name, "[250]");
        assert_eq!(children[250].row.value, "251");
        assert_eq!(children[274].row.name, "[274]");
        assert_eq!(children[274].row.value, "275");
        // Cached children retain typed facts so a later request's ValueFormat
        // projects from typed authority (#9588).
        assert!(
            matches!(children[250].typed, Some(crate::value::PerlValue::Integer(251))),
            "child rows must retain typed facts, got {:?}",
            children[250].typed
        );
    }

    #[test]
    fn build_locals_b_eval_cmd_output_format_matches_variable_parser() {
        // The Perl code emits "$name = value\n" lines.  Verify the template
        // contains both the "= $v" assignment format and the double-quote for Perl
        // variable interpolation (both are required for parse_assignment to succeed).
        let cmd = DebugAdapter::build_locals_b_eval_cmd();
        assert!(cmd.contains("$o.="), "Perl code must append to $o for each variable: {cmd}");
        // The variable/value format string uses double-quote interpolation.
        assert!(
            cmd.contains("= $v"),
            "Perl emit format must contain '= $v' for parse_assignment compatibility: {cmd}"
        );
    }

    #[test]
    fn build_locals_b_eval_cmd_reads_raw_slots_only() {
        // #9590's public stdio canaries proved the old read chain
        // (`$s->SV->PV`/`$s->SV->IV`) executed tied `FETCH` and overloaded
        // `""`/numification during a read-only locals dump, and lost NV-only
        // scalars entirely. The command must read raw slots gated on FLAGS,
        // take reference addresses through `overload::StrVal` (which bypasses
        // overloaded stringification), cover the NV slot, and never emit a
        // flagged string into perl5db's print (whose "Wide character" warning
        // echoes the whole eval back into the control stream).
        let cmd = DebugAdapter::build_locals_b_eval_cmd();
        assert!(
            !cmd.contains("$s->SV->PV") && !cmd.contains("$s->SV->IV"),
            "the stringify-prone read chain must stay gone: {cmd}"
        );
        assert!(cmd.contains("B::SVf_ROK()"), "references must be gated on SVf_ROK: {cmd}");
        assert!(
            cmd.contains("overload::StrVal"),
            "reference addresses must be read hook-free via overload::StrVal: {cmd}"
        );
        assert!(cmd.contains("B::SVf_POK()"), "string values must be gated on SVf_POK: {cmd}");
        assert!(cmd.contains("B::SVf_IOK()"), "integers must be gated on SVf_IOK: {cmd}");
        assert!(cmd.contains("B::SVf_NOK()"), "floats must be read from the NV slot: {cmd}");
        assert!(
            cmd.contains("utf8::encode"),
            "wide PVs must be encoded to UTF-8 bytes before reaching perl5db's print: {cmd}"
        );
    }
}

// ---------------------------------------------------------------------------
// #9588 - DAP `ValueFormat` family tests
//
// One typed presentation policy shared by every `ValueFormat` request family
// (variables / setVariable / evaluate / setExpression per the pinned upstream
// schema). These tests cover the per-family behaviors the issue demands:
// default/no-format, hex projection from typed authority, unknown/unsupported
// option rejection (never silently ignored), malformed input, identity
// independence, and no cross-request format leak through the shared cache.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod value_format_family_tests {
    use super::*;
    use crate::debug_adapter::var_ref::VariableReference;
    use crate::debug_adapter::variable_cache::{CachedVariable, VariableCacheKind};
    use crate::types::{Source, StackFrame, Variable};
    use serde_json::json;
    use std::error::Error;

    type TestResult = Result<(), Box<dyn Error>>;

    fn perl_available() -> bool {
        std::process::Command::new("perl").arg("-e").arg("1").output().is_ok()
    }

    /// Frame 1 as the exact current stopped frame, so the Locals scope wire
    /// reference for frame 1 (11) passes current-frame admission.
    fn seed_current_frame(adapter: &DebugAdapter) -> TestResult {
        adapter.seed_stopped_session_with_frames_for_test(vec![StackFrame::new(
            1,
            "main::run",
            Source {
                name: Some("test.pl".to_string()),
                path: "/tmp/test.pl".to_string(),
                source_reference: None,
            },
            3,
        )]);
        Ok(())
    }

    fn seed_typed_roots(adapter: &DebugAdapter, wire: i32) {
        let lines = vec![
            "$n = 255".to_string(),
            "$neg = -42".to_string(),
            "$f = 2.5".to_string(),
            "$s = 'hello'".to_string(),
            "$u = undef".to_string(),
            "$zero = 0".to_string(),
        ];
        let (roots, _children) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            wire,
            0,
            16,
            DebuggerOutputOrigin::FixtureOrInstrumentInput,
            ParseIdentity::new(),
        );
        let mut session = lock_or_recover(&adapter.session, "value_format_family_tests.seed");
        if let Some(ref mut sess) = *session {
            sess.variable_cache.upsert(wire, VariableCacheKind::Root, roots);
        }
    }

    fn response_value_at(
        adapter: &mut DebugAdapter,
        wire: i64,
        format: Option<serde_json::Value>,
        index: usize,
    ) -> Result<serde_json::Value, String> {
        let mut arguments = json!({ "variablesReference": wire });
        if let Some(format) = format {
            arguments["format"] = format;
        }
        match adapter.handle_request(1, "variables", Some(arguments)) {
            DapMessage::Response { success: true, body: Some(body), .. } => body
                .get("variables")
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(index))
                .cloned()
                .ok_or_else(|| format!("expected variable row {index}")),
            other => Err(format!("expected successful variables response, got {other:?}")),
        }
    }

    fn response_message(
        adapter: &mut DebugAdapter,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        match adapter.handle_request(1, command, Some(arguments)) {
            DapMessage::Response { success: false, message: Some(message), .. } => Ok(message),
            other => Err(format!("expected failed {command} response, got {other:?}")),
        }
    }

    // --- variables family: #9581 floor + default-contract integrity ---------

    #[test]
    fn hex_requests_are_floored_and_default_projection_preserves_identity() -> TestResult {
        if !perl_available() {
            return Ok(());
        }
        let mut adapter = DebugAdapter::new();
        seed_current_frame(&adapter)?;
        seed_typed_roots(&adapter, 11);

        // #9581: a hex request is rejected before any cache read or projection.
        let hexed = response_value_at(&mut adapter, 11, Some(json!({ "hex": true })), 0);
        let hexed_err = match hexed {
            Err(message) => message,
            Ok(row) => {
                return Err(format!(
                    "hex requests must be floored-rejected (#9581), got row: {row:?}"
                )
                .into());
            }
        };
        assert!(
            hexed_err.contains("supportsValueFormattingOptions"),
            "expected the #9581 floor rejection, got: {hexed_err}"
        );

        // Rows are sorted by name: $f, $n, $neg, $s, $u, $zero. The default
        // contract is untouched by the floor and identity fields stay exact.
        let f = response_value_at(&mut adapter, 11, None, 0)?;
        assert_eq!(f["value"], "2.5");
        let n = response_value_at(&mut adapter, 11, None, 1)?;
        assert_eq!(n["value"], "255", "default decimal rendering is unchanged");
        assert_eq!(n["name"], "$n", "identity fields are unaffected by the floor");
        assert_eq!(n["type"], "SCALAR");
        assert_eq!(n["evaluateName"], "$n");
        let neg = response_value_at(&mut adapter, 11, None, 2)?;
        assert_eq!(neg["value"], "-42");
        let s = response_value_at(&mut adapter, 11, None, 3)?;
        assert_eq!(s["value"], "\"hello\"");
        let u = response_value_at(&mut adapter, 11, None, 4)?;
        assert_eq!(u["value"], "undef");
        let zero = response_value_at(&mut adapter, 11, None, 5)?;
        assert_eq!(zero["value"], "0");
        Ok(())
    }

    #[test]
    fn floored_hex_never_leaks_into_default_requests_sharing_the_cache() -> TestResult {
        if !perl_available() {
            return Ok(());
        }
        let mut adapter = DebugAdapter::new();
        seed_current_frame(&adapter)?;
        seed_typed_roots(&adapter, 11);

        // Hex first (floored, no effect), then default on the same cached
        // reference: the response must be decimal — the floor mutated nothing.
        let floored = response_value_at(&mut adapter, 11, Some(json!({ "hex": true })), 1);
        let err = match floored {
            Err(message) => message,
            Ok(row) => {
                return Err(format!(
                    "hex requests must be floored-rejected (#9581), got row: {row:?}"
                )
                .into());
            }
        };
        assert!(err.contains("supportsValueFormattingOptions"), "got: {err}");
        assert_eq!(response_value_at(&mut adapter, 11, None, 1)?["value"], "255");
        // And the floor is stable across requests on the same reference.
        let again = response_value_at(&mut adapter, 11, Some(json!({ "hex": true })), 1);
        assert!(again.is_err(), "hex stays floored on the same reference");
        Ok(())
    }

    #[test]
    fn variables_hex_false_and_empty_format_behave_as_default() -> TestResult {
        if !perl_available() {
            return Ok(());
        }
        let mut adapter = DebugAdapter::new();
        seed_current_frame(&adapter)?;
        seed_typed_roots(&adapter, 11);

        assert_eq!(
            response_value_at(&mut adapter, 11, Some(json!({ "hex": false })), 1)?["value"],
            "255"
        );
        assert_eq!(response_value_at(&mut adapter, 11, Some(json!({})), 1)?["value"], "255");
        Ok(())
    }

    #[test]
    fn child_rows_serve_the_default_contract_under_the_format_floor() -> TestResult {
        if !perl_available() {
            return Ok(());
        }
        let mut adapter = DebugAdapter::new();
        seed_current_frame(&adapter)?;

        let lines = vec!["@arr = [10, 20]".to_string()];
        let (roots, children) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            0,
            16,
            DebuggerOutputOrigin::FixtureOrInstrumentInput,
            ParseIdentity::new(),
        );
        let child_ref = roots[0].row.variables_reference;
        {
            let mut session = lock_or_recover(&adapter.session, "value_format_family_tests.child");
            if let Some(ref mut sess) = *session {
                sess.variable_cache.upsert(11, VariableCacheKind::Root, roots);
                for (reference, rows) in children {
                    sess.variable_cache.upsert(reference, VariableCacheKind::Child, rows);
                }
            }
        }
        assert!(child_ref > 0, "fixture must produce an expandable child ref");

        // #9581: a hex request on a child reference is floored-rejected...
        let floored =
            response_value_at(&mut adapter, i64::from(child_ref), Some(json!({ "hex": true })), 0);
        assert!(floored.is_err(), "hex child projection must be floored-rejected");
        // ...and the default child row keeps rendering from the cache.
        let first_default = response_value_at(&mut adapter, i64::from(child_ref), None, 0)?;
        assert_eq!(first_default["value"], "10");
        Ok(())
    }

    // --- unknown/unsupported option: one documented behavior (fail) ---------

    #[test]
    fn variables_unknown_format_option_fails_the_request() -> TestResult {
        let mut adapter = DebugAdapter::new();
        let message = response_message(
            &mut adapter,
            "variables",
            json!({ "variablesReference": 11, "format": { "octal": true } }),
        )?;
        assert!(message.contains("Invalid arguments"), "got: {message}");
        assert!(message.contains("octal"), "unknown option must be named: {message}");
        Ok(())
    }

    #[test]
    fn variables_wrong_typed_hex_option_fails_the_request() -> TestResult {
        let mut adapter = DebugAdapter::new();
        let message = response_message(
            &mut adapter,
            "variables",
            json!({ "variablesReference": 11, "format": { "hex": "true" } }),
        )?;
        assert!(message.contains("Invalid arguments"), "got: {message}");
        Ok(())
    }

    #[test]
    fn mutation_and_evaluate_families_reject_unknown_format_options() -> TestResult {
        let mut adapter = DebugAdapter::new();
        // Argument deserialization fails before any session or mutation work.
        // setVariable is deliberately absent from the family loop: since
        // #8354 its capability gate refuses before argument parsing, so
        // setExpression carries the mutation-family admission proof. The
        // ordering claim itself (gate precedes screening) is asserted below
        // and in dap_setvariable_capability_fail_closed_8354.
        for (command, arguments) in [
            ("evaluate", json!({ "expression": "$x", "format": { "radix": 16 } })),
            (
                "setExpression",
                json!({ "expression": "$x", "value": "5", "format": { "radix": 16 } }),
            ),
        ] {
            let message = response_message(&mut adapter, command, arguments)?;
            assert!(
                message.contains("Invalid arguments") && message.contains("radix"),
                "{command} must reject the unknown option by name: {message}"
            );
        }
        // setVariable must refuse on the capability floor before it would ever
        // reach this argument validation.
        let message = response_message(
            &mut adapter,
            "setVariable",
            json!({ "variablesReference": 11, "name": "$x", "value": "5", "format": { "radix": 16 } }),
        )?;
        assert!(
            message.contains("supportsSetVariable"),
            "setVariable must be refused by the #8354 capability gate, not by argument \
             validation: {message}"
        );
        Ok(())
    }

    #[test]
    fn valid_hex_format_is_floored_on_all_four_families_and_set_expression_refused() -> TestResult {
        // #9581: a well-formed hex format is no longer consumed by the
        // handlers — the capability floor rejects every family explicitly
        // BEFORE deserialization/session work, and never silently ignores it.
        //
        // #8354: setVariable is absent from this family because its capability
        // gate refuses before argument parsing, so evaluate carries the
        // format-acceptance proof.
        //
        // #9568: setExpression refuses at the capability floor before any
        // session work, so its well-formed-format leg expects the authority
        // refusal instead of "No debugger session". Typed deserialization is
        // still proven here: a *malformed* format on the same command fails
        // with "Invalid arguments" (see
        // `mutation_and_evaluate_families_reject_unknown_format_options`),
        // which runs before this same gate.
        let mut adapter = DebugAdapter::new();
        // #9568: setVariable and setExpression hex requests are refused by
        // their own exact-mutation authority (SET_VARIABLE_UNSUPPORTED_MESSAGE
        // / SET_EXPRESSION_UNSUPPORTED_MESSAGE) before the ValueFormat floor
        // is consulted — covered by that authority's tests. The VFO floor's
        // own refusal contract covers the remaining two families:
        for (command, arguments) in [
            ("evaluate", json!({ "expression": "$x", "format": { "hex": true } })),
            ("variables", json!({ "variablesReference": 11, "format": { "hex": true } })),
        ] {
            let message = response_message(&mut adapter, command, arguments)?;
            assert!(
                message.contains("unsupported")
                    && message.contains("supportsValueFormattingOptions"),
                "{command} must get the #9581 floor rejection: {message}"
            );
        }
        // #9568 note: setExpression's dedicated authority refusal
        // (SET_EXPRESSION_UNSUPPORTED_MESSAGE) is unreachable for a
        // well-formed format while the #9581 ValueFormat floor holds — the
        // floor rejects the request first (asserted above for setExpression
        // among the four families). The #9568 message re-emerges exactly when
        // its re-enable gate (#9570 promotion boundary) lands.
        Ok(())
    }

    #[test]
    fn missing_arguments_message_is_preserved() -> TestResult {
        // Regression guard: `None` arguments still report "Missing arguments"
        // (existing integration tests assert this exact message). setVariable
        // is absent since #8354: its capability gate refuses before argument
        // parsing, so it can no longer produce this message.
        let mut adapter = DebugAdapter::new();
        for command in ["variables", "evaluate", "setExpression"] {
            let outcome = adapter.handle_request(1, command, None);
            match outcome {
                DapMessage::Response { success: false, message: Some(message), .. } => {
                    assert_eq!(message, "Missing arguments", "{command}: {message}");
                }
                other => {
                    return Err(format!(
                        "{command} must fail with Missing arguments, got {other:?}"
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    // --- EvaluateResult placeholder expansion under the format floor --------

    #[test]
    fn eval_result_placeholder_rows_serve_the_default_contract_under_the_floor() -> TestResult {
        if !perl_available() {
            return Ok(());
        }
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test()?; // EvalResult refs bypass frame admission

        let wire: i32 = VariableReference::EvalResult { counter: 7 }
            .encode()
            .ok_or("EvalResult counter=7 must encode")?;
        let row = Variable {
            name: "$expr".to_string(),
            value: "48879".to_string(),
            type_: Some("SCALAR".to_string()),
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            evaluate_name: Some("$expr".to_string()),
        };
        {
            let mut session =
                lock_or_recover(&adapter.session, "value_format_family_tests.evalref");
            if let Some(ref mut sess) = *session {
                sess.variable_cache.upsert(
                    wire,
                    VariableCacheKind::EvaluateResult,
                    vec![CachedVariable::typed(row, crate::value::PerlValue::Integer(48879))],
                );
            }
        }

        let mut adapter_mut = adapter;
        // #9581: a hex request on an EvalResult reference is floored-rejected.
        let floored =
            response_value_at(&mut adapter_mut, i64::from(wire), Some(json!({ "hex": true })), 0);
        assert!(floored.is_err(), "hex EvalResult projection must be floored-rejected");
        // The default contract serves the cached decimal row unchanged.
        let decimal = response_value_at(&mut adapter_mut, i64::from(wire), None, 0)?;
        assert_eq!(decimal["value"], "48879");
        assert_eq!(decimal["name"], "$expr");
        Ok(())
    }
    fn install_locals_test_session(
        adapter: &DebugAdapter,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::process::{Command, Stdio};
        let previous = lock_or_recover(&adapter.session, "test.replace_session").take();
        if let Some(mut previous) = previous {
            let _ = previous.process.kill();
            let _ = previous.process.wait();
        }
        adapter.seed_stopped_session_with_frames_for_test(vec![crate::types::StackFrame::new(
            7,
            "main::test",
            crate::types::Source::new("fixture.pl"),
            1,
        )]);
        let child = Command::new("perl")
            .args(["-e", "while (<STDIN>) {}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut old = {
            let mut session = lock_or_recover(&adapter.session, "test.install_pipe");
            std::mem::replace(&mut session.as_mut().ok_or("missing session")?.process, child)
        };
        let _ = old.kill();
        let _ = old.wait();
        Ok(())
    }

    fn pending_locals_query(
        adapter: &std::sync::Arc<DebugAdapter>,
        marker: u64,
    ) -> Result<std::thread::JoinHandle<DapMessage>, Box<dyn std::error::Error>> {
        let request_adapter = std::sync::Arc::clone(adapter);
        let request = std::thread::spawn(move || {
            request_adapter.handle_variables(1, 1, Some(json!({ "variablesReference": 71 })))
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while adapter.debugger_query_count_for_test() < marker
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if adapter.debugger_query_count_for_test() < marker {
            let _ = request.join();
            return Err("locals query was not submitted".into());
        }
        Ok(request)
    }

    fn deliver_locals(adapter: &DebugAdapter, marker: u64, payload: &str) {
        adapter.push_recent_output_line_for_test(&format!("DAP_BEGIN_{marker}"));
        adapter.push_recent_output_line_for_test(payload);
        adapter.push_recent_output_line_for_test(&format!("DAP_END_{marker}"));
    }

    fn require_empty_locals(response: DapMessage) -> Result<(), Box<dyn std::error::Error>> {
        match response {
            DapMessage::Response { success: true, body: Some(body), .. }
                if body.get("variables").and_then(Value::as_array).is_some_and(Vec::is_empty)
                    && body.get("totalVariables").is_none() =>
            {
                Ok(())
            }
            other => Err(format!("stale locals were not honest-empty: {other:?}").into()),
        }
    }

    fn require_fresh_locals(
        adapter: &DebugAdapter,
        response: DapMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let DapMessage::Response { success: true, body: Some(body), .. } = response else {
            return Err(format!("fresh locals failed: {response:?}").into());
        };
        let rows = body.get("variables").and_then(Value::as_array).ok_or("missing rows")?;
        let row = rows.first().ok_or("fresh locals unexpectedly empty")?;
        let child =
            row.get("variablesReference").and_then(Value::as_i64).ok_or("missing child ref")?;
        if rows.len() != 1
            || row.get("name").and_then(Value::as_str) != Some("@fresh")
            || child <= 0
            || body.get("totalVariables").and_then(Value::as_i64) != Some(1)
        {
            return Err(format!("incorrect fresh locals: {body}").into());
        }
        let mut session = lock_or_recover(&adapter.session, "test.fresh_cache");
        let cache = &mut session.as_mut().ok_or("missing fresh session")?.variable_cache;
        if cache.root_count(71) != Some(1) {
            return Err("fresh root was not cached".into());
        }
        let children = cache.get_page(i32::try_from(child)?, 0, 10).ok_or("child cache missing")?;
        if children.len() != 2
            || children.first().map(|v| v.row.value.as_str()) != Some("42")
            || children.get(1).map(|v| v.row.value.as_str()) != Some("43")
        {
            return Err(format!("wrong cached children: {children:?}").into());
        }
        Ok(())
    }

    #[test]
    fn delayed_locals_query_does_not_hold_session_lock() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = std::sync::Arc::new(DebugAdapter::new());
        install_locals_test_session(&adapter)?;
        let request = pending_locals_query(&adapter, 1)?;
        let lock_available = (0..100).any(|_| {
            if let Ok(guard) = adapter.session.try_lock() {
                drop(guard);
                true
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1));
                false
            }
        });
        deliver_locals(&adapter, 1, "@fresh = [42, 43]");
        let response = request.join().map_err(|_| "locals thread panicked")?;
        if !lock_available {
            return Err("session lock remained held while locals response was pending".into());
        }
        require_fresh_locals(&adapter, response)
    }

    fn reject_delayed_locals_and_recover(
        transition: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let adapter = std::sync::Arc::new(DebugAdapter::new());
        install_locals_test_session(&adapter)?;
        let request = pending_locals_query(&adapter, 1)?;
        if transition == "replacement" {
            adapter.operation_broker.settle_all("test_replacement");
            install_locals_test_session(&adapter)?;
            adapter.operation_broker.open_session();
        } else {
            let mut guard = lock_or_recover(&adapter.session, "test.stop_transition");
            let session = guard.as_mut().ok_or("missing session")?;
            if transition == "running" {
                session.state = DebugState::Running;
            } else {
                session.stopped_generation = session.stopped_generation.saturating_add(1);
            }
        }
        deliver_locals(&adapter, 1, "@fresh = [42, 43]");
        require_empty_locals(request.join().map_err(|_| "stale locals thread panicked")?)?;
        {
            let mut guard = lock_or_recover(&adapter.session, "test.stale_cache");
            let session = guard.as_mut().ok_or("missing session")?;
            if session.variable_cache.all_variables().next().is_some() {
                return Err("stale locals populated root or child cache".into());
            }
            if transition == "running" {
                session.state = DebugState::Stopped;
                session.stopped_generation = session.stopped_generation.saturating_add(1);
            }
        }
        // Recover in the same replacement/new-stop session, not a third fresh fixture.
        let request = pending_locals_query(&adapter, 2)?;
        deliver_locals(&adapter, 2, "@fresh = [42, 43]");
        require_fresh_locals(&adapter, request.join().map_err(|_| "recovery thread panicked")?)
    }

    #[test]
    fn delayed_locals_rejects_running_then_recovers() -> Result<(), Box<dyn std::error::Error>> {
        reject_delayed_locals_and_recover("running")
    }

    #[test]
    fn delayed_locals_rejects_new_stop_then_recovers() -> Result<(), Box<dyn std::error::Error>> {
        reject_delayed_locals_and_recover("new_stop")
    }

    #[test]
    fn delayed_locals_rejects_replacement_then_recovers() -> Result<(), Box<dyn std::error::Error>>
    {
        reject_delayed_locals_and_recover("replacement")
    }
}
