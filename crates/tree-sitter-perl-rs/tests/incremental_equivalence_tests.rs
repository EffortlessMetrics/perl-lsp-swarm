//! Black-box proof that the facade's incremental entry point remains equivalent
//! to a fresh parse across editor-shaped edits.
//!
//! This harness deliberately makes no reuse claim. It proves the correctness
//! contract first; reuse metrics and latency budgets belong to the incremental
//! kernel once the facade is wired to it.

use std::error::Error;

use perl_position_tracking::Position;
use tree_sitter_perl_rs::{InputEdit, Node, OverlayDefinition, Parser, Tree, VisibleImport};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy)]
struct EditSpec {
    needle: &'static str,
    replacement: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
struct NodeSnapshot {
    kind: String,
    is_error: bool,
    field_name: Option<String>,
    start_byte: usize,
    end_byte: usize,
    start_position: (usize, usize),
    end_position: (usize, usize),
    text: String,
    children: Vec<NodeSnapshot>,
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticSnapshot {
    definition: Option<OverlayDefinition>,
    imports: Vec<VisibleImport>,
    pragma: String,
}

fn position(source: &str, byte: usize) -> Position {
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().map_or(prefix.len(), str::len);
    Position::new(byte, line as u32, column as u32)
}

fn replace_range(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut result = source.to_owned();
    result.replace_range(start..end, replacement);
    result
}

fn snapshot(node: Node<'_>, source: &str, field_name: Option<String>) -> TestResult<NodeSnapshot> {
    let mut children = Vec::with_capacity(node.child_count());
    for index in 0..node.child_count() {
        let child = node.child(index).ok_or("child count and indexed access diverged")?;
        let child_field = node.field_name_for_child(index).map(str::to_owned);
        children.push(snapshot(child, source, child_field)?);
    }

    let start_position = node.start_position();
    let end_position = node.end_position();
    Ok(NodeSnapshot {
        kind: node.kind(),
        is_error: node.is_error(),
        field_name,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_position: (start_position.row, start_position.column),
        end_position: (end_position.row, end_position.column),
        text: node.utf8_text(source.as_bytes())?.to_owned(),
        children,
    })
}

fn semantic_snapshot(tree: &Tree, source: &str) -> Vec<SemanticSnapshot> {
    let overlay = tree.semantic_overlay();
    let mut offsets = source.char_indices().map(|(byte, _)| byte).take(8).collect::<Vec<_>>();
    offsets.push(source.len());
    offsets.sort_unstable();
    offsets.dedup();
    offsets
        .into_iter()
        .map(|offset| SemanticSnapshot {
            definition: overlay.definition_at_offset(offset),
            imports: overlay.visible_imports_at_offset(offset),
            pragma: format!("{:?}", overlay.pragma_state_at_offset(offset)),
        })
        .collect()
}

fn equivalent(incremental: &Tree, fresh: &Tree, source: &str) -> TestResult {
    assert_eq!(incremental.root_node().to_sexp(), fresh.root_node().to_sexp());
    assert_eq!(incremental.diagnostics(), fresh.diagnostics(), "incremental diagnostics diverged");
    assert_eq!(incremental.has_error(), fresh.has_error(), "incremental recovery status diverged");
    assert_eq!(
        snapshot(incremental.root_node(), source, None)?,
        snapshot(fresh.root_node(), source, None)?,
        "incremental tree diverged from fresh parse"
    );
    assert_eq!(
        semantic_snapshot(incremental, source),
        semantic_snapshot(fresh, source),
        "incremental semantic overlay diverged from fresh parse"
    );
    Ok(())
}

fn apply_and_compare(
    parser: &mut Parser,
    old_tree: &mut Tree,
    source: &mut String,
    start: usize,
    old_end: usize,
    replacement: &str,
) -> TestResult {
    let new_source = replace_range(source, start, old_end, replacement);
    let edit = InputEdit::new(
        start,
        old_end,
        start + replacement.len(),
        position(source, start),
        position(source, old_end),
        position(&new_source, start + replacement.len()),
    );
    old_tree.edit(&edit);

    let incremental = parser
        .parse_with_old_tree(&new_source, old_tree)
        .ok_or("incremental parse returned None")?;
    let fresh = parser.parse(&new_source).ok_or("fresh parse returned None")?;
    equivalent(&incremental, &fresh, &new_source)?;

    *source = new_source;
    *old_tree = incremental;
    Ok(())
}

fn run_specs(initial: &str, specs: &[EditSpec]) -> TestResult {
    let mut parser = Parser::new();
    let mut source = initial.to_owned();
    let mut tree = parser.parse(&source).ok_or("initial parse returned None")?;

    for spec in specs {
        let start = source.find(spec.needle).ok_or_else(|| {
            format!("seeded edit needle {:?} was absent from current source", spec.needle)
        })?;
        apply_and_compare(
            &mut parser,
            &mut tree,
            &mut source,
            start,
            start + spec.needle.len(),
            spec.replacement,
        )?;
    }
    Ok(())
}

#[test]
fn seeded_edit_sequences_match_fresh_parse() -> TestResult {
    run_specs(
        "my $x = 1;\nmy $unicode = \"café\";\n",
        &[
            EditSpec { needle: "1", replacement: "42" },
            EditSpec { needle: "café", replacement: "naïve" },
            EditSpec { needle: "my $x", replacement: "my $value" },
            EditSpec { needle: "42", replacement: "" },
            EditSpec { needle: "my $unicode", replacement: "our $unicode" },
        ],
    )?;

    run_specs(
        "my $value = $a / 2;\nmy $match = $value =~ /foo/;\nmy $hash = { key => 1 };\n",
        &[
            EditSpec { needle: "/ 2", replacement: "/ 3" },
            EditSpec { needle: "foo", replacement: "bar" },
            EditSpec { needle: "key", replacement: "other" },
            EditSpec { needle: "{ other => 1 }", replacement: "{ other => 2 }" },
        ],
    )?;

    run_specs(
        "my $text = <<'END';\nhello $name\nEND\nsub nested { my $inner = 1; }\n",
        &[
            EditSpec { needle: "hello $name", replacement: "hello $user" },
            EditSpec { needle: "my $inner", replacement: "state $inner" },
            EditSpec { needle: "sub nested", replacement: "sub renamed" },
            EditSpec { needle: "state $inner = 1", replacement: "state $inner = 2" },
        ],
    )?;

    run_specs(
        "my $x = q{alpha};\nmy $y = s/foo/bar/;\nformat REPORT =\nName: @<<<\n$x\n.\n",
        &[
            EditSpec { needle: "alpha", replacement: "beta" },
            EditSpec { needle: "foo", replacement: "baz" },
            EditSpec { needle: "REPORT", replacement: "SUMMARY" },
            EditSpec { needle: "Name", replacement: "Value" },
        ],
    )?;

    run_specs(
        "if ($x) { print $x; }\n",
        &[
            EditSpec { needle: "}", replacement: "" },
            EditSpec { needle: "print $x;", replacement: "print ;" },
            EditSpec { needle: "print ;", replacement: "print $x; }" },
        ],
    )?;

    run_specs(
        "my $x = 1;\nmy $y = 2;\n",
        &[
            EditSpec { needle: "\n", replacement: "\r\n" },
            EditSpec { needle: "my $y = 2;\n", replacement: "my $y = 2;\r\n" },
        ],
    )
}

#[test]
fn deterministic_seeded_insertions_remain_fresh_equivalent() -> TestResult {
    let mut parser = Parser::new();
    let mut source = "my $x = 1;\nmy $y = 2;\nmy $z = 3;\n".to_owned();
    let mut tree = parser.parse(&source).ok_or("initial parse returned None")?;
    let mut seed = 0x5EED_u64;

    for _ in 0..16 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut boundaries: Vec<usize> = source.char_indices().map(|(byte, _)| byte).collect();
        boundaries.push(source.len());
        let start = boundaries[(seed as usize) % boundaries.len()];
        let insertion = if seed & 1 == 0 { " " } else { "\n# seeded edit\n" };
        apply_and_compare(&mut parser, &mut tree, &mut source, start, start, insertion)?;
    }
    Ok(())
}

#[test]
fn deterministic_mixed_editor_edits_remain_fresh_equivalent() -> TestResult {
    let seeds = [
        "my $value = $left / 2;\nmy $regex = qr/foo/;\n",
        "my $doc = <<'END';\nbody\nEND\nsub nested { my $x = 1; }\n",
        "use strict;\nmy $special = $!;\nmy $hash = { key => 1 };\n",
    ];

    for initial in seeds {
        let mut parser = Parser::new();
        let mut source = initial.to_owned();
        let mut tree = parser.parse(&source).ok_or("initial parse returned None")?;
        let mut seed = source.len() as u64 ^ 0xA11C_E5E1;

        for _ in 0..24 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut boundaries = source.char_indices().map(|(byte, _)| byte).collect::<Vec<_>>();
            boundaries.push(source.len());
            let start = boundaries
                .get((seed as usize) % boundaries.len())
                .copied()
                .ok_or("editor boundary selection failed")?;
            let operation = seed % 3;
            let (old_end, replacement) = match operation {
                0 => (start, if seed & 1 == 0 { " " } else { "\n# edit\n" }),
                1 if start < source.len() => {
                    let end = source[start..]
                        .char_indices()
                        .nth(1)
                        .map_or(source.len(), |(offset, _)| start + offset);
                    (end, "")
                }
                _ => (start, if seed & 1 == 0 { "x" } else { "42" }),
            };
            apply_and_compare(&mut parser, &mut tree, &mut source, start, old_end, replacement)?;
        }
    }
    Ok(())
}
