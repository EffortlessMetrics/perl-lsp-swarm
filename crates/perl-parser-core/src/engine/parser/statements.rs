impl<'a> Parser<'a> {
    /// Parse a complete program
    fn parse_program(&mut self) -> ParseResult<Node> {
        let start = self.current_position();
        let mut statements = Vec::new();

        while !self.tokens.is_eof() {
            self.check_cancelled()?;

            // Check for UnknownRest token (lexer budget exceeded)
            if matches!(self.peek_kind(), Some(TokenKind::UnknownRest)) {
                let t = self.consume_token()?;
                statements.push(Node::new(
                    NodeKind::UnknownRest,
                    SourceLocation { start: t.start, end: t.end },
                ));
                break; // Stop parsing but preserve earlier nodes
            }

            // Parse statement with error recovery
            let stmt_result = self.parse_statement();
            match stmt_result {
                Ok(stmt) => statements.push(stmt),
                Err(e) => {
                    // Don't recover from these — propagate immediately
                    if matches!(
                        e,
                        ParseError::RecursionLimit
                            | ParseError::NestingTooDeep { .. }
                            | ParseError::Cancelled
                    ) {
                        return Err(e);
                    }

                    // Record the actual error
                    self.errors.push(e.clone());

                    // Create error node for failed statement
                    let error_location = self.current_position();
                    let error_msg = format!("{}", e);
                    // Collect peek_kind before mutable borrow in recover_from_error
                    let peek_display = self.peek_kind()
                        .map(|k| k.display_name())
                        .unwrap_or("end of input");
                    let error_node = self.recover_from_error(
                        error_msg,
                        "statement".to_string(),
                        peek_display.to_string(),
                        error_location
                    );
                    statements.push(error_node);

                    // Try to synchronize to next statement
                    if !self.synchronize() {
                        // If synchronization fails, we're likely at EOF
                        break;
                    }
                }
            }
        }

        let end = self.previous_position();
        Ok(Node::new(NodeKind::Program { statements }, SourceLocation { start, end }))
    }

    /// Parse a single statement
    fn parse_statement(&mut self) -> ParseResult<Node> {
        if self.peek_kind() == Some(TokenKind::LeftBrace) {
            return self.parse_statement_inner();
        }
        self.with_recursion_guard(|s| s.parse_statement_inner())
    }

    /// Check if the current token is a keyword that is being used as an
    /// autoquoted hash key before a fat arrow (`=>`).
    ///
    /// In Perl, any bareword before `=>` is treated as a string:
    /// ```perl
    /// my %h = (if => 1, for => 2, return => 3);
    /// ```
    fn is_keyword_before_fat_arrow(&mut self) -> bool {
        self.tokens
            .peek_second()
            .ok()
            .map(|t| t.kind == TokenKind::FatArrow)
            .unwrap_or(false)
    }

    /// Check whether the current keyword-like token is being used as a bare
    /// hash key rather than as its keyword/operator meaning.
    ///
    /// Perl permits reserved words as hash keys without quotes:
    /// `$self->{defer}` and `$bits{tie}` are valid. In those subscript
    /// contexts the token that follows the key is the subscript delimiter,
    /// so expression parsing should produce a bare identifier/string instead
    /// of dispatching to the keyword parser.
    fn is_keyword_hash_key_boundary(&mut self) -> bool {
        self.tokens
            .peek_second()
            .ok()
            .is_some_and(|t| matches!(t.kind, TokenKind::RightBrace | TokenKind::FatArrow))
    }

    fn is_async_sub_start(&mut self) -> bool {
        self.peek_kind() == Some(TokenKind::Identifier)
            && self.tokens.peek().ok().is_some_and(|t| t.text.as_ref() == "async")
            && self
                .tokens
                .peek_second()
                .ok()
                .is_some_and(|t| t.kind == TokenKind::Sub)
    }

    fn is_adjust_block_start(&mut self) -> bool {
        self.in_class_body > 0
            && self.peek_kind() == Some(TokenKind::Identifier)
            && self.tokens.peek().ok().is_some_and(|t| t.text.as_ref() == "ADJUST")
            && self
                .tokens
                .peek_second()
                .ok()
                .is_some_and(|t| t.kind == TokenKind::LeftBrace)
    }

    fn finish_subroutine_statement(&mut self, sub_node: Node) -> ParseResult<Node> {
        Ok(if let NodeKind::Subroutine { name, .. } = &sub_node.kind {
            if name.is_none() {
                // Anonymous sub may be followed by arrow or participate in a
                // comma-list expression: sub { 1 }, sub { 2 }
                let mut expr = if self.peek_kind() == Some(TokenKind::Arrow) {
                    self.parse_postfix_chain(sub_node)?
                } else {
                    sub_node
                };
                expr = self.collect_comma_fat_arrow_continuation(expr)?;
                expr = self.parse_word_or_expr(expr)?;
                // Wrap anonymous subroutines in expression statements
                let location = expr.location;
                Node::new(
                    NodeKind::ExpressionStatement { expression: Box::new(expr) },
                    location,
                )
            } else {
                // Named subroutines are statements by themselves
                sub_node
            }
        } else {
            // Shouldn't happen, but return as-is
            sub_node
        })
    }

    fn parse_statement_inner(&mut self) -> ParseResult<Node> {
        // Every new statement begins here
        self.at_stmt_start = true;
        // A surrounding compound statement can queue a heredoc while it parses
        // its condition, then recursively parse this statement as a block body.
        // Remember its queue length so this statement drains only declarations it adds.
        let pending_heredoc_start = self.pending_heredocs.len();

        // A `/` at statement start is always a regex delimiter, never division.
        // The lexer may be in ExpectOperator mode after a preceding block's `}`,
        // causing it to emit Division (Slash) instead of RegexMatch.  Roll back
        // and re-lex in ExpectTerm mode to get the correct token.
        if self.tokens.peek()?.kind == TokenKind::Slash {
            self.tokens.relex_as_term();
        }

        let kind = self.tokens.peek()?.kind;

        // Don't check for labels here - it breaks regular identifier parsing
        // Labels will be handled differently

        // In Perl, any bareword (including reserved keywords) before `=>` is
        // autoquoted as a string.  Detect this pattern early so that keyword
        // tokens such as `if`, `for`, `return`, `my`, etc. are NOT dispatched
        // to their keyword-specific parsers when they appear as hash keys.
        if Self::is_keyword_token(kind) && self.is_keyword_before_fat_arrow() {
            let token = self.consume_token()?;
            self.mark_not_stmt_start();
            // Produce a String node (autoquoting) and continue as an expression statement
            let key_node = Node::new(
                NodeKind::String { value: token.text.to_string(), interpolated: false },
                SourceLocation { start: token.start, end: token.end },
            );
            // Now parse the rest of the expression (=> value, more pairs, etc.)
            // Re-enter the comma parser with the key already consumed
            let mut stmt = self.finish_expression_from(key_node)?;
            // Check for statement modifiers on ANY statement
            if matches!(self.peek_kind(), Some(k) if Self::is_stmt_modifier_kind(k)) {
                stmt = self.parse_statement_modifier(stmt)?;
            }
            self.finish_statement_terminator(&stmt)?;
            self.drain_pending_heredocs_from(pending_heredoc_start, &mut stmt);
            return Ok(stmt);
        }

        if kind == TokenKind::Identifier {
            let keyword_text = self.tokens.peek()?.text.clone();
            let next_kind = self.tokens.peek_second().ok().map(|t| t.kind);

            if keyword_text.as_ref() == "else" && next_kind == Some(TokenKind::LeftBrace) {
                return self.parse_orphaned_else();
            }

            if keyword_text.as_ref() == "elsif" && next_kind == Some(TokenKind::LeftParen) {
                return self.parse_orphaned_elsif();
            }

            if self.is_adjust_block_start() {
                return self.parse_adjust_block();
            }
        }

        let mut stmt = if self.is_async_sub_start() {
            let async_token = self.consume_token()?;
            let mut sub_node = self.parse_subroutine()?;
            sub_node.location.start = async_token.start;
            if let NodeKind::Subroutine { attributes, .. } = &mut sub_node.kind
                && !attributes.iter().any(|attr| attr == "async")
            {
                attributes.insert(0, "async".to_string());
            }
            self.finish_subroutine_statement(sub_node)
        } else {
            match kind {
            // Empty statement (lone semicolon) - just consume and return a no-op
            TokenKind::Semicolon => {
                let pos = self.current_position();
                self.consume_token()?;
                // Return an empty block as a no-op placeholder
                return Ok(Node::new(
                    NodeKind::Block { statements: vec![] },
                    SourceLocation { start: pos, end: pos },
                ));
            }

            // Variable declarations (`my $x`, `our @y`, ...) and scoped sub declarations
            // (`my sub helper { ... }`, `our sub helper { ... }`, `state sub memo { ... }`).
            TokenKind::My | TokenKind::Our | TokenKind::State => {
                if matches!(self.tokens.peek_second().map(|t| t.kind), Ok(TokenKind::Sub)) {
                    let decl_token = self.consume_token()?;
                    let mut sub_node = self.parse_subroutine()?;
                    sub_node.location.start = decl_token.start;
                    // Inject the declarator into the Subroutine node
                    if let NodeKind::Subroutine { declarator, name, .. } = &mut sub_node.kind {
                        *declarator = Some(decl_token.text.to_string());
                        if name.is_none() {
                            self.errors.push(ParseError::syntax(
                                "Expected subroutine name after scoped declarator",
                                decl_token.start,
                            ));
                        }
                    }
                    Ok(sub_node)
                } else {
                    let decl = self.parse_variable_declaration()?;
                    // `my`/`our`/`state` declare only the FIRST variable when the
                    // list is unparenthesized (perlsub: "the list must be placed
                    // in parentheses"). A comma directly following the
                    // declaration is therefore NOT part of it — it starts the
                    // surrounding comma expression (e.g. `my $a, $b, $c = 1;`
                    // deparses as `(my($a), $b, ($c = 1));`), so fold it into the
                    // same statement-level comma/fat-arrow continuation used for
                    // autoquoted keys.
                    if matches!(
                        self.peek_kind(),
                        Some(TokenKind::FatArrow) | Some(TokenKind::Comma)
                    ) {
                        self.finish_expression_from(decl)
                    } else {
                        Ok(self.parse_word_or_expr(decl)?)
                    }
                }
            }
            // `field` is a variable declarator only in Perl 5.38+ class bodies.
            // In legacy code it is commonly a regular identifier (function call,
            // hash key, etc.).  We disambiguate by peeking at the next token:
            // if it starts a variable directly (sigil or sigil-prefixed
            // identifier), treat it as a declaration; otherwise fall through
            // to expression parsing.
            TokenKind::Field if self.is_field_declaration_context() => {
                let decl = self.parse_variable_declaration()?;
                if self.peek_kind() == Some(TokenKind::FatArrow) {
                    let variable = match decl.kind {
                        NodeKind::VariableDeclaration { variable, .. } => *variable,
                        _ => decl,
                    };
                    let call_start = variable.location.start;
                    let mut args = vec![variable];

                    while matches!(self.peek_kind(), Some(TokenKind::Comma) | Some(TokenKind::FatArrow)) {
                        self.consume_token()?;

                        if self.peek_kind() == Some(TokenKind::FatArrow) {
                            self.consume_token()?;
                        }

                        if self.is_at_statement_end() {
                            break;
                        }

                        args.push(self.parse_assignment_or_declaration()?);
                    }

                    let end = args.last().map(|arg| arg.location.end).unwrap_or(call_start);
                    let call = Node::new(
                        NodeKind::FunctionCall { name: "field".to_string(), args },
                        SourceLocation { start: call_start, end },
                    );
                    Ok(self.parse_word_or_expr(call)?)
                } else {
                    Ok(self.parse_word_or_expr(decl)?)
                }
            }
            TokenKind::Local => self.parse_local_statement(),

            // Control flow
            TokenKind::If => self.parse_if_statement(),
            TokenKind::Unless => self.parse_unless_statement(),

            // Orphaned else/elsif — these appear at statement level when the
            // preceding if/unless block failed to parse or was consumed by
            // error recovery. Instead of crashing into expression parsing,
            // consume the else/elsif clause gracefully and wrap it in an
            // error-recovery If node so the rest of the file can keep parsing.
            TokenKind::Else => self.parse_orphaned_else(),
            TokenKind::Elsif => self.parse_orphaned_elsif(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Until => self.parse_until_statement(),
            TokenKind::For => self.parse_for_statement(),
            TokenKind::Foreach => self.parse_foreach_statement(),
            TokenKind::Given => self.parse_given_statement(),
            TokenKind::Default => self.parse_default_statement(),
            // `try` can be a user-defined subroutine name. Only the block form
            // is the try/catch construct; `try(...)` is an ordinary call.
            TokenKind::Try
                if self
                    .tokens
                    .peek_second()
                    .ok()
                    .is_some_and(|token| token.kind == TokenKind::LeftBrace) => self.parse_try(),
                TokenKind::Defer => self.parse_defer(),

            // Loop control — next/last/redo can be followed by a word operator at statement level,
            // e.g. `last and die` means `(last) and (die)`.
            TokenKind::Next | TokenKind::Last | TokenKind::Redo => {
                let ctrl = self.parse_loop_control()?;
                Ok(self.parse_word_or_expr(ctrl)?)
            }

            // Subroutines and modern OOP
            TokenKind::Sub => {
                let sub_node = self.parse_subroutine()?;
                self.finish_subroutine_statement(sub_node)
            }
            TokenKind::Class
                if matches!(
                    self.tokens.peek_second().map(|t| t.kind),
                    Ok(TokenKind::Identifier)
                        | Ok(TokenKind::DoubleColon)
                        | Ok(TokenKind::Colon)
                ) =>
            {
                self.parse_class()
            }
            // `method NAME SIGNATURE BLOCK` is a Perl 5.38+ declaration.
            // Legacy code uses `method` as a function name; disambiguate by
            // checking the next token is an Identifier (the method name).
            TokenKind::Method
                if matches!(
                    self.tokens.peek_second().map(|t| t.kind),
                    Ok(TokenKind::Identifier)
                ) =>
            {
                self.parse_method()
            }

            // Package management
            TokenKind::Package => self.parse_package(),
            TokenKind::Use => self.parse_use(),
            TokenKind::No => self.parse_no(),

            // Format declarations
            TokenKind::Format => self.parse_format(),

            // Phase blocks — but first check for label syntax (CHECK: ..., BEGIN: ..., etc.)
            // In Perl, phase-block keywords are valid statement labels when followed by `:`.
            // e.g. `CHECK: for (my $i = 0; ...)` uses CHECK as a loop label, not a phase block.
            TokenKind::Begin
            | TokenKind::End
            | TokenKind::Check
            | TokenKind::Init
            | TokenKind::Unitcheck
                if self
                    .tokens
                    .peek_second()
                    .ok()
                    .map(|t| t.kind == TokenKind::Colon)
                    .unwrap_or(false) =>
            {
                self.parse_keyword_as_label()
            }
            TokenKind::Begin
            | TokenKind::End
            | TokenKind::Check
            | TokenKind::Init
            | TokenKind::Unitcheck
                if self
                    .tokens
                    .peek_second()
                    .ok()
                    .map(|t| t.kind == TokenKind::LeftBrace)
                    .unwrap_or(false) =>
            {
                self.parse_phase_block()
            }

            // Phase keywords can also be used as barewords/sub names in normal
            // statement position (e.g. `CHECK();` from CPAN code).  If there is
            // no `{` after the keyword, parse as a regular expression statement
            // instead of forcing phase-block syntax.
            TokenKind::Begin
            | TokenKind::End
            | TokenKind::Check
            | TokenKind::Init
            | TokenKind::Unitcheck => self.parse_expression_statement(),

            TokenKind::Return
                if self
                    .tokens
                    .peek_second()
                    .ok()
                    .map(|t| t.kind == TokenKind::Colon)
                    .unwrap_or(false) =>
            {
                self.parse_keyword_as_label()
            }

            // Data sections
            TokenKind::DataMarker => self.parse_data_section(),

            // Return statement — may be followed by a word operator at statement level,
            // e.g. `return or die` means `(return) or (die)`.
            TokenKind::Return => {
                let ret = self.parse_return()?;
                Ok(self.parse_word_or_expr(ret)?)
            }

            // Goto statement
            TokenKind::Goto => self.parse_goto(),

            // Block — or hashref/block constructor followed by arrow dereference
            // e.g. {key => "value"}->{key}
            TokenKind::LeftBrace => {
                let block = self.parse_block()?;
                if self.peek_kind() == Some(TokenKind::Arrow) {
                    // The block is actually an expression (hash constructor)
                    // followed by postfix arrow operators.
                    let chained = self.parse_postfix_chain(block)?;
                    let loc = chained.location;
                    Ok(Node::new(
                        NodeKind::ExpressionStatement { expression: Box::new(chained) },
                        loc,
                    ))
                } else {
                    Ok(block)
                }
            }

            // Expression-ish statement
            _ => {
                // Check if this might be a labeled statement
                if self.is_label_start() {
                    return self.parse_labeled_statement();
                }

                // Either build via indirect-object path or the normal expression path
                if let TokenKind::Identifier = kind {
                    // We need the text for the indirect call check and the route trace.
                    // We must copy because is_indirect_call_pattern borrows self mutably to peek ahead.
                    // The span feeds the test-only decision trace and is unused otherwise.
                    #[cfg_attr(not(test), allow(unused_variables))]
                    let (text, token_start, token_end) = {
                        let token = self.tokens.peek()?;
                        (token.text.clone(), token.start, token.end)
                    };
                    if self.is_unknown_lowercase_bareword_call_pattern(&text) {
                        // The predicate stays the sole dispatch authority. The test-only
                        // mutation control suppresses route evidence without moving any
                        // source shape onto a different route, so the public AST is
                        // preserved for every input while the positive proof fails.
                        #[cfg(test)]
                        if !self.unknown_lowercase_bareword_decision_is_bypassed() {
                            self.record_unknown_lowercase_bareword_call_decision(
                                token_start,
                                token_end,
                            );
                        }
                        let call = self.parse_unknown_lowercase_bareword_call()?;
                        Ok(self.parse_named_unary_statement_tail(call)?)
                    } else if self.is_indirect_call_pattern(&text) {
                        // Parse indirect call but DON'T return early - let it go through
                        // the same modifier/semicolon handling as other statements.
                        // Short-circuit operators may follow: `print $fh "msg" or die`,
                        // `close FH || croak`.
                        let call = self.parse_indirect_call()?;
                        Ok(self.parse_named_unary_statement_tail(call)?)
                    } else {
                        self.parse_expression_statement()
                    }
                } else {
                    self.parse_expression_statement()
                }
            }
            }
        }?;

        // Check for statement modifiers — only on non-compound statements.
        // Compound statements (if/while/for/foreach/given/default/try/sub/package)
        // cannot take postfix modifiers; the keyword that follows is a new statement.
        if !Self::is_compound_statement(&stmt)
            && matches!(self.peek_kind(), Some(k) if Self::is_stmt_modifier_kind(k))
        {
            stmt = self.parse_statement_modifier(stmt)?;
        }

        self.finish_statement_terminator(&stmt)?;

        // Drain pending heredocs after statement completion (attach content to AST)
        self.drain_pending_heredocs_from(pending_heredoc_start, &mut stmt);

        Ok(stmt)
    }

    /// Consume the statement's terminating `;`, or record that it was missing.
    ///
    /// Perl requires `;` between statements. It is omissible in exactly two
    /// places: before the closing `}` of a block, and at end of file. A
    /// compound statement (`if`, `while`, `sub`, a bare block, …) is terminated
    /// by its own closing brace and never needs one.
    ///
    /// Treating the `;` as unconditionally optional — as this did — made the
    /// parser accept `my $x = 1` followed by `print "hi";` and report a clean
    /// parse, so `--check` answered `ok` for source real `perl` rejects with a
    /// syntax error. A missing statement terminator is the most common Perl
    /// syntax error, so that false pass was the likeliest first-contact
    /// failure (#5474).
    ///
    /// The missing `;` is *inferred* rather than fatal: the statement already
    /// parsed, and the next one parses on its own, so recording a recovery and
    /// continuing produces a usable tree plus an honest diagnostic. Consuming
    /// nothing here leaves the next token for the statement loop.
    ///
    /// Deliberately does not use `peek_fresh_kind()`, which misbehaves with
    /// nested blocks.
    fn finish_statement_terminator(&mut self, stmt: &Node) -> ParseResult<()> {
        if self.peek_kind() == Some(TokenKind::Semicolon) {
            if self.pending_heredocs.is_empty()
                && !Self::contains_heredoc(stmt)
                && Self::can_arm_heredoc_recovery(stmt)
            {
                if let Some(tag) = self.statement_span_heredoc_tag(stmt) {
                    self.heredoc_recovery_tag = Some(tag);
                }
            }
            let semi_token = self.consume_token()?;
            // Track cursor after semicolon for heredoc content collection
            if self.pending_heredocs.is_empty() {
                self.byte_cursor = semi_token.end;
            }
            return Ok(());
        }

        if Self::is_brace_terminated_statement(stmt) {
            return Ok(());
        }

        // `None` is end of token stream; `RightBrace` closes the enclosing
        // block; `DataMarker` is `__END__`/`__DATA__`, which ends the program
        // text exactly like EOF — `1\n__END__\n\n=head1 …` is the idiomatic
        // module ending and real `perl -c` accepts it. `UnknownRest` means the
        // lexer hit its budget and stopped, so the statement's terminator is
        // unknowable rather than absent; blaming the user for it would be
        // wrong for the same reason.
        if matches!(
            self.peek_kind(),
            None | Some(
                TokenKind::Eof
                    | TokenKind::RightBrace
                    | TokenKind::DataMarker
                    | TokenKind::UnknownRest
            )
        ) {
            return Ok(());
        }

        // A token that cannot begin a statement means the parser stopped in the
        // middle of one, not that the user omitted a terminator.
        //
        // `File/Copy.pm:175` is the measured case: `copy($from, $to)` and its
        // continuation `or goto fail_inner;` are one statement wrapped across a
        // newline, and no Perl statement begins with `or`. Crossing a line
        // boundary is necessary but not sufficient — this is what makes it
        // sufficient (found in review, #5503).
        if Self::cannot_begin_a_statement(self.peek_kind()) {
            return Ok(());
        }

        // `use`/`no` import lists are not fully modelled — `no warnings qw(…)`,
        // multi-line `use overload …` (`autodie/exception.pm:17`) and
        // `use constant NAME => …` all stop early today. The seam cannot tell
        // those from a real missing `;`, so it does not police pragmas.
        if matches!(stmt.kind, NodeKind::Use { .. } | NodeKind::No { .. }) {
            return Ok(());
        }

        // Heredoc bodies are collected out of band, so the parser's position is
        // not trustworthy for a statement that declared one. The AST check alone
        // is not enough: when the introducer follows an unknown bareword
        // (`_sprintf562 <<'CODE'`, `ExtUtils/MM_Any.pm:1779`) the lexer emits a
        // left shift, so no `Heredoc` node exists even though the body lines are
        // still in the token stream. Scanning the statement's own source span
        // catches that case too.
        if self.pending_heredocs.is_empty()
            && !Self::contains_heredoc(stmt)
            && Self::can_arm_heredoc_recovery(stmt)
        {
            if let Some(tag) = self.statement_span_heredoc_tag(stmt) {
                self.heredoc_recovery_tag = Some(tag);
                return Ok(());
            }
        }

        // An unrecognised heredoc may leak its body and terminator into the
        // token stream. Exempt only the exact delimiter line, not every lone
        // identifier: `foo` followed by `print` is a real missing terminator.
        if Self::is_bare_identifier_statement(stmt) {
            let matches_recovery_tag = self.heredoc_recovery_tag.as_deref().is_some_and(|tag| {
                let start = stmt.location.start.min(self.src_bytes.len());
                let end = stmt.location.end.min(self.src_bytes.len());
                std::str::from_utf8(&self.src_bytes[start..end])
                    .map(|text| text.trim() == tag)
                    .unwrap_or(false)
            });
            if matches_recovery_tag {
                self.heredoc_recovery_tag = None;
                return Ok(());
            }
        }

        // Only report when the leftover token begins a later line.
        //
        // Reaching here mid-line means the statement stopped short of its own
        // end — the parser did not consume a construct it should have. Three
        // such gaps exist in this repository's own corpus today (`no warnings
        // qw(...)`, the `x=` repetition-assignment operator, and `method NAME
        // {...}` in a class body), and reporting a missing `;` for them would
        // reject valid Perl to describe a defect that is not the user's.
        //
        // A statement terminator the *user* omitted separates two statements
        // written on different lines, which is also the only shape `perl`
        // itself reports this way. Staying inside that shape trades some
        // false negatives — `my $x = 1 print "hi";` on one line is missed —
        // for no false positives, which is the correct direction for a check
        // that gates a release.
        if !self.line_break_precedes_current_token() {
            return Ok(());
        }

        let location = self.current_position();
        self.errors.push(ParseError::Recovered {
            site: RecoverySite::Statement,
            kind: RecoveryKind::InferredSemicolon,
            location,
        });
        Ok(())
    }

    /// Whether only whitespace containing at least one newline separates the
    /// previous token from the one the parser is positioned on.
    ///
    /// Scans the raw source backwards rather than trusting
    /// `previous_position()`. `last_end_position` is only updated by
    /// `consume_token()`/`expect()`, and several expression paths take tokens
    /// straight off the stream, so it lags behind the real end of the statement
    /// — on `my $x = 1` it sits at the end of `$x`, not of `1`. A window keyed
    /// on it is wider than the actual gap and can contain a newline that is not
    /// between the statement and the leftover token (found in review, #5503).
    fn line_break_precedes_current_token(&mut self) -> bool {
        let start = self.current_position().min(self.src_bytes.len());
        self.src_bytes[..start]
            .iter()
            .rev()
            .take_while(|byte| byte.is_ascii_whitespace())
            .any(|&byte| byte == b'\n')
    }

    /// Whether this token can never be the first token of a Perl statement.
    ///
    /// Infix, postfix and separator tokens all mean the same thing at the
    /// terminator seam: the parser stopped inside an expression. Reporting a
    /// missing `;` there blames the user for our gap.
    fn cannot_begin_a_statement(kind: Option<TokenKind>) -> bool {
        matches!(
            kind,
            Some(
                TokenKind::WordAnd
                    | TokenKind::WordOr
                    | TokenKind::WordXor
                    | TokenKind::And
                    | TokenKind::Or
                    | TokenKind::DefinedOr
                    | TokenKind::Assign
                    | TokenKind::Plus
                    // `Minus` is here for the same reason as `Plus`, and with
                    // the same trade: `1\n-$y;` is one subtraction to `perl`,
                    // not two statements, so a leading `-` is a continuation
                    // even though unary minus could in principle start a
                    // statement. Expression parsing already consumes both
                    // today, so neither costs measured coverage.
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Percent
                    | TokenKind::Power
                    | TokenKind::LeftShift
                    | TokenKind::RightShift
                    | TokenKind::BitwiseAnd
                    | TokenKind::BitwiseOr
                    | TokenKind::BitwiseXor
                    | TokenKind::PlusAssign
                    | TokenKind::MinusAssign
                    | TokenKind::StarAssign
                    | TokenKind::SlashAssign
                    | TokenKind::PercentAssign
                    | TokenKind::DotAssign
                    | TokenKind::AndAssign
                    | TokenKind::OrAssign
                    | TokenKind::XorAssign
                    | TokenKind::PowerAssign
                    | TokenKind::LeftShiftAssign
                    | TokenKind::RightShiftAssign
                    | TokenKind::LogicalAndAssign
                    | TokenKind::LogicalOrAssign
                    | TokenKind::DefinedOrAssign
                    | TokenKind::Equal
                    | TokenKind::NotEqual
                    | TokenKind::Match
                    | TokenKind::NotMatch
                    | TokenKind::SmartMatch
                    | TokenKind::Less
                    | TokenKind::Greater
                    | TokenKind::LessEqual
                    | TokenKind::GreaterEqual
                    | TokenKind::Spaceship
                    | TokenKind::StringCompare
                    | TokenKind::Arrow
                    | TokenKind::FatArrow
                    | TokenKind::Dot
                    | TokenKind::Range
                    | TokenKind::DoubleColon
                    | TokenKind::Question
                    | TokenKind::Colon
                    | TokenKind::Comma
                    | TokenKind::Semicolon
                    | TokenKind::RightParen
                    | TokenKind::RightBracket
            )
        )
    }

    /// Whether the statement is a single bare identifier.
    fn is_bare_identifier_statement(node: &Node) -> bool {
        match &node.kind {
            NodeKind::Identifier { .. } => true,
            NodeKind::ExpressionStatement { expression } => {
                matches!(expression.kind, NodeKind::Identifier { .. })
            }
            _ => false,
        }
    }

    /// Whether the subtree declares a heredoc.
    fn contains_heredoc(node: &Node) -> bool {
        matches!(node.kind, NodeKind::Heredoc { .. })
            || node.children().into_iter().any(Self::contains_heredoc)
    }

    /// Ordinary shift expressions can contain a quoted word immediately after
    /// `<<`, but that word is not a heredoc delimiter. Keep them out of the
    /// narrow leaked-heredoc recovery path.
    fn contains_left_shift(node: &Node) -> bool {
        matches!(&node.kind, NodeKind::Binary { op, .. } if op == "<<" || op == "<<=")
            || node.children().into_iter().any(Self::contains_left_shift)
    }

    fn contains_identifier_left_shift(node: &Node) -> bool {
        matches!(
            &node.kind,
            NodeKind::Binary { op, left, .. }
                if op == "<<" && matches!(left.kind, NodeKind::Identifier { .. })
        ) || node.children().into_iter().any(Self::contains_identifier_left_shift)
    }

    fn can_arm_heredoc_recovery(node: &Node) -> bool {
        !Self::contains_left_shift(node) || Self::contains_identifier_left_shift(node)
    }

    /// Whether the slash at `index` starts a `qr//` quote-like body.
    ///
    /// A bare slash is also Perl's division operator, so treating every slash
    /// as a quote delimiter would hide real heredoc introducers. Restrict this
    /// guard to the unambiguous `qr` operator form used by the parser's regex
    /// syntax and by the false-negative regression below.
    fn starts_qr_slash_body(span: &[u8], index: usize) -> bool {
        if span.get(index) != Some(&b'/') {
            return false;
        }
        let mut operator_end = index;
        while operator_end > 0
            && matches!(span[operator_end - 1], b' ' | b'\t' | b'\r' | b'\n')
        {
            operator_end -= 1;
        }
        operator_end >= 2 && span.get(operator_end - 2..operator_end) == Some(&b"qr"[..])
    }

    /// Find an unrecognised heredoc introducer in the source the statement spans
    /// (`<<"X"`, `<<'X'`, `<<X`, `<<~X`) **in code position**.
    ///
    /// Two things are deliberately not matched, because matching them would
    /// suppress a real diagnostic:
    ///
    /// - a bare `<<` used as left shift (`1 << 2`), which is followed by
    ///   whitespace or a digit;
    /// - `<<` inside a string literal, a comment, or a regex/quote-like body —
    ///   `my $s = "<<EOF"` is a string containing two angle brackets, not a
    ///   heredoc, and treating it as one made `--check` answer `ok` for the
    ///   statement after it (found in review, #5503).
    ///
    /// The quote tracking is intentionally shallow: it follows ordinary
    /// literals, comments, and Perl's quote-like operators. It is a heuristic
    /// guarding a heuristic, and it errs toward *reporting* — an unmatched
    /// quote resolves at end of span, so a real introducer is never hidden
    /// behind one.
    fn statement_span_heredoc_tag(&mut self, stmt: &Node) -> Option<String> {
        let end = self.current_position().min(self.src_bytes.len());
        let start = stmt.location.start.min(end);
        let span = &self.src_bytes[start..end];

        let mut quote: Option<u8> = None;
        let mut index = 0;
        while index < span.len() {
            let byte = span[index];

            if let Some(delimiter) = quote {
                // `\x` inside a literal escapes whatever follows, including the
                // closing delimiter.
                if byte == b'\\' && delimiter != b'#' {
                    index += 2;
                    continue;
                }
                if byte == delimiter || (delimiter == b'#' && byte == b'\n') {
                    quote = None;
                }
                index += 1;
                continue;
            }

            match byte {
                b'\'' | b'"' | b'`' | b'#' => {
                    quote = Some(byte);
                    index += 1;
                    continue;
                }
                b'<' if span.get(index + 1) == Some(&b'<') => {
                    let mut rest = span[index + 2..].iter();
                    let next = match rest.next() {
                        Some(b'~') => rest.next(),
                        other => other,
                    };
                    if matches!(next, Some(b'"' | b'\'' | b'`' | b'A'..=b'Z' | b'a'..=b'z' | b'_'))
                    {
                        let mut tag = &span[index + 2..];
                        if tag.first() == Some(&b'~') {
                            tag = &tag[1..];
                        }
                        let tag = match tag.first() {
                            Some(b'\'' | b'"' | b'`') => {
                                let delimiter = tag[0];
                                let end = tag[1..]
                                    .iter()
                                    .position(|&candidate| candidate == delimiter)?
                                    + 1;
                                &tag[1..end]
                            }
                            Some(b'A'..=b'Z' | b'a'..=b'z' | b'_') => {
                                let end = tag
                                    .iter()
                                    .position(|candidate| {
                                        !matches!(
                                            candidate,
                                            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'
                                        )
                                    })
                                    .unwrap_or(tag.len());
                                &tag[..end]
                            }
                            _ => {
                                index += 2;
                                continue;
                            }
                        };
                        return std::str::from_utf8(tag).ok().map(str::to_owned);
                    }
                    index += 2;
                    continue;
                }
                _ => {
                    if let Some(end) = Self::quote_like_body_end(span, index) {
                        index = end;
                        continue;
                    }
                    index += 1;
                }
            }
        }

        None
    }

    /// Return the byte after a quote-like expression beginning at `index`.
    ///
    /// This is deliberately a source scanner rather than a parser-level
    /// expression check: its only job is to keep `<<TAG` inside quote-like
    /// bodies from being mistaken for a heredoc introducer. Paired delimiters
    /// are balanced, escapes are skipped, and substitution-like operators
    /// consume both bodies.
    #[expect(
        clippy::question_mark,
        reason = "policy:ripr-quote-like-body: intentional let-else return None so RIPR None-oracles observe the miss path (#5838)"
    )]
    fn quote_like_body_end(span: &[u8], index: usize) -> Option<usize> {
        const OPERATORS: &[&[u8]] = &[b"tr", b"qq", b"qx", b"qr", b"qw", b"m", b"s", b"y", b"q"];

        if index > 0 && matches!(span[index - 1], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'$') {
            return None;
        }

        // Prefer an explicit miss-return over `find(...)?`. RIPR classifies the
        // Option-`?` form as an error_path sink that existing `None` oracles do
        // not observe; `return None` is the same control flow and is already
        // covered by the non-operator prefix discriminators below.
        let Some(operator) = OPERATORS.iter().find(|operator| {
            span.get(index..index + operator.len()) == Some(**operator)
        }) else {
            return None;
        };
        let mut delimiter_index = index + operator.len();
        while matches!(span.get(delimiter_index), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            delimiter_index += 1;
        }
        let delimiter = *span.get(delimiter_index)?;
        if delimiter.is_ascii_alphanumeric() || delimiter == b'_' || delimiter == b'$' {
            return None;
        }
        if *operator == b"qr"
            && delimiter == b'/'
            && !Self::starts_qr_slash_body(span, delimiter_index)
        {
            return None;
        }

        let parts = if matches!(*operator, b"s" | b"tr" | b"y") { 2 } else { 1 };
        let first_delimiter = span[delimiter_index];
        let paired = matches!(first_delimiter, b'(' | b'[' | b'{' | b'<');
        let mut cursor = delimiter_index;
        for part in 0..parts {
            cursor = if part == 0 || paired {
                Self::quote_like_part_end(span, cursor)?
            } else {
                Self::quote_like_unpaired_end(span, cursor, first_delimiter)?
            };
            if part + 1 < parts {
                while matches!(span.get(cursor), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                    cursor += 1;
                }
            }
        }
        Some(cursor)
    }

    fn quote_like_unpaired_end(span: &[u8], start: usize, delimiter: u8) -> Option<usize> {
        let mut index = start;
        while index < span.len() {
            match span[index] {
                b'\\' => index = index.saturating_add(2),
                byte if byte == delimiter => return Some(index + 1),
                _ => index += 1,
            }
        }
        None
    }

    fn quote_like_part_end(span: &[u8], delimiter_index: usize) -> Option<usize> {
        let opener = *span.get(delimiter_index)?;
        let closer = match opener {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            delimiter => delimiter,
        };
        let paired = opener != closer;
        let mut depth = 0usize;
        let mut index = delimiter_index + 1;
        while index < span.len() {
            match span[index] {
                b'\\' => index = index.saturating_add(2),
                byte if paired && byte == opener => {
                    depth = depth.saturating_add(1);
                    index += 1;
                }
                byte if byte == closer => {
                    if depth == 0 {
                        return Some(index + 1);
                    }
                    depth -= 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }
        None
    }

    /// Mark that we're no longer at statement start (called after consuming statement head)
    fn mark_not_stmt_start(&mut self) {
        self.at_stmt_start = false;
    }

    /// Check if current token is a statement modifier keyword
    fn is_statement_modifier_keyword(&mut self) -> bool {
        matches!(self.peek_kind(), Some(k) if Self::is_stmt_modifier_kind(k))
    }

    /// Whether this statement ends with its own closing brace, and so needs no
    /// `;`.
    ///
    /// Narrower than [`Self::is_compound_statement`] in exactly one place:
    /// `package NAME;` and `package NAME VERSION;` are ordinary statements that
    /// require a terminator, and only `package NAME { … }` is brace-terminated.
    /// The two predicates are not the same question — a `package` statement
    /// cannot take a postfix modifier in either form, which is what the
    /// compound-statement check is for, but only the block form ends itself.
    /// Sharing one predicate left `package Foo` followed by another statement
    /// reported as `ok` while `perl -c` rejects it (found in review, #5503).
    fn is_brace_terminated_statement(node: &Node) -> bool {
        match &node.kind {
            NodeKind::Package { block, .. } => block.is_some(),
            // Forward `sub foo;` declarations are represented with a synthetic
            // zero-width empty block at the semicolon. A real `{}` body spans
            // the braces even when it has no statements.
            NodeKind::Subroutine { body, .. }
            | NodeKind::Class { body, .. }
            | NodeKind::Method { body, .. } => body.location.start != body.location.end,
            _ => Self::is_compound_statement(node),
        }
    }

    /// Returns true if the node is a compound statement that cannot take a postfix modifier.
    /// In Perl a compound statement is terminated by its own closing brace, so a modifier
    /// keyword that follows one begins a new top-level statement rather than modifying it.
    ///
    /// The full set this accepts: `if`, `while`, `for`, `foreach`, `given`, `default`,
    /// `try`, `defer`, `sub`, `package`, `class`, `method`, `format`, a bare block, and a
    /// phase block (`BEGIN`/`END`/`CHECK`/`INIT`/`UNITCHECK`). Keep this list and the
    /// `matches!` arm below in step — a summary that lags the arm is how a reader ends up
    /// reasoning about a predicate that does something else (#5503).
    ///
    /// A bare `Block` is also compound: `{ ... } for @arr` is a syntax error in Perl
    /// (verified: `perl -c` rejects it). Without this, `{ ... }\nfor my $x (...) { }`
    /// would be misread as `{ ... } for my` (postfix-for with `my` as the iterator
    /// expression), causing the `for my $x (LIST) { BLOCK }` form to fail.
    ///
    /// Not the same question as [`Self::is_brace_terminated_statement`]: this one
    /// answers "can a postfix modifier follow", which is false for `package` in
    /// both its statement and block forms.
    fn is_compound_statement(node: &Node) -> bool {
        matches!(
            node.kind,
            NodeKind::If { .. }
                | NodeKind::While { .. }
                | NodeKind::For { .. }
                | NodeKind::Foreach { .. }
                | NodeKind::Given { .. }
                | NodeKind::Default { .. }
                | NodeKind::Try { .. }
                | NodeKind::Defer { .. }
                | NodeKind::Subroutine { .. }
                // `class NAME { … }` and `method NAME { … }` (Perl 5.38) are
                // brace-terminated declarations exactly like `sub`. Their
                // absence here was invisible while the terminator was optional.
                | NodeKind::Class { .. }
                | NodeKind::Method { .. }
                | NodeKind::Package { .. }
                | NodeKind::Format { .. }
                | NodeKind::Block { .. }
                | NodeKind::PhaseBlock { .. }
        )
    }

    /// Parse expression statement
    /// Resume comma-level expression parsing from an already-consumed first
    /// token (used when a keyword has been autoquoted before `=>`).
    ///
    /// Produces an `ExpressionStatement` wrapping the resulting list / hash
    /// expression.
    fn finish_expression_from(&mut self, first: Node) -> ParseResult<Node> {
        let start = first.location.start;
        let mut expr = self.collect_comma_fat_arrow_continuation(first)?;

        // Handle trailing word operators (or, and, xor)
        expr = self.parse_word_or_expr(expr)?;

        let end = self.previous_position();
        Ok(Node::new(
            NodeKind::ExpressionStatement { expression: Box::new(expr) },
            SourceLocation { start, end },
        ))
    }

    fn parse_expression_statement(&mut self) -> ParseResult<Node> {
        let start = self.current_position();

        // Check for special blocks like AUTOLOAD and DESTROY
        if let Ok(token) = self.tokens.peek() {
            if matches!(token.text.as_ref(), "AUTOLOAD" | "DESTROY" | "CLONE" | "CLONE_SKIP") {
                // Check if next token is a block
                if let Ok(second) = self.tokens.peek_second() {
                    if second.kind == TokenKind::LeftBrace {
                        return self.parse_special_block();
                    }
                }
            }
        }

        // First, try to parse the initial part as a simple statement
        let mut expr = self.parse_simple_statement()?;

        // Some statement-start calls parse their argument list before returning
        // here, but symbolic logical operators still belong to the surrounding
        // expression: `print(...) || die`, `system(...) && die`.
        if self.peek_kind() == Some(TokenKind::And) || Self::is_logical_or(self.peek_kind()) {
            expr = self.parse_named_unary_statement_tail(expr)?;
        } else {
            // Check for word operators (or, and, xor) which have very low precedence.
            expr = self.parse_word_or_expr(expr)?;
        }

        // Statement modifiers are handled at the statement level in parse_statement()

        // Prefer the later of expression end and the last consumed token so
        // wrappers such as `(42)` keep their closing delimiter in the span.
        let end = expr.location.end.max(self.previous_position());

        // Wrap the expression in an ExpressionStatement node
        Ok(Node::new(
            NodeKind::ExpressionStatement { expression: Box::new(expr) },
            SourceLocation { start, end },
        ))
    }

    /// Continue parsing operators after a no-arg named-unary/nullary call.
    fn parse_call_statement_tail(&mut self, mut expr: Node) -> ParseResult<Node> {
        expr = self.parse_power_with(expr)?;
        expr = self.parse_multiplicative_with(expr)?;
        expr = self.parse_additive_with(expr)?;
        expr = self.parse_shift_with(expr)?;
        self.parse_named_unary_statement_tail(expr)
    }

    /// Continue parsing operators that bind less tightly than named-unary/list
    /// operator arguments after we have already constructed the call node.
    fn parse_named_unary_statement_tail(&mut self, mut expr: Node) -> ParseResult<Node> {
        expr = self.parse_relational_with(expr)?;
        expr = self.parse_equality_with(expr)?;
        expr = self.parse_range_with(expr)?;
        expr = self.parse_bitwise_and_with(expr)?;
        expr = self.parse_bitwise_xor_with(expr)?;
        expr = self.parse_bitwise_or_with(expr)?;
        expr = self.parse_and_with(expr)?;
        expr = self.parse_or_with(expr)?;
        expr = self.parse_ternary_with(expr)?;
        expr = self.collect_comma_fat_arrow_continuation(expr)?;
        self.parse_word_or_expr(expr)
    }

    fn parse_named_unary_statement_call(
        &mut self,
        start: usize,
        func_name: &str,
        allow_no_args: bool,
    ) -> ParseResult<Node> {
        let binary_operator_starts_missing_arg = self.peek_kind().is_some_and(Self::is_binary_operator)
            && !(Self::is_optional_arg_builtin(func_name)
                && self.is_explicit_sub_sigil_argument_start());
        let omit_optional_arg = allow_no_args
            && (binary_operator_starts_missing_arg
                || self.peek_kind() == Some(TokenKind::Slash)
                // Nullary/named-unary builtins at statement start may be
                // followed by a comma operator:
                //   shift, return ...
                // In that form `shift` has no explicit argument; the comma
                // belongs to the surrounding expression list.
                || self.peek_kind() == Some(TokenKind::Comma));

        // String comparison operators (ne, eq, lt, le, gt, ge) are tokenized as
        // Identifier tokens, so `is_binary_operator` won't catch them. When a
        // nullary builtin like `ref` is followed by one of these, don't consume
        // the operator as an argument -- let it become a binary operator instead.
        let next_is_str_cmp_op = self.peek_kind() == Some(TokenKind::Identifier)
            && self.tokens.peek().is_ok_and(|t| {
                matches!(t.text.as_ref(), "eq" | "ne" | "lt" | "le" | "gt" | "ge")
            });

        let args = if self.is_at_statement_end() || omit_optional_arg || next_is_str_cmp_op {
            vec![]
        } else {
            vec![self.parse_shift()?]
        };

        if args.is_empty() && !allow_no_args && !next_is_str_cmp_op {
            return Err(ParseError::unexpected(
                "expression".to_string(),
                format!("{:?}", self.peek_kind()),
                self.current_position(),
            ));
        }

        let had_args = !args.is_empty();
        let end = args
            .last()
            .map(|arg| arg.location.end)
            .unwrap_or_else(|| self.previous_position());
        let mut expr = Node::new(
            NodeKind::FunctionCall {
                name: func_name.to_string(),
                args,
            },
            SourceLocation { start, end },
        );

        // `pos` is an lvalue-capable builtin in Perl: `pos $s = value` and
        // `pos = value` assign to the current regex-match position.  After
        // building the call node, check for an assignment operator so the
        // result is `Assignment { lhs: pos($s), rhs: value }` rather than
        // leaving `= value` as an unparsed token sequence.
        if func_name == "pos" {
            let assign_op = match self.peek_kind() {
                Some(TokenKind::Assign) => Some("="),
                Some(TokenKind::PlusAssign) => Some("+="),
                Some(TokenKind::MinusAssign) => Some("-="),
                Some(TokenKind::StarAssign) => Some("*="),
                Some(TokenKind::SlashAssign) => Some("/="),
                Some(TokenKind::PercentAssign) => Some("%="),
                Some(TokenKind::DotAssign) => Some(".="),
                Some(TokenKind::AndAssign) => Some("&="),
                Some(TokenKind::OrAssign) => Some("|="),
                Some(TokenKind::XorAssign) => Some("^="),
                Some(TokenKind::PowerAssign) => Some("**="),
                Some(TokenKind::LeftShiftAssign) => Some("<<="),
                Some(TokenKind::RightShiftAssign) => Some(">>="),
                Some(TokenKind::LogicalAndAssign) => Some("&&="),
                Some(TokenKind::LogicalOrAssign) => Some("||="),
                Some(TokenKind::DefinedOrAssign) => Some("//="),
                _ => None,
            };
            if let Some(op) = assign_op {
                self.tokens.next()?; // consume the assignment operator
                let rhs = self.parse_assignment()?;
                let assign_end = rhs.location.end;
                expr = Node::new(
                    NodeKind::Assignment {
                        lhs: Box::new(expr),
                        rhs: Box::new(rhs),
                        op: op.to_string(),
                    },
                    SourceLocation { start, end: assign_end },
                );
                return self.parse_named_unary_statement_tail(expr);
            }
        }

        if had_args {
            self.parse_named_unary_statement_tail(expr)
        } else {
            self.parse_call_statement_tail(expr)
        }
    }

    /// Parse simple statement (print, die, next, last, etc. with their arguments)
    fn parse_simple_statement(&mut self) -> ParseResult<Node> {
        // In Perl, any bareword before `=>` is autoquoted as a hash key.
        // When a builtin name (e.g. `log`, `abs`, `die`) appears before `=>`,
        // skip the builtin dispatch and fall through to expression parsing.
        // This handles patterns like `has log => sub { ... }`.
        if self.is_keyword_before_fat_arrow() {
            return self.parse_expression();
        }
        // Check if it's a builtin that can take arguments without parens
        if let Ok(token) = self.tokens.peek() {
            let token_text = token.text.clone();
            let token_start = token.start;

            match token_text.as_ref() {
                // Parenthesized nullary builtins are handled by the general
                // expression parser; this branch is for statement-start
                // bare calls like `shift @arr` or `caller 1 || die`.
                name if Self::is_nullary_builtin(name) => {
                    if self.tokens.peek_second().is_ok_and(|t| {
                        matches!(t.kind, TokenKind::LeftParen | TokenKind::Arrow)
                    }) {
                        self.parse_expression()
                    } else {
                        let token = self.consume_token()?;
                        self.mark_not_stmt_start();
                        self.parse_named_unary_statement_call(token_start, token.text.as_ref(), true)
                    }
                }
                // Special-cased builtins with dedicated AST nodes — must come
                // before the generic `is_builtin_function` guard below.
                "tie" => {
                    let start = token_start;
                    self.consume_token()?; // consume tie
                    self.mark_not_stmt_start();

                    // `tie(VARIABLE, CLASS, LIST)` is valid Perl syntax.
                    let has_parens = self.peek_kind() == Some(TokenKind::LeftParen);
                    if has_parens {
                        self.consume_token()?; // consume '('
                    }

                    // First argument to tie can be a variable declaration, e.g. tie my %hash, ...
                    let variable = if matches!(self.peek_kind(), Some(TokenKind::My | TokenKind::Our | TokenKind::Local | TokenKind::State)) {
                        Box::new(self.parse_variable_declaration()?)
                    } else {
                        Box::new(self.parse_assignment()?)
                    };

                    // Accept comma or fat arrow between variable and package
                    // (Perl treats `=>` as a synonym for `,`)
                    match self.peek_kind() {
                        Some(TokenKind::Comma) | Some(TokenKind::FatArrow) => {
                            self.consume_token()?;
                        }
                        _ => {
                            return Err(ParseError::unexpected(
                                "Comma".to_string(),
                                format!("{:?}", self.peek_kind()),
                                self.current_position(),
                            ));
                        }
                    }
                    let package = Box::new(self.parse_assignment()?);

                    let mut args = vec![];
                    while matches!(self.peek_kind(), Some(TokenKind::Comma) | Some(TokenKind::FatArrow)) {
                        self.consume_token()?; // consume , or =>
                        if has_parens && self.peek_kind() == Some(TokenKind::RightParen) {
                            break;
                        }
                        args.push(self.parse_assignment()?);
                    }

                    if has_parens {
                        self.expect_closing_delimiter(TokenKind::RightParen)?;
                    }

                    let end = self.previous_position();
                    Ok(Node::new(
                        NodeKind::Tie { variable, package, args },
                        SourceLocation { start, end },
                    ))
                }
                "untie" => {
                    let start = token_start;
                    self.consume_token()?; // consume untie
                    self.mark_not_stmt_start();

                    let variable = Box::new(self.parse_assignment()?);

                    let end = self.previous_position();
                    Ok(Node::new(
                        NodeKind::Untie { variable },
                        SourceLocation { start, end },
                    ))
                }
                "new" => {
                    // Check for indirect constructor syntax
                    let _start = token_start;
                    // Clone to satisfy borrow checker
                    let text = token.text.clone();

                    if self.is_indirect_call_pattern(&text) {
                        return self.parse_indirect_call();
                    }

                    // Otherwise parse as regular expression
                    self.parse_expression()
                }
                "goto" => {
                    // goto LABEL | goto &sub | goto $expr
                    self.parse_goto()
                }
                // Generic builtin functions that can take arguments without parens.
                // Uses the canonical builtin registry in `perl-builtins-phf`.
                name if Self::is_builtin_function(name) => {
                    let start = token_start;
                    // We need to clone the text to check for indirect call pattern because
                    // is_indirect_call_pattern borrows self mutably to peek ahead
                    let text = token.text.clone();

                    // Check for indirect object syntax before consuming the token
                    if self.is_indirect_call_pattern(&text) {
                        return self.parse_indirect_call();
                    }

                    // Consume the function name token
                    let token = self.consume_token()?;
                    let func_name = token.text;

                    // We're consuming the function name, no longer at statement start
                    self.mark_not_stmt_start();

                    // Check if there are arguments (not followed by semicolon or modifier)
                    match self.peek_kind() {
                        Some(TokenKind::Semicolon)
                        | Some(TokenKind::If)
                        | Some(TokenKind::Unless)
                        | Some(TokenKind::While)
                        | Some(TokenKind::Until)
                        | Some(TokenKind::For)
                        | Some(TokenKind::Foreach)
                        | Some(TokenKind::RightBrace)
                        | Some(TokenKind::Eof)
                        // Word operators bind below list operators, so they terminate a
                        // zero-argument builtin call instead of starting its arguments.
                        | Some(TokenKind::WordOr)
                        | Some(TokenKind::WordAnd)
                        | Some(TokenKind::WordXor)
                        | Some(TokenKind::WordNot)
                        | None => {
                            // No arguments - return as function call with empty args
                            let end = self.previous_position();
                            Ok(Node::new(
                                NodeKind::FunctionCall { name: func_name.to_string(), args: vec![] },
                                SourceLocation { start, end },
                            ))
                        }
                        _ => {
                            // `defined` and `ref` at statement start without parens use
                            // parse_unary() for the single argument. This fixes
                            // the precedence issue: `ref $obj->{list} eq 'ARRAY'` must parse
                            // as `(eq (ref ...) 'ARRAY')` not `(ref (eq ...))`.
                            //
                            // Only these two are included because they specifically have the
                            // arrow-chain pattern (`defined $obj->{k}`, `ref $obj->{list}`)
                            // and should stop the indirect-call path from eating the `->`
                            // chain while still leaving surrounding comparisons outside the
                            // call node.
                            //
                            // When called WITH parens we fall through — parens already delimit.
                            if self.peek_kind() != Some(TokenKind::LeftParen)
                                && Self::is_optional_arg_builtin(func_name.as_ref())
                            {
                                return self.parse_named_unary_statement_call(
                                    start,
                                    func_name.as_ref(),
                                    true,
                                );
                            }

                            // Has arguments - parse them as a comma-separated list
                            let mut args = vec![];

                            // Parse first argument
                            // Special handling for open/pipe/socket which can take my $var as first arg
                            let mut parsed_block_arg = false;
                            if (func_name.as_ref() == "open"
                                || func_name.as_ref() == "pipe"
                                || func_name.as_ref() == "socket")
                                && (self.peek_kind() == Some(TokenKind::My)
                                    || self.peek_kind() == Some(TokenKind::Our)
                                    || self.peek_kind() == Some(TokenKind::Local)
                                    || self.peek_kind() == Some(TokenKind::State))
                            {
                                args.push(self.parse_variable_declaration()?);
                            } else if Self::is_block_list_func(func_name.as_ref())
                                && self.peek_kind() == Some(TokenKind::LeftBrace)
                            {
                                // Special handling for map/grep/sort/first/any/all/etc.
                                // with block first argument
                                args.push(self.parse_builtin_block()?);
                                parsed_block_arg = true;
                            } else if matches!(func_name.as_ref(), "split" | "grep" | "map" | "sort")
                                && self.peek_kind() == Some(TokenKind::Slash)
                            {
                                // For `split /regex/, ...` and `grep /regex/, @list`,
                                // the `/` after these builtins is a regex delimiter, not
                                // division. Roll back the lexer to re-lex the `/` in
                                // ExpectTerm mode so it becomes a regex.
                                self.tokens.relex_as_term();
                                args.push(self.parse_assignment()?);
                            } else if self.peek_kind() == Some(TokenKind::LeftParen)
                                && (Self::is_block_list_func(func_name.as_ref())
                                    || Self::is_optional_arg_builtin(func_name.as_ref())
                                    || Self::is_lvalue_builtin(func_name.as_ref())
                                    || matches!(
                                        func_name.as_ref(),
                                        "exec" | "system" | "print" | "say" | "printf" | "send"
                                    ))
                            {
                                // block-list and filehandle builtins followed by (...) use
                                // parse_args() so that `map({...} keys ...)` and
                                // `exec({...} @prog)` work: the block/hash inside the parens
                                // may be followed by the list without a separating comma.
                                // For print/say/printf, use the filehandle-aware variant so
                                // that `print( $fh EXPR )` works with no comma after $fh.
                                //
                                // is_optional_arg_builtin names (chr, defined, ref, length, etc.)
                                // also use parse_args() when followed by `(` so that the parens
                                // tightly bind to the function name and the ternary operator
                                // applies to the call's RESULT rather than being absorbed as an
                                // argument.  e.g. `chr($x) ? 1 : 0` must parse as
                                // `(ternary (chr $x) 1 0)` not `(chr (ternary $x 1 0))`.
                                //
                                // is_lvalue_builtin names (pos, substr, vec) also use
                                // parse_args() when followed by `(` so that the parens
                                // tightly bind and the subsequent `= RHS` is handled by
                                // parse_lvalue_builtin_assignment_tail as an outer assignment.
                                let paren_args = if matches!(
                                    func_name.as_ref(),
                                    "print" | "say" | "printf" | "send"
                                ) {
                                    self.parse_print_parens_args()?
                                } else {
                                    self.parse_args()?
                                };
                                args.extend(paren_args);
                            } else {
                                // For builtins, use parse_assignment_or_declaration to handle
                                // my/our/local/state declarations inside argument lists
                                args.push(self.parse_assignment_or_declaration()?);
                            }

                            // Handle map/grep/sort { block } LIST case where no comma separates block and list.
                            // Also skip an optional fat arrow (`=>`) which Perl treats as a comma synonym.
                            if parsed_block_arg && !self.is_at_statement_end() {
                                // Skip optional comma or fat arrow before the list
                                if matches!(self.peek_kind(), Some(TokenKind::Comma) | Some(TokenKind::FatArrow)) {
                                    self.consume_token()?;
                                }
                                if !self.is_at_statement_end() {
                                    args.push(self.parse_assignment()?);
                                }
                            }

                            // Parse remaining arguments
                            // For block-list builtins, parse list arguments without requiring
                            // commas. Use is_at_statement_end() so `map { ... } @arr` is
                            // accepted as the last statement before a block close even without
                            // a trailing semicolon.
                            //
                            // Word operators (or, and, xor, not) terminate the argument list
                            // because they bind less tightly than list operators.
                            // e.g., `sort @list or die` => (sort @list) or (die)
                            if Self::is_block_list_func(func_name.as_ref()) {
                                while !self.is_at_statement_end()
                                    && !self.peek_kind().is_some_and(TokenKind::is_low_precedence_word_operator)
                                {
                                    // Skip optional comma or fat arrow
                                    if matches!(self.peek_kind(), Some(TokenKind::Comma) | Some(TokenKind::FatArrow)) {
                                        self.consume_token()?;
                                    }
                                    // Allow optional trailing separator in list-builtin
                                    // argument lists (e.g. `grep defined, @list,;`).
                                    if self.is_at_statement_end() {
                                        break;
                                    }
                                    args.push(self.parse_assignment()?);
                                }
                            } else {
                                // For other functions, require commas (or fat arrows) between arguments
                                // Perl allows `push @array => $value` as well as `push @array, $value`
                                while matches!(self.peek_kind(), Some(TokenKind::Comma) | Some(TokenKind::FatArrow)) {
                                    if self
                                        .consume_bare_lvalue_assignment_separator(func_name.as_ref())?
                                    {
                                        break;
                                    }

                                    self.consume_token()?; // consume comma or fat arrow

                                    // Handle `, =>` (comma then fat arrow) — consume
                                    // the redundant separator.
                                    if self.peek_kind() == Some(TokenKind::FatArrow) {
                                        self.consume_token()?;
                                    }

                                    if self.is_at_statement_end() {
                                        break;
                                    }

                                    // Check if we hit a statement modifier.
                                    match self.peek_kind() {
                                        Some(TokenKind::If)
                                        | Some(TokenKind::Unless)
                                        | Some(TokenKind::While)
                                        | Some(TokenKind::Until)
                                        | Some(TokenKind::For)
                                        | Some(TokenKind::Foreach) => break,
                                        _ => args.push(self.parse_assignment_or_declaration()?),
                                    }
                                }
                            }

                            // Keep closing `)` when args were parenthesized; bare
                            // calls still end at the last argument.
                            let end = args
                                .last()
                                .map(|arg| arg.location.end.max(self.previous_position()))
                                .unwrap_or_else(|| self.previous_position());
                            let call = Node::new(
                                NodeKind::FunctionCall { name: func_name.to_string(), args },
                                SourceLocation { start, end },
                            );
                            let call = self
                                .parse_lvalue_builtin_assignment_tail(func_name.as_ref(), call)?;
                            self.parse_named_unary_statement_tail(call)
                        }
                    }
                }
                _ => {
                    // Regular expression
                    self.parse_expression()
                }
            }
        } else {
            // Regular expression
            self.parse_expression()
        }
    }

    /// Parse statement modifier (if, unless, while, until, for)
    fn parse_statement_modifier(&mut self, statement: Node) -> ParseResult<Node> {
        let modifier_token = self.consume_token()?;
        let modifier = modifier_token.text.to_string();

        // For 'for' and 'foreach', we parse a list expression
        let condition = if matches!(modifier_token.kind, TokenKind::For | TokenKind::Foreach) {
            self.parse_expression()?
        } else {
            // For other modifiers, parse a regular expression
            self.parse_expression()?
        };

        let start = statement.location.start;
        let end = condition.location.end;

        Ok(Node::new(
            NodeKind::StatementModifier {
                statement: Box::new(statement),
                modifier,
                condition: Box::new(condition),
            },
            SourceLocation { start, end },
        ))
    }

    /// Parse a block statement
    fn parse_block(&mut self) -> ParseResult<Node> {
        self.with_block_recursion_guard(|s| {
            let start = s.current_position();

            s.expect(TokenKind::LeftBrace)?;

            let mut statements = Vec::new();

            while s.peek_kind() != Some(TokenKind::RightBrace) && !s.tokens.is_eof() {
                s.check_cancelled()?;

                // Parse statement with error recovery (AC3: Panic Mode Recovery inside blocks)
                let stmt_result = s.parse_statement();
                match stmt_result {
                    Ok(stmt) => {
                        // Don't add empty blocks (from lone semicolons) to the statement list
                        if !matches!(stmt.kind, NodeKind::Block { ref statements } if statements.is_empty()) {
                            statements.push(stmt);
                        }
                    }
                    Err(e) => {
                        // Don't recover from these — propagate immediately
                        if matches!(
                            e,
                            ParseError::RecursionLimit
                                | ParseError::NestingTooDeep { .. }
                                | ParseError::Cancelled
                        ) {
                            return Err(e);
                        }

                        // Record the actual error
                        s.errors.push(e.clone());

                        // Create error node for failed statement
                        let error_location = s.current_position();
                        let error_msg = format!("{}", e);
                        // Collect peek_kind before mutable borrow in recover_from_error
                        let peek_display = s.peek_kind()
                            .map(|k| k.display_name())
                            .unwrap_or("end of input");
                        let error_node = s.recover_from_error(
                            error_msg,
                            "statement".to_string(),
                            peek_display.to_string(),
                            error_location
                        );
                        statements.push(error_node);

                        // Try to synchronize to next statement
                        if !s.synchronize() {
                            // If synchronization fails, we check if we're at block end or EOF
                            if s.peek_kind() == Some(TokenKind::RightBrace) || s.tokens.is_eof() {
                                break;
                            }
                            // Otherwise stop to prevent infinite loop
                            break; 
                        }
                    }
                }

                // parse_statement already invalidates peek, so we don't need to do it again

                // Swallow any stray semicolons before checking for the next statement or closing brace
                while s.peek_kind() == Some(TokenKind::Semicolon) {
                    s.consume_token()?;
                    s.tokens.invalidate_peek();
                }
            }

            // Handle unclosed block at EOF: emit error but return partial block
            if s.peek_kind() == Some(TokenKind::RightBrace) {
                s.expect(TokenKind::RightBrace)?;
            } else {
                // Missing closing brace (EOF or recovery break). Anchor the
                // diagnostic at the opening `{` (recorded in `start`) rather
                // than at end-of-input, so the squiggle lands on the brace the
                // user needs to close and nested unclosed blocks are
                // distinguishable (#5546).
                s.errors.push(ParseError::syntax(
                    "Unclosed block: expected '}' but reached end of input",
                    start,
                ));
            }
            let end = s.previous_position();

            Ok(Node::new(NodeKind::Block { statements }, SourceLocation { start, end }))
        })
    }

    /// Check if the token after `Identifier:` cannot start a Perl statement.
    ///
    /// Returns `true` when the token kind belongs to the set of tokens that are
    /// exclusive to expression contexts and can never begin a statement:
    ///
    /// - `?` — ternary operator (requires a condition before it)
    /// - `:` — ternary else-part (always follows the then-branch)
    /// - `,` — comma separator (expression continuation)
    /// - `=>` — fat arrow (hash key-value context)
    /// - `)` / `]` — closing delimiters (orphan, not a statement)
    /// - EOF — nothing follows the colon
    ///
    /// Notably absent: `TokenKind::Semicolon` and `TokenKind::RightBrace`.  In
    /// Perl, `LABEL: ;` and a final `LABEL:` before a block end are valid
    /// labeled empty-statements, so both must be allowed through as potential
    /// label starts.
    fn third_token_cannot_start_statement(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Question      // ternary `?` operator
            | TokenKind::Colon       // chained ternary else-part
            | TokenKind::Comma       // expression continuation
            | TokenKind::FatArrow   // hash key-value context
            | TokenKind::RightParen // closing paren
            | TokenKind::RightBracket // closing bracket
            | TokenKind::Eof        // end of input
        )
    }

    /// Check if we're at the start of a labeled statement (`LABEL: ...`).
    ///
    /// Uses 3-token lookahead to distinguish label colons from ternary and
    /// hash-constructor colons.  A valid label must be an `Identifier` followed
    /// by a single `:` (not `::`) followed by a token that can start a statement.
    ///
    /// Valid patterns (returns `true`):
    /// - `LABEL: { ... }` — labeled block
    /// - `LABEL: while (...) { }` — labeled loop
    /// - `LABEL: print ...` — labeled expression statement
    /// - `LABEL: ;` — labeled empty statement
    ///
    /// Invalid patterns (returns `false`):
    /// - `foo: ?` — ternary operator after colon
    /// - `foo: :` — chained ternary else-part
    /// - `foo: ,` — expression continuation
    /// - `foo: =>` — fat-arrow hash context
    fn is_label_start(&mut self) -> bool {
        // We need an identifier followed by a colon
        if self.peek_kind() != Some(TokenKind::Identifier) {
            return false;
        }

        // Check if the second token is a colon
        let Ok(second_token) = self.tokens.peek_second() else {
            return false;
        };
        if second_token.kind != TokenKind::Colon {
            return false;
        }

        // Check the 3rd token (token after the colon)
        // If it can't start a statement, this is not a label
        if let Ok(third_token) = self.tokens.peek_third() {
            if Self::third_token_cannot_start_statement(third_token.kind) {
                return false;
            }
        }

        // Single colon (`:`, not `::`) unambiguously indicates a label in Perl.
        // Qualified identifiers use `::` which tokenizes as DoubleColon, so
        // `Identifier Colon` (single colon) is always a label — even for
        // uppercase names like OUTER:, LOOP:, LINE: which are idiomatic Perl labels.
        true
    }

    /// Parse a labeled statement (LABEL: statement)
    fn parse_labeled_statement(&mut self) -> ParseResult<Node> {
        let start = self.current_position();

        // Parse the label
        let label_token = self.expect(TokenKind::Identifier)?;
        let label = label_token.text.to_string();

        // Consume the colon
        self.expect(TokenKind::Colon)?;

        let statement = self.parse_label_statement_body()?;

        let end = self.previous_position();
        Ok(Node::new(
            NodeKind::LabeledStatement { label, statement },
            SourceLocation { start, end },
        ))
    }

    /// Parse loop control statement (next, last, redo)
    fn parse_loop_control(&mut self) -> ParseResult<Node> {
        let start = self.current_position();
        let op_token = self.consume_token()?;
        let op = op_token.text.to_string();

        self.mark_not_stmt_start();

        // Check for optional label.
        // Labels may be ordinary identifiers, and phase keywords are also
        // valid labels when used in labeled-loop control (`last CHECK`).
        let label = if matches!(
            self.peek_kind(),
            Some(TokenKind::Identifier)
                | Some(TokenKind::Begin)
                | Some(TokenKind::End)
                | Some(TokenKind::Check)
                | Some(TokenKind::Init)
                | Some(TokenKind::Unitcheck)
        ) {
            let label_token = self.consume_token()?;
            Some(label_token.text.to_string())
        } else {
            None
        };

        let end = self.previous_position();
        Ok(Node::new(
            NodeKind::LoopControl { op, label },
            SourceLocation { start, end },
        ))
    }

    /// Parse a phase-block keyword token used as a statement label.
    ///
    /// Perl allows phase-block keywords (BEGIN, END, CHECK, INIT, UNITCHECK) as
    /// statement labels when followed by `:`.  Because these tokenise as their own
    /// `TokenKind` variants rather than `TokenKind::Identifier`, the standard
    /// `parse_labeled_statement` (which calls `expect(TokenKind::Identifier)`)
    /// cannot handle them.  This function consumes the keyword token, then the
    /// `:`, then the subordinate statement, producing a `LabeledStatement` node.
    fn parse_keyword_as_label(&mut self) -> ParseResult<Node> {
        let start = self.current_position();

        // Consume the phase-keyword token (BEGIN / END / CHECK / INIT / UNITCHECK)
        let label_token = self.consume_token()?;
        let label = label_token.text.to_string();

        // Consume the `:`
        self.expect(TokenKind::Colon)?;

        let statement = self.parse_label_statement_body()?;

        let end = self.previous_position();
        Ok(Node::new(
            NodeKind::LabeledStatement { label, statement },
            SourceLocation { start, end },
        ))
    }

    fn parse_label_statement_body(&mut self) -> ParseResult<Box<Node>> {
        if matches!(self.peek_kind(), Some(TokenKind::RightBrace) | Some(TokenKind::Eof)) {
            let pos = self.current_position();
            return Ok(Box::new(Node::new(
                NodeKind::Block { statements: Vec::new() },
                SourceLocation { start: pos, end: pos },
            )));
        }

        Ok(Box::new(self.parse_statement()?))
    }

}

#[cfg(test)]
mod statement_terminator_seam_tests {
    use super::Parser;

    #[test]
    fn starts_qr_slash_body_boundary_discriminator() {
        assert!(Parser::starts_qr_slash_body(b"qr/", 2), "direct qr delimiter");
        assert!(Parser::starts_qr_slash_body(b"qr /", 3), "spaced qr delimiter");
        assert!(!Parser::starts_qr_slash_body(b"/", 0), "bare slash is division");
        assert!(!Parser::starts_qr_slash_body(b"ar/", 2), "suffix ar is not qr");
    }

    /// Colocated observer for the OPERATORS-miss early return in
    /// `quote_like_body_end`. Bare `/` and `+` are not operators and are not
    /// alphanumeric, so they hit the miss-return specifically (not the later
    /// alphanumeric delimiter guard). A recognized operator must still succeed.
    #[test]
    fn quote_like_body_end_operators_miss_returns_none() {
        assert_eq!(
            Parser::quote_like_body_end(b"/foo/", 0),
            None,
            "bare '/' must miss OPERATORS and return None"
        );
        assert_eq!(
            Parser::quote_like_body_end(b"+ 1", 0),
            None,
            "bare '+' must miss OPERATORS and return None"
        );
        assert_eq!(
            Parser::quote_like_body_end(b"q(foo)", 0),
            Some(6),
            "recognized operator must still return Some(end)"
        );
    }
}
