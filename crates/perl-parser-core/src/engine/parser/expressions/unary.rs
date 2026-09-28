/// Returns `true` when `kind` is a token that can immediately follow a
/// single-punctuation typeglob such as `*<`, `*>`, `*(`, or `*)`.
///
/// This lets the parser distinguish the typeglob form (e.g. `*REAL_USER_ID = *<;`)
/// from a dereference expression (e.g. `*<EXPR>`), without consuming the
/// punctuation character first.
fn is_typeglob_punct_terminator(kind: Option<TokenKind>) -> bool {
    matches!(
        kind,
        Some(
            TokenKind::Semicolon
                | TokenKind::Comma
                | TokenKind::RightParen
                | TokenKind::RightBrace
                | TokenKind::RightBracket
                | TokenKind::Eof
        ) | None
    )
}

fn typeglob_punctuation_name(
    kind: TokenKind,
    text: &str,
    next_kind: Option<TokenKind>,
) -> Option<String> {
    if !is_typeglob_punct_terminator(next_kind) {
        return None;
    }

    let name = match kind {
        TokenKind::Backslash => "\\",
        TokenKind::Semicolon => ";",
        TokenKind::Percent | TokenKind::HashSigil => "%",
        TokenKind::BitwiseNot => "~",
        TokenKind::BitwiseXor => "^",
        TokenKind::Not => "!",
        TokenKind::SubSigil | TokenKind::BitwiseAnd => "&",
        TokenKind::ScalarSigil => "$",
        TokenKind::ArraySigil => "@",
        TokenKind::LeftBracket => "[",
        TokenKind::RightBracket => "]",
        TokenKind::Unknown if text.starts_with('"') => "\"",
        TokenKind::Unknown if text.starts_with('`') => "`",
        TokenKind::Unknown if text.starts_with('\'') => "'",
        _ => return None,
    };

    Some(name.to_string())
}

impl<'a> Parser<'a> {
    fn is_contextual_await_start(&mut self) -> bool {
        if self.peek_kind() != Some(TokenKind::Identifier)
            || !self.tokens.peek().is_ok_and(|token| token.text.as_ref() == "await")
        {
            return false;
        }

        let second_kind = self.tokens.peek_second().ok().map(|token| token.kind());
        if matches!(
            second_kind,
            Some(TokenKind::FatArrow | TokenKind::Arrow | TokenKind::DoubleColon)
        ) {
            return false;
        }

        if second_kind == Some(TokenKind::Colon)
            && self.tokens.peek_third().is_ok_and(|token| token.kind() == TokenKind::Colon)
        {
            return false;
        }

        true
    }

    /// Parse unary expression
    fn parse_unary(&mut self) -> ParseResult<Node> {
        self.with_recursion_guard(|s| s.parse_unary_inner())
    }

    fn parse_unary_inner(&mut self) -> ParseResult<Node> {
        if self.peek_kind() == Some(TokenKind::Slash) {
            self.reclassify_head_as_term()?;
        }

        if self.is_contextual_await_start() {
            let op_token = self.advance_token()?;
            let start = op_token.start();

            if self.tokens.is_eof() || self.is_at_statement_end() {
                let end = op_token.end();
                // Charged before the enclosing node so the charge order matches
                // the original construction order.
                let charged_operand = self.charge_node(
                    NodeKind::Undef,
                    SourceLocation { start: end, end },
                )?;
                return self.charge_node(
                    NodeKind::Unary {
                        op: op_token.text.to_string(),
                        operand: Box::new(charged_operand),
                    },
                    SourceLocation { start, end },
                );
            }

            let operand = self.parse_unary()?;
            let end = operand.location.end;

            return self.charge_node(
                NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                SourceLocation { start, end },
            );
        }

        if let Some(kind) = self.peek_kind() {
            match kind {
                TokenKind::Minus => {
                    let op_token = self.advance_token()?;
                    let start = op_token.start();

                    // Check for file test operators (-e, -f, -d, etc.)
                    if let Some(TokenKind::Identifier) = self.peek_kind() {
                        let next_token = self.tokens.peek()?;
                        if next_token.text.len() == 1 {
                            // Before a fat arrow, `-G`, `-r`, etc. are hash/list keys, not
                            // file-test operators.
                            if self
                                .tokens
                                .peek_second()
                                .is_ok_and(|token| token.kind() == TokenKind::FatArrow)
                            {
                                let test_token = self.advance_token()?;
                                let end = test_token.end();
                                return self.charge_node(
                                    NodeKind::Identifier {
                                        name: format!("-{}", test_token.text),
                                    },
                                    SourceLocation { start, end },
                                );
                            }

                            // It's a file test operator
                            let test_token = self.advance_token()?;
                            let file_test = format!("-{}", test_token.text);

                            // File test can be used without operand (tests $_).
                            // Treat a following comma as end-of-expression so that
                            // `grep -e, @INC` and `grep !/pat/ && -d, @list` parse
                            // correctly: the comma is the EXPR/LIST separator for
                            // grep/map, not an argument to the file-test operator.
                            let operand = if self.is_at_statement_end()
                                || self.peek_kind() == Some(TokenKind::Comma)
                                || matches!(
                                    self.peek_kind(),
                                    Some(
                                        TokenKind::And
                                            | TokenKind::Or
                                            | TokenKind::DefinedOr
                                            | TokenKind::Greater
                                            | TokenKind::Less
                                            | TokenKind::GreaterEqual
                                            | TokenKind::LessEqual
                                            | TokenKind::Equal
                                            | TokenKind::NotEqual
                                            | TokenKind::RightParen
                                            | TokenKind::RightBrace
                                            | TokenKind::RightBracket
                                    )
                                ) {
                                // No operand, test $_
                                self.charge_node(
                                    NodeKind::Variable {
                                        sigil: "$".to_string(),
                                        name: "_".to_string(),
                                    },
                                    SourceLocation { start: test_token.end(), end: test_token.end() },
                                )?
                            } else {
                                self.parse_unary()?
                            };

                            let end = operand.location.end;
                            return self.charge_node(
                                NodeKind::Unary { op: file_test, operand: Box::new(operand) },
                                SourceLocation { start, end },
                            );
                        }
                    }

                    // Word-operator keywords (`or`, `and`, `xor`, `not`, `cmp`) cannot be
                    // parsed as primary expressions, but Perl permits them as bareword hash
                    // keys when immediately followed by `=>`:
                    //   -or => 1, -and => 2, -xor => 3
                    // The fat arrow auto-quotes the combined "-keyword" string.
                    if self.tokens.peek_second().is_ok_and(|t| t.kind() == TokenKind::FatArrow)
                        && let Some(kw_kind) = self.peek_kind()
                            && Self::is_word_op_keyword(kw_kind) {
                                let kw_token = self.advance_token()?;
                                let end = kw_token.end();
                                return self.charge_node(
                                    NodeKind::Identifier {
                                        name: format!("-{}", kw_token.text),
                                    },
                                    SourceLocation { start, end },
                                );
                            }

                    // Regular unary minus
                    let operand = if self.goto_starts_control_flow() {
                        self.parse_goto()?
                    } else {
                        self.parse_power()?
                    };
                    let end = operand.location.end;

                    return self.charge_node(
                        NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                        SourceLocation { start, end },
                    );
                }
                TokenKind::Plus => {
                    let op_token = self.advance_token()?;
                    let start = op_token.start();

                    // Special case: +{ ... } forces a hash constructor (not a block)
                    // If followed by ->, allow postfix chaining: +{@_}->{key}
                    if self.peek_kind() == Some(TokenKind::LeftBrace) {
                        // Parse as hash literal
                        let hash = self.parse_hash_or_block()?;
                        let end = hash.location.end;

                        // Wrap the hash in a unary plus to preserve the explicit disambiguation
                        let node = self.charge_node(
                            NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(hash) },
                            SourceLocation { start, end },
                        )?;
                        return self.parse_postfix_chain(node);
                    }

                    // Check if we're at EOF or a terminator (for standalone operators)
                    if self.tokens.is_eof() || self.is_at_statement_end() {
                        // Create a placeholder for standalone operator
                        let end = op_token.end();
                        // Charged before the enclosing node so the charge order matches
                        // the original construction order.
                        let charged_operand_2 = self.charge_node(
                            NodeKind::Undef,
                            SourceLocation { start: end, end },
                        )?;
                        return self.charge_node(
                            NodeKind::Unary {
                                op: op_token.text.to_string(),
                                operand: Box::new(charged_operand_2),
                            },
                            SourceLocation { start, end },
                        );
                    }

                    let operand = if self.goto_starts_control_flow() {
                        self.parse_goto()?
                    } else {
                        self.parse_power()?
                    };
                    let end = operand.location.end;

                    return self.charge_node(
                        NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                        SourceLocation { start, end },
                    );
                }
                // Handle 'not' keyword as a unary prefix at expression level.
                // This lets `$a && not $b` parse correctly.
                // Canonical boundary lives in parse_word_not_expr (#13932).
                TokenKind::WordNot => {
                    return self.parse_word_not_expr();
                }
                TokenKind::Not | TokenKind::Backslash | TokenKind::BitwiseNot | TokenKind::Star => {
                    let op_token = self.advance_token()?;
                    let start = op_token.start();

                    // AC1: Disambiguate typeglob (*foo) from multiplication (*)
                    // If TokenKind is Star and it is followed by an identifier or {
                    if op_token.kind() == TokenKind::Star
                        && let Some(next_kind) = self.peek_kind()
                    {
                        let next_text = self.tokens.peek()?.text.to_string();
                        let next_is_sigil_identifier = next_kind == TokenKind::Identifier
                            && next_text
                                .chars()
                                .next()
                                .is_some_and(|c| matches!(c, '$' | '@' | '%' | '&' | '*'));
                        let terminator_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                        if let Some(name) =
                            typeglob_punctuation_name(next_kind, &next_text, terminator_kind)
                        {
                            let t = self.advance_token()?;
                            return self.charge_node(
                                NodeKind::Typeglob { name, body: None },
                                SourceLocation { start, end: t.end() },
                            );
                        }

                        match next_kind {
                            kind if Self::can_be_sub_name(kind) && !next_is_sigil_identifier => {
                                let id_token = self.advance_token()?;
                                let end = id_token.end();
                                let node = self.charge_node(
                                    NodeKind::Typeglob {
                                        name: id_token.text.to_string(),
                                        body: None,
                                    },
                                    SourceLocation { start, end },
                                )?;
                                // Allow postfix chaining: *$self->{key}
                                return self.parse_postfix_chain(node);
                            }
                            TokenKind::LeftBrace => {
                                // Dynamic typeglob *{$name}
                                // Parse the braced primary before looking for assignment so a
                                // postfix such as `*{$glob}{CODE}` cannot be mistaken for a
                                // direct dynamic typeglob assignment.
                                let brace_expr = self.parse_primary()?;
                                let direct_assignment = self.peek_kind() == Some(TokenKind::Assign);
                                let body_end = brace_expr.location.end;
                                let brace_expr = self.parse_postfix_chain(brace_expr)?;
                                let end = brace_expr.location.end;
                                if direct_assignment {
                                    // Slice the braced source text (including the
                                    // braces) and normalize exactly like the
                                    // variable-path dynamic typeglob assignment.
                                    let raw = String::from_utf8_lossy(
                                        &self.src_bytes[brace_expr.location.start..body_end],
                                    );
                                    let name = normalize_dynamic_typeglob_name(&raw);
                                    // A computed body keeps its braced name; retain
                                    // the parsed expression as the structured body
                                    // so scope analysis can see the variables it
                                    // uses (#15731). A braced bareword strips to
                                    // its static name and carries no body.
                                    let body = name.starts_with('{').then(|| Box::new(brace_expr));
                                    return self.charge_node(
                                        NodeKind::Typeglob { name, body },
                                        SourceLocation { start, end: body_end },
                                    );
                                }
                                let node = self.charge_node(
                                    NodeKind::Unary {
                                        op: "*{}".to_string(),
                                        operand: Box::new(brace_expr),
                                    },
                                    SourceLocation { start, end },
                                )?;
                                return self.parse_postfix_chain(node);
                            }
                            TokenKind::BitwiseXor => {
                                // *^X typeglob for control variable $^X (e.g. *^N, *^W, *^F)
                                self.consume_token()?; // consume ^
                                if let Some(TokenKind::Identifier) = self.peek_kind() {
                                    let id_token = self.advance_token()?;
                                    let name = format!("^{}", id_token.text);
                                    let end = id_token.end();
                                    return self.charge_node(
                                        NodeKind::Typeglob { name, body: None },
                                        SourceLocation { start, end },
                                    );
                                }
                                // Standalone *^ — fall through to parse operand
                            }
                            // Typeglobs for Perl's process-identity punctuation variables.
                            // English.pm aliases: *REAL_USER_ID = *<; *EFFECTIVE_USER_ID = *>;
                            //                    *REAL_GROUP_ID = *(; *EFFECTIVE_GROUP_ID = *);
                            // Use 2-token lookahead so *<EXPR> and *(EXPR) still fall through
                            // when a real sub-expression follows the punctuation character.
                            TokenKind::Less => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: "<".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            TokenKind::Greater => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: ">".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            TokenKind::LeftParen => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: "(".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            TokenKind::RightParen => {
                                // *) is always a typeglob for $) (effective GID).
                                // RightParen cannot start a valid sub-expression in this
                                // context, so no lookahead disambiguation is needed.
                                let t = self.advance_token()?;
                                return self.charge_node(
                                    NodeKind::Typeglob { name: ")".to_string(), body: None },
                                    SourceLocation { start, end: t.end() },
                                );
                            }
                            // *? = typeglob for $? (child process status).
                            // Question cannot start an expression after *, so no lookahead.
                            TokenKind::Question => {
                                let t = self.advance_token()?;
                                return self.charge_node(
                                    NodeKind::Typeglob { name: "?".to_string(), body: None },
                                    SourceLocation { start, end: t.end() },
                                );
                            }
                            // *, = typeglob for $, (output field separator).
                            // Comma cannot start an expression after *, so no lookahead.
                            TokenKind::Comma => {
                                let t = self.advance_token()?;
                                return self.charge_node(
                                    NodeKind::Typeglob { name: ",".to_string(), body: None },
                                    SourceLocation { start, end: t.end() },
                                );
                            }
                            // *= — the lexer emits StarAssign for the compound assignment
                            // operator, so Star followed by bare Assign is always the typeglob
                            // *=  (for $= "format lines per page").  No lookahead needed.
                            TokenKind::Assign => {
                                let t = self.advance_token()?;
                                return self.charge_node(
                                    NodeKind::Typeglob { name: "=".to_string(), body: None },
                                    SourceLocation { start, end: t.end() },
                                );
                            }
                            // */ = typeglob for $/ (input record separator).
                            // Use lookahead: if followed by a statement terminator, it's a
                            // typeglob; otherwise fall through (could be multiply + regex).
                            TokenKind::Slash => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: "/".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            // *. = typeglob for $. (input line number).
                            // Use lookahead: if followed by a statement terminator, it's a
                            // typeglob; otherwise fall through (could be multiply + concat).
                            TokenKind::Dot => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: ".".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            // *| = typeglob for $| (output autoflush flag).
                            // Use lookahead: if followed by a statement terminator, it's a
                            // typeglob; otherwise fall through (could be multiply + bitwise-or).
                            TokenKind::BitwiseOr => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: "|".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            // *: = typeglob for $: (format line-break characters).
                            // Use lookahead: if followed by a statement terminator, it's a
                            // typeglob; otherwise fall through (could be label or ternary colon).
                            TokenKind::Colon => {
                                let second_kind = self.tokens.peek_second().ok().map(|t| t.kind());
                                if is_typeglob_punct_terminator(second_kind) {
                                    let t = self.advance_token()?;
                                    return self.charge_node(
                                        NodeKind::Typeglob { name: ":".to_string(), body: None },
                                        SourceLocation { start, end: t.end() },
                                    );
                                }
                            }
                            TokenKind::ArraySigil
                            | TokenKind::HashSigil
                            | TokenKind::SubSigil
                            | TokenKind::BitwiseAnd => {
                                // `*@x` and friends: a bare aggregate or
                                // code sigil directly after the typeglob
                                // sigil is the bare double-sigil shape of
                                // issue #15750 (only `$` scalars, braced
                                // expressions, and names are valid unbraced
                                // `*` operands). Surface the UnexpectedToken
                                // diagnostic + ERROR node instead of
                                // silently parsing the sigil as a variable
                                // operand.
                                let bad = self.advance_token()?;
                                let bad_text = bad.text.to_string();
                                let node = self.recover_from_error(
                                    "bare '*' sigil followed by another sigil — \
                                     not a valid Perl variable"
                                        .to_string(),
                                    "identifier, '$', or '{' after '*' sigil".to_string(),
                                    bad_text,
                                    start,
                                );
                                // Widen the ERROR span to cover the bad token.
                                let mut node = node;
                                node.location.end = bad.end();
                                return Ok(node);
                            }
                            // The lexer folds a trailing sigil+name like `*@x`
                            // into one Identifier token, so the operand parse
                            // below would silently build a `Variable` from it.
                            // A non-`$` sigil after the typeglob sigil is the
                            // bare double-sigil shape of issue #15750 (only `$`
                            // scalars, braced expressions, and plain names are
                            // valid unbraced `*` operands): surface the
                            // UnexpectedToken diagnostic + ERROR node instead.
                            TokenKind::Identifier
                                if next_text.starts_with(['@', '%', '&', '*']) =>
                            {
                                let bad = self.advance_token()?;
                                let bad_text = bad.text.to_string();
                                let node = self.recover_from_error(
                                    "bare '*' sigil followed by another sigil — \
                                     not a valid Perl variable"
                                        .to_string(),
                                    "identifier, '$', or '{' after '*' sigil".to_string(),
                                    bad_text,
                                    start,
                                );
                                let mut node = node;
                                node.location.end = bad.end();
                                return Ok(node);
                            }
                            _ => {}
                        }
                    }

                    // Check if we're at EOF or a terminator (for standalone operators)
                    if self.tokens.is_eof() || self.is_at_statement_end() {
                        // Create a placeholder for standalone operator
                        let end = op_token.end();
                        // Charged before the enclosing node so the charge order matches
                        // the original construction order.
                        let charged_operand_4 = self.charge_node(
                            NodeKind::Undef,
                            SourceLocation { start: end, end },
                        )?;
                        return self.charge_node(
                            NodeKind::Unary {
                                op: op_token.text.to_string(),
                                operand: Box::new(charged_operand_4),
                            },
                            SourceLocation { start, end },
                        );
                    }

                    let operand = if self.goto_starts_control_flow() {
                        self.parse_goto()?
                    } else if matches!(
                        op_token.kind(),
                        TokenKind::Not | TokenKind::Backslash | TokenKind::BitwiseNot
                    ) {
                        self.parse_power()?
                    } else {
                        self.parse_unary()?
                    };
                    let end = operand.location.end;

                    let node = self.charge_node(
                        NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                        SourceLocation { start, end },
                    )?;

                    // For typeglob (*), allow postfix chaining: *$self->{field}
                    if op_token.kind() == TokenKind::Star {
                        return self.parse_postfix_chain(node);
                    }

                    return Ok(node);
                }
                TokenKind::Increment | TokenKind::Decrement => {
                    // Pre-increment and pre-decrement
                    let op_token = self.advance_token()?;
                    let start = op_token.start();
                    let operand = self.parse_unary()?;
                    let end = operand.location.end;

                    return self.charge_node(
                        NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                        SourceLocation { start, end },
                    );
                }
                TokenKind::SmartMatch => {
                    // Smart match can be used as a unary operator
                    let op_token = self.advance_token()?;
                    let start = op_token.start();

                    // Check if we're at EOF or a terminator (for standalone operators)
                    if self.tokens.is_eof() || self.is_at_statement_end() {
                        // Create a placeholder for standalone operator
                        let end = op_token.end();
                        // Charged before the enclosing node so the charge order matches
                        // the original construction order.
                        let charged_operand_5 = self.charge_node(
                            NodeKind::Undef,
                            SourceLocation { start: end, end },
                        )?;
                        return self.charge_node(
                            NodeKind::Unary {
                                op: op_token.text.to_string(),
                                operand: Box::new(charged_operand_5),
                            },
                            SourceLocation { start, end },
                        );
                    }

                    let operand = self.parse_unary()?;
                    let end = operand.location.end;

                    return self.charge_node(
                        NodeKind::Unary { op: op_token.text.to_string(), operand: Box::new(operand) },
                        SourceLocation { start, end },
                    );
                }
                _ => {}
            }
        }

        self.parse_postfix()
    }

    /// Returns `true` for word-operator token kinds that cannot be parsed as
    /// primary expressions but are valid as negative bareword hash keys when
    /// immediately followed by `=>`: `-or => 1`, `-and => 2`, `-xor => 3`.
    fn is_word_op_keyword(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::WordOr
                | TokenKind::WordAnd
                | TokenKind::WordXor
                | TokenKind::WordNot
                | TokenKind::StringCompare // cmp
        )
    }
}
