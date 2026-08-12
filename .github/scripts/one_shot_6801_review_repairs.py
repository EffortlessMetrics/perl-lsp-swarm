from pathlib import Path


def replace_section(text: str, start_marker: str, end_marker: str, transform):
    start = text.index(start_marker)
    end = text.index(end_marker, start)
    section = text[start:end]
    return text[:start] + transform(section) + text[end:]


def insert_after(lines: list[str], marker: str, added: list[str]) -> None:
    index = lines.index(marker)
    lines[index + 1 : index + 1] = added


engine_path = Path("crates/perl-parser/src/incremental/incremental_advanced_reuse.rs")
engine = engine_path.read_text(encoding="utf-8")


def repair_content(section: str) -> str:
    lines = section.splitlines()
    insert_after(
        lines,
        "    ) {",
        [
            "        let mut used_target_positions: HashSet<usize> = reuse_map",
            "            .values()",
            "            .map(|strategy| strategy.target_position)",
            "            .collect();",
            "",
        ],
    )
    insert_after(
        lines,
        "                for (new_pos, new_info) in &new_analysis.node_info {",
        [
            "                    if used_target_positions.contains(new_pos) {",
            "                        continue;",
            "                    }",
            "",
        ],
    )
    start = lines.index("                        if confidence >= config.min_confidence")
    end = lines.index("                        }", start)
    lines[start : end + 1] = [
        "                        if confidence >= config.min_confidence {",
        "                            reuse_map.insert(",
        "                                *old_pos,",
        "                                ReuseStrategy {",
        "                                    target_position: *new_pos,",
        "                                    reuse_type: ReuseType::ContentUpdate,",
        "                                    confidence_score: confidence,",
        "                                    position_adjustment: (*new_pos as isize)",
        "                                        - (*old_pos as isize),",
        "                                },",
        "                            );",
        "                            used_target_positions.insert(*new_pos);",
        "                            self.analysis_stats.content_matches += 1;",
        "                            break;",
        "                        }",
    ]
    return "\n".join(lines) + "\n"


engine = replace_section(
    engine,
    "    fn find_content_updated_matches(",
    "    /// Find aggressive structural matches",
    repair_content,
)


def repair_aggressive(section: str) -> str:
    lines = section.splitlines()
    insert_after(
        lines,
        "    ) {",
        [
            "        let mut used_target_positions: HashSet<usize> = reuse_map",
            "            .values()",
            "            .map(|strategy| strategy.target_position)",
            "            .collect();",
            "",
        ],
    )
    occupancy_start = lines.index("                if reuse_map")
    occupancy_end = lines.index("                }", occupancy_start)
    lines[occupancy_start : occupancy_end + 1] = [
        "                if used_target_positions.contains(new_pos) {",
        "                    continue;",
        "                }",
    ]
    registration_start = lines.index(
        "                if confidence >= config.min_confidence * 0.7"
    )
    registration_end = lines.index("                }", registration_start)
    lines[registration_start : registration_end + 1] = [
        "                if confidence >= config.min_confidence * 0.7 {",
        "                    // Final threshold check",
        "                    reuse_map.insert(",
        "                        *old_pos,",
        "                        ReuseStrategy {",
        "                            target_position: best_pos,",
        "                            reuse_type: ReuseType::StructuralEquivalent,",
        "                            confidence_score: confidence,",
        "                            position_adjustment: (best_pos as isize)",
        "                                - (*old_pos as isize),",
        "                        },",
        "                    );",
        "                    used_target_positions.insert(best_pos);",
        "                    self.analysis_stats.reuse_candidates_found += 1;",
        "                }",
    ]
    return "\n".join(lines) + "\n"


engine = replace_section(
    engine,
    "    fn find_aggressive_structural_matches(",
    "    /// Validate reuse candidates",
    repair_aggressive,
)
engine_path.write_text(engine, encoding="utf-8")

parser_path = Path("crates/perl-parser/src/incremental/incremental_v2.rs")
parser = parser_path.read_text(encoding="utf-8")
analysis_start = parser.index("        // Store analysis results for inspection")
analysis_end = parser.index("\n        None\n", analysis_start) + len("\n        None\n")
selected_only = (
    "        // Expose only the analysis selected for the produced tree.\n"
    "        if analysis_result.reused_nodes <= analysis_result.total_new_nodes\n"
    "            && analysis_result\n"
    "                .meets_efficiency_target(self.reuse_config.min_confidence * 100.0)\n"
    "        {\n"
    "            self.reused_nodes = analysis_result.reused_nodes;\n"
    "            self.reparsed_nodes =\n"
    "                analysis_result.total_new_nodes - analysis_result.reused_nodes;\n"
    "            self.advanced_reuse_selected = true;\n"
    "            self.last_reuse_analysis = Some(analysis_result);\n"
    "            return Some(new_tree);\n"
    "        }\n"
    "\n"
    "        None\n"
)
parser = parser[:analysis_start] + selected_only + parser[analysis_end:]
parser_path.write_text(parser, encoding="utf-8")

test_path = Path("crates/perl-parser/tests/incremental_parser_accuracy.rs")
tests = test_path.read_text(encoding="utf-8")
import_anchor = "use perl_parser::edit::Edit;\n"
if tests.count(import_anchor) != 1:
    raise SystemExit("reuse-config import anchor drifted")
tests = tests.replace(
    import_anchor,
    import_anchor + "use perl_parser::incremental_advanced_reuse::ReuseConfig;\n",
    1,
)
if "fn rejected_advanced_analysis_is_not_exposed_as_last_parse()" in tests:
    raise SystemExit("rejected-analysis regression already exists")
tests = (
    tests.rstrip()
    + """

#[test]
fn rejected_advanced_analysis_is_not_exposed_as_last_parse() -> TestResult {
    let config = ReuseConfig { min_confidence: 1.1, ..ReuseConfig::default() };
    let mut incremental = IncrementalParserV2::with_reuse_config(config);
    let source = "my $x = 1;";
    incremental.parse(source)?;

    let edited =
        apply_incremental_edit(&mut incremental, source, "1", "2", "rejected-analysis")?;
    incremental.parse(&edited)?;

    if !incremental.used_incremental_path() {
        return Err("the simple fallback must accept the queued value edit".into());
    }
    if incremental.used_advanced_reuse() {
        return Err("the over-threshold advanced analysis must be rejected".into());
    }
    if incremental.get_last_reuse_analysis().is_some() {
        return Err("a rejected advanced analysis must not remain public".into());
    }
    if !incremental
        .get_reuse_efficiency_report()
        .starts_with("Basic Incremental Analysis:")
    {
        return Err("the efficiency report must describe the accepted simple path".into());
    }
    Ok(())
}
"""
)
test_path.write_text(tests, encoding="utf-8")

Path(__file__).unlink()
