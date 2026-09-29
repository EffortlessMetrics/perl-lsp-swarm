use std::path::Path;

use perl_module::{extract_use_lib_operations, extract_use_lib_paths, resolve_use_lib_paths};

fn fuzz_string(state: &mut u64, max_len: usize) -> String {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;

    let len = (*state as usize) % (max_len.saturating_add(1));
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        let byte = match (*state % 16) as u8 {
            0 => b'\n',
            1 => b'\r',
            2 => b'\t',
            3 => b'"',
            4 => b'\'',
            5 => b';',
            6 => b'(',
            7 => b')',
            8 => b'/',
            9 => b'\\',
            10 => b'.',
            11 => b',',
            _ => b'a' + ((*state % 26) as u8),
        };
        out.push(char::from(byte));
    }
    out
}

fn fuzz_path(state: &mut u64) -> String {
    let segment: String = fuzz_string(state, 24)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-'))
        .collect();
    let segment = if segment.is_empty() { "fallback" } else { segment.as_str() };
    format!("lib/{segment}")
}

#[test]
fn fuzz_use_lib_parser_and_resolver_preserve_core_invariants() {
    let mut seed = 0x00C0_FFEE_F00D_BAAD_u64;

    for _ in 0..5_000 {
        let source = fuzz_string(&mut seed, 256);
        let paths = extract_use_lib_paths(&source);
        let ops = extract_use_lib_operations(&source);

        for path in &paths {
            assert!(
                !path.path.is_empty(),
                "extracted use lib path should never be empty; source={source:?}"
            );
        }

        let resolved = resolve_use_lib_paths(
            &paths,
            Path::new("/workspace"),
            Some(Path::new("/workspace/project/lib")),
        );

        for rel in resolved {
            assert!(
                !Path::new(&rel).is_absolute(),
                "resolved include path should be relative to workspace: {rel:?}; source={source:?}"
            );
            assert!(
                !Path::new(&rel)
                    .components()
                    .any(|component| component == std::path::Component::ParentDir),
                "resolved include path should not contain parent traversals: {rel:?}; source={source:?}"
            );
        }

        for op in &ops {
            match op {
                perl_module::UseLibAction::Add(op_paths)
                | perl_module::UseLibAction::Remove(op_paths) => {
                    for op_path in op_paths {
                        assert!(
                            !op_path.path.is_empty(),
                            "operation paths should never be empty; source={source:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fuzz_leading_begin_wrapper_preserves_static_pragma_operations() {
    let mut seed = 0xB16B_00B5_1AC0_11EC_u64;

    for _ in 0..2_000 {
        let first = fuzz_path(&mut seed);
        let second = fuzz_path(&mut seed);
        let selector = seed;
        let pragma = match selector % 5 {
            0 => format!("use lib '{first}';"),
            1 => format!("use lib qw({first} {second});"),
            2 => format!("no lib '{first}';"),
            3 => format!("use lib \"{first}\";"),
            _ => format!("no lib \"{first}\";"),
        };
        let wrapped = match (selector >> 2) % 4 {
            0 => format!("BEGIN {{ {pragma}\n}}\n"),
            1 => format!("BEGIN\n{{\n{pragma}\n}}\n"),
            2 => format!("BEGIN # compile-time phase\n{{ {pragma}\n}}\n"),
            _ => format!("BEGIN\n{{\n# static include roots\n{pragma}\n}}\n"),
        };

        let expected = extract_use_lib_operations(&pragma);
        assert!(!expected.is_empty(), "generated pragma must be static: {pragma:?}");
        assert_eq!(
            extract_use_lib_operations(&wrapped),
            expected,
            "leading BEGIN wrapper changed pragma operations; wrapped={wrapped:?}"
        );
    }
}
