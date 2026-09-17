use perl_module::{has_standalone_module_token_boundaries, parse_module_token};

fn next_u64(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn gen_char(seed: &mut u64) -> char {
    const ALPHABET: &[char] = &[
        'a', 'Z', '0', '9', '_', ':', '/', '\\', '\'', ' ', ';', '(', ')', '[', ']', '{', '}', 'λ',
        'Ж', '界', 'é', '🙂',
    ];
    ALPHABET[(next_u64(seed) as usize) % ALPHABET.len()]
}

fn gen_string(seed: &mut u64, max_len: usize) -> String {
    let len = (next_u64(seed) % (max_len as u64 + 1)) as usize;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(gen_char(seed));
    }
    out
}

fn gen_ident_start(seed: &mut u64) -> char {
    const STARTS: &[char] = &['a', 'Z', '_', 'λ', 'Ж', '界', 'é'];
    STARTS[(next_u64(seed) as usize) % STARTS.len()]
}

fn gen_ident_continue(seed: &mut u64) -> char {
    const CONT: &[char] = &['a', 'Z', '0', '9', '_', 'λ', 'Ж', '界', 'é'];
    CONT[(next_u64(seed) as usize) % CONT.len()]
}

fn gen_identifier(seed: &mut u64, max_len: usize) -> String {
    let len = ((next_u64(seed) as usize) % max_len).saturating_add(1);
    let mut out = String::with_capacity(len);
    out.push(gen_ident_start(seed));
    for _ in 1..len {
        out.push(gen_ident_continue(seed));
    }
    out
}

fn gen_valid_module_token(seed: &mut u64) -> String {
    let segments = ((next_u64(seed) as usize) % 4).saturating_add(1);
    let mut token = gen_identifier(seed, 16);
    for _ in 1..segments {
        let sep = if next_u64(seed).is_multiple_of(2) { "::" } else { "'" };
        token.push_str(sep);
        token.push_str(&gen_identifier(seed, 16));
    }
    token
}

#[test]
fn fuzz_token_core_does_not_panic_on_random_utf8_inputs() {
    let mut state = 0xF00D_BAAD_BEEF_CAFE_u64;

    for _ in 0..5000 {
        let line = gen_string(&mut state, 128);
        let start = (next_u64(&mut state) as usize) % (line.len() + 1);

        let span = parse_module_token(&line, start);
        if let Some(span) = span {
            assert!(span.start <= span.end);
            assert!(span.end <= line.len());
            assert!(line.is_char_boundary(span.start));
            assert!(line.is_char_boundary(span.end));
            let token = &line[span.start..span.end];
            assert!(!token.is_empty());
            let _ = has_standalone_module_token_boundaries(&line, span.start, span.end);
        }

        let end = (next_u64(&mut state) as usize) % (line.len() + 1);
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        let _ = has_standalone_module_token_boundaries(&line, start, end);
    }
}

#[test]
fn fuzz_valid_unicode_module_tokens_are_parsed_to_expected_span() {
    let mut state = 0xBADC_0FFE_EE01_1234_u64;
    const SAFE_CONTEXT: &[char] = &[' ', '\t', ';', ',', '(', ')', '[', ']'];

    for _ in 0..5000 {
        let token = gen_valid_module_token(&mut state);
        let left = SAFE_CONTEXT[(next_u64(&mut state) as usize) % SAFE_CONTEXT.len()];
        let right = SAFE_CONTEXT[(next_u64(&mut state) as usize) % SAFE_CONTEXT.len()];
        let line = format!("{left}{token}{right}");
        let start = left.len_utf8();
        let expected_end = start + token.len();

        let parsed = parse_module_token(&line, start);
        assert!(parsed.is_some(), "expected token parse for {token:?} in {line:?}");
        let parsed = parsed.unwrap_or_else(|| unreachable!("checked is_some"));
        assert_eq!(parsed.start, start);
        assert_eq!(parsed.end, expected_end);
    }
}

#[test]
fn fuzz_boundary_detection_for_standalone_vs_embedded_tokens() {
    let mut state = 0xCAFE_F00D_1234_5678_u64;
    const SAFE_CONTEXT: &[char] = &[' ', '\t', ';', ',', '(', ')', '[', ']'];
    const EMBED_CONTEXT: &[char] = &['a', 'Z', '0', '_', ':', 'λ', '界'];

    for _ in 0..3000 {
        let token = gen_valid_module_token(&mut state);
        let safe_left = SAFE_CONTEXT[(next_u64(&mut state) as usize) % SAFE_CONTEXT.len()];
        let safe_right = SAFE_CONTEXT[(next_u64(&mut state) as usize) % SAFE_CONTEXT.len()];
        let safe_line = format!("{safe_left}{token}{safe_right}");
        let safe_start = safe_left.len_utf8();
        assert!(has_standalone_module_token_boundaries(
            &safe_line,
            safe_start,
            safe_start + token.len()
        ));

        let embed_left = EMBED_CONTEXT[(next_u64(&mut state) as usize) % EMBED_CONTEXT.len()];
        let embed_right = EMBED_CONTEXT[(next_u64(&mut state) as usize) % EMBED_CONTEXT.len()];

        let left_embedded = format!("{embed_left}{token} ");
        let left_start = embed_left.len_utf8();
        assert!(!has_standalone_module_token_boundaries(
            &left_embedded,
            left_start,
            left_start + token.len()
        ));

        let right_embedded = format!(" {token}{embed_right}");
        assert!(!has_standalone_module_token_boundaries(&right_embedded, 1, 1 + token.len()));
    }
}
