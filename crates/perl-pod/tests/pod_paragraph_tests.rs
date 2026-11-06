//! Byte-ranged POD paragraph parser tests (#8444).

#![deny(clippy::map_err_ignore)]

use perl_pod::{PodBlock, PodCommand, PodModelVersion, PodParse, PodRange, parse_pod};
use perl_test_must::must_some_with;

fn block_text(source: &str, range: PodRange) -> &str {
    &source[range.start..range.end]
}

fn assert_ranges_in_bounds(source: &str, parse: &PodParse) {
    let len = source.len();
    let mut prev_end = 0usize;
    for block in &parse.blocks {
        let range = block.range();
        assert!(range.start <= range.end, "range start after end: {range:?}");
        assert!(range.end <= len, "range end {range:?} exceeds source len {len}");
        assert!(
            source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
            "range {range:?} not on UTF-8 boundary"
        );
        assert!(
            range.start >= prev_end,
            "blocks overlap or out of order: prev_end={prev_end} range={range:?}"
        );
        prev_end = range.end;
    }
}

trait BlockRange {
    fn range(&self) -> PodRange;
}

impl BlockRange for PodBlock {
    fn range(&self) -> PodRange {
        match self {
            PodBlock::Command(c) => c.range,
            PodBlock::Ordinary(t) => t.range,
            PodBlock::Verbatim(t) => t.range,
            PodBlock::UnknownCommand(c) => c.range,
        }
    }
}

#[test]
fn pod_free_input_yields_no_blocks() {
    let source = "package Foo;\n\nsub bar { 1 }\n";
    let parse = parse_pod(source);
    assert!(parse.blocks.is_empty());
    assert_eq!(parse.model_version, PodModelVersion::V1);
}

#[test]
fn empty_input_yields_no_blocks() {
    let parse = parse_pod("");
    assert!(parse.blocks.is_empty());
}

#[test]
fn code_before_between_and_after_pod_regions() {
    let source = concat!(
        "# preamble\n",
        "=head1 NAME\n",
        "\n",
        "Module name.\n",
        "\n",
        "=cut\n",
        "sub foo {}\n",
        "=pod\n",
        "\n",
        "Second region.\n",
        "\n",
        "=cut\n",
        "# tail\n"
    );
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    assert_eq!(parse.blocks.len(), 6);
    assert!(matches!(parse.blocks[0], PodBlock::Command(_)));
    assert!(matches!(parse.blocks[1], PodBlock::Ordinary(_)));
    assert!(matches!(parse.blocks[2], PodBlock::Command(_)));
    assert!(matches!(parse.blocks[3], PodBlock::Command(_)));
    assert!(matches!(parse.blocks[4], PodBlock::Ordinary(_)));
    assert!(matches!(parse.blocks[5], PodBlock::Command(_)));
    assert!(!block_text(source, parse.blocks[0].range()).starts_with('#'));
}

#[test]
fn exact_cut_ends_pod_cutlery_is_unknown_inside_pod() {
    let source = "=head1 ARGUMENTS\n\nFirst.\n\n=cutlery\n\nSecond.\n\n=cut trailing\n";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    let kinds: Vec<_> = parse
        .blocks
        .iter()
        .map(|b| match b {
            PodBlock::Command(_) => "command",
            PodBlock::Ordinary(_) => "ordinary",
            PodBlock::Verbatim(_) => "verbatim",
            PodBlock::UnknownCommand(_) => "unknown",
        })
        .collect();
    assert!(kinds.contains(&"unknown"));
    assert!(matches!(
        &parse.blocks[kinds.len() - 1],
        PodBlock::Command(PodCommand { name, .. }) if name == "cut"
    ));
    assert!(matches!(
        &parse.blocks[2],
        PodBlock::UnknownCommand(PodCommand { name, .. }) if name == "cutlery"
    ));
}

#[test]
fn head1_not_head10_and_unknown_command_preserved() {
    let source = "=head10 Title\n\n=xyzzy magic\n\n=cut\n";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    assert!(matches!(&parse.blocks[0], PodBlock::UnknownCommand(c) if c.name == "head10"));
    assert!(matches!(&parse.blocks[1], PodBlock::UnknownCommand(c) if c.name == "xyzzy"));
}

#[test]
fn indented_command_is_ordinary_not_command() {
    let source = "=head1 NAME\n\n  =fake directive\n\n=cut\n";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    assert!(
        parse.blocks.iter().all(|b| !matches!(b, PodBlock::Command(c) if c.name == "fake")),
        "indented =fake must not be a command block"
    );
    let text_block = must_some_with(
        parse.blocks.iter().find_map(|b| match b {
            PodBlock::Ordinary(t) => Some(t),
            PodBlock::Verbatim(t) => Some(t),
            _ => None,
        }),
        "expected text paragraph with indented =fake",
    );
    assert!(block_text(source, text_block.range).contains("=fake"));
}

#[test]
fn ordinary_vs_verbatim_paragraphs_preserve_bytes() {
    let source = "=head1 DESCRIPTION\n\nPlain line.\n\n    indented code\n\t tabbed\n\n=cut\n";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    let verbatim = must_some_with(
        parse.blocks.iter().find_map(|b| match b {
            PodBlock::Verbatim(t) => Some(t),
            _ => None,
        }),
        "verbatim block",
    );
    let text = block_text(source, verbatim.range);
    assert!(text.starts_with(' ') || text.starts_with('\t'));
    assert!(text.contains("    indented code"));
    assert!(text.contains('\t'));
}

#[test]
fn lf_crlf_bare_cr_and_unicode_ranges() {
    let lf = "=head1 NAME\n\nλ\n\n=cut\n";
    let parse_lf = parse_pod(lf);
    assert_ranges_in_bounds(lf, &parse_lf);

    let crlf = "=head1 NAME\r\n\r\nλ\r\n\r\n=cut\r\n";
    let parse_crlf = parse_pod(crlf);
    assert_ranges_in_bounds(crlf, &parse_crlf);

    let bare_cr = "=head1 NAME\r\rλ\r\r=cut\r";
    let parse_cr = parse_pod(bare_cr);
    assert_ranges_in_bounds(bare_cr, &parse_cr);

    for (source, parse) in [(lf, &parse_lf), (crlf, &parse_crlf), (bare_cr, &parse_cr)] {
        let ordinary = must_some_with(
            parse.blocks.iter().find_map(|b| match b {
                PodBlock::Ordinary(t) => Some(t),
                _ => None,
            }),
            "unicode ordinary paragraph",
        );
        assert_eq!(block_text(source, ordinary.range), "λ");
    }
}

#[test]
fn eof_without_final_newline_and_without_cut() {
    let source = "=head1 NAME\n\nTrailing pod";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    assert!(!parse.blocks.is_empty());
    let last = must_some_with(parse.blocks.last(), "EOF pod must retain trailing paragraph");
    assert!(matches!(last, PodBlock::Ordinary(_)));
    assert_eq!(block_text(source, last.range()), "Trailing pod");
}

#[test]
fn multiline_ordinary_paragraph_single_block() {
    let source =
        "=head1 DESCRIPTION\n\nLine one.\nLine two still same paragraph.\n\nNext para.\n\n=cut\n";
    let parse = parse_pod(source);
    assert_ranges_in_bounds(source, &parse);
    let first_ordinary = must_some_with(
        parse.blocks.iter().find_map(|b| match b {
            PodBlock::Ordinary(t) => Some(t),
            _ => None,
        }),
        "first ordinary paragraph",
    );
    let text = block_text(source, first_ordinary.range);
    assert!(text.contains("Line one."));
    assert!(text.contains("Line two still same paragraph."));
    assert!(!text.contains("Next para."));
}

#[test]
fn command_argument_range_covers_full_line() {
    let source = "=head1 NAME\n\n=cut\n";
    let parse = parse_pod(source);
    assert!(matches!(&parse.blocks[0], PodBlock::Command(_)));
    let cmd = must_some_with(
        match &parse.blocks[0] {
            PodBlock::Command(c) => Some(c),
            _ => None,
        },
        "head1 command block",
    );
    assert_eq!(cmd.name, "head1");
    assert_eq!(block_text(source, cmd.range), "=head1 NAME");
    assert_eq!(block_text(source, cmd.argument_range), "NAME");
}

#[test]
fn head1_command_not_prefix_matched_as_head() {
    let source = "=headache\n\n=cut\n";
    let parse = parse_pod(source);
    assert!(matches!(
        &parse.blocks[0],
        PodBlock::UnknownCommand(c) if c.name == "headache"
    ));
}
