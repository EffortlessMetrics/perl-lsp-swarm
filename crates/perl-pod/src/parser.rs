//! Canonical byte-ranged POD paragraph parser (#8444).

use crate::model::{PodBlock, PodCommand, PodModelVersion, PodParse, PodRange, PodTextBlock};
use crate::paragraph::{
    PhysicalLine, PhysicalLines, is_blank_line, line_starts_verbatim, parse_command_line,
};

enum OpenParagraph {
    Ordinary { start: usize, end: usize },
    Verbatim { start: usize, end: usize },
}

impl OpenParagraph {
    fn append_line(&mut self, line: &PhysicalLine<'_>) {
        match self {
            Self::Ordinary { end, .. } | Self::Verbatim { end, .. } => {
                *end = line.content_end;
            }
        }
    }

    fn range(&self) -> PodRange {
        match self {
            Self::Ordinary { start, end } | Self::Verbatim { start, end } => {
                PodRange { start: *start, end: *end }
            }
        }
    }
}

/// Parse `source` into ordered byte-ranged POD blocks.
#[must_use]
pub fn parse_pod_source(source: &str) -> PodParse {
    let mut parse = PodParse::empty();
    parse.model_version = PodModelVersion::V1;
    let mut in_pod = false;
    let mut open: Option<OpenParagraph> = None;

    let flush_open = |open: &mut Option<OpenParagraph>, blocks: &mut Vec<PodBlock>| {
        if let Some(para) = open.take() {
            let range = para.range();
            if range.start >= range.end {
                return;
            }
            let block = match para {
                OpenParagraph::Ordinary { .. } => PodBlock::Ordinary(PodTextBlock { range }),
                OpenParagraph::Verbatim { .. } => PodBlock::Verbatim(PodTextBlock { range }),
            };
            blocks.push(block);
        }
    };

    for line in PhysicalLines::new(source) {
        if !in_pod {
            if let Some(cmd) = parse_command_line(&line) {
                in_pod = true;
                let ends_pod = cmd.name == "cut";
                emit_command(&mut parse.blocks, cmd);
                if ends_pod {
                    in_pod = false;
                }
            }
            continue;
        }

        if is_blank_line(line.content) {
            flush_open(&mut open, &mut parse.blocks);
            continue;
        }

        if let Some(cmd) = parse_command_line(&line) {
            flush_open(&mut open, &mut parse.blocks);
            let ends_pod = cmd.name == "cut";
            emit_command(&mut parse.blocks, cmd);
            if ends_pod {
                in_pod = false;
            }
            continue;
        }

        let verbatim = line_starts_verbatim(line.content);
        match (&mut open, verbatim) {
            (None, true) => {
                open = Some(OpenParagraph::Verbatim { start: line.start, end: line.content_end });
            }
            (None, false) => {
                open = Some(OpenParagraph::Ordinary { start: line.start, end: line.content_end });
            }
            (Some(OpenParagraph::Verbatim { .. }), true) => {
                if let Some(para) = open.as_mut() {
                    para.append_line(&line);
                }
            }
            (Some(OpenParagraph::Ordinary { .. }), false) => {
                if let Some(para) = open.as_mut() {
                    para.append_line(&line);
                }
            }
            (Some(_), _) => {
                flush_open(&mut open, &mut parse.blocks);
                open = Some(if verbatim {
                    OpenParagraph::Verbatim { start: line.start, end: line.content_end }
                } else {
                    OpenParagraph::Ordinary { start: line.start, end: line.content_end }
                });
            }
        }
    }

    flush_open(&mut open, &mut parse.blocks);
    parse
}

fn emit_command(blocks: &mut Vec<PodBlock>, cmd: crate::paragraph::ParsedCommand) {
    let pod_cmd =
        PodCommand { name: cmd.name.clone(), range: cmd.range, argument_range: cmd.argument_range };
    if cmd.known {
        blocks.push(PodBlock::Command(pod_cmd));
    } else {
        blocks.push(PodBlock::UnknownCommand(pod_cmd));
    }
}
