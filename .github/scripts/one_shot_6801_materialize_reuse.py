from pathlib import Path


ENGINE = Path("crates/perl-parser/src/incremental/incremental_advanced_reuse.rs")
PARSER = Path("crates/perl-parser/src/incremental/incremental_v2.rs")
ACCURACY = Path("crates/perl-parser/tests/incremental_parser_accuracy.rs")

engine = ENGINE.read_text(encoding="utf-8")

direct_anchor = (
    "                if old_info.structural_hash == new_info.structural_hash\n"
    "                    && old_info.children_count == new_info.children_count\n"
    "                {\n"
)
direct_replacement = (
    "                if old_info.structural_hash == new_info.structural_hash\n"
    "                    && old_info.content_hash == new_info.content_hash\n"
    "                    && old_info.children_count == new_info.children_count\n"
    "                {\n"
)
if engine.count(direct_anchor) != 1:
    raise SystemExit("direct-match anchor drifted")
engine = engine.replace(direct_anchor, direct_replacement, 1)

content_start = engine.index("    /// Calculate content-based hash for value comparison")
content_end = engine.index("    /// Get count of direct children for a node", content_start)
content_hash_block = '''    /// Calculate content-based hash for exact subtree comparison.
    fn calculate_content_hash(&self, node: &Node) -> u64 {
        let mut hasher = DefaultHasher::new();
        node.to_sexp().hash(&mut hasher);
        hasher.finish()
    }

'''
engine = engine[:content_start] + content_hash_block + engine[content_end:]
ENGINE.write_text(engine, encoding="utf-8")

parser = PARSER.read_text(encoding="utf-8")
import_anchor = (
    "use super::incremental_advanced_reuse::{AdvancedReuseAnalyzer, ReuseAnalysisResult, ReuseConfig};\n"
)
import_replacement = '''use super::incremental_advanced_reuse::{
    AdvancedReuseAnalyzer, ReuseAnalysisResult, ReuseConfig, ReuseStrategy, ReuseType,
};
'''
if parser.count(import_anchor) != 1:
    raise SystemExit("advanced-reuse import anchor drifted")
parser = parser.replace(import_anchor, import_replacement, 1)

advanced_start = parser.index("    /// Try advanced reuse analysis for sophisticated tree reuse")
advanced_end = parser.index("    fn is_simple_value_edit", advanced_start)
advanced_block = '''    /// Try advanced reuse analysis for sophisticated tree reuse.
    fn try_advanced_reuse_parse(
        &mut self,
        source: &str,
        last_tree: &IncrementalTree,
    ) -> Option<Node> {
        let mut parser = Parser::new(source);
        let new_tree = parser.parse().ok()?;

        let mut analysis_result = self.reuse_analyzer.analyze_reuse_opportunities(
            &last_tree.root,
            &new_tree,
            &self.pending_edits,
            &self.reuse_config,
        );
        let (reuse_map, replacements) = self.collect_materializable_reuse(
            &last_tree.root,
            &new_tree,
            &analysis_result.reuse_map,
        );
        analysis_result.reuse_map = reuse_map;
        analysis_result.reused_nodes = analysis_result.reuse_map.len();
        analysis_result.reuse_percentage = if analysis_result.total_old_nodes == 0 {
            0.0
        } else {
            analysis_result.reused_nodes as f64 / analysis_result.total_old_nodes as f64 * 100.0
        };

        if analysis_result.reused_nodes == 0
            || analysis_result.reused_nodes > analysis_result.total_new_nodes
            || !analysis_result
                .meets_efficiency_target(self.reuse_config.min_confidence * 100.0)
        {
            return None;
        }

        let materialized_tree = self.materialize_advanced_reuse_tree(&new_tree, &replacements);
        if materialized_tree != new_tree {
            return None;
        }

        self.reused_nodes = analysis_result.reused_nodes;
        self.reparsed_nodes = analysis_result.total_new_nodes - analysis_result.reused_nodes;
        self.advanced_reuse_selected = true;
        self.last_reuse_analysis = Some(analysis_result);
        Some(materialized_tree)
    }

    fn collect_materializable_reuse(
        &self,
        old_tree: &Node,
        new_tree: &Node,
        reuse_map: &HashMap<usize, ReuseStrategy>,
    ) -> (HashMap<usize, ReuseStrategy>, HashMap<usize, Vec<(Node, Node)>>) {
        let mut materialized_map = HashMap::new();
        let mut replacements: HashMap<usize, Vec<(Node, Node)>> = HashMap::new();

        for (old_position, strategy) in reuse_map {
            if !matches!(&strategy.reuse_type, ReuseType::Direct | ReuseType::PositionShift) {
                continue;
            }
            let Some(old_node) = Self::find_analyzed_node_at_start(old_tree, *old_position) else {
                continue;
            };
            let Some(new_node) =
                Self::find_analyzed_node_at_start(new_tree, strategy.target_position)
            else {
                continue;
            };

            let replacement =
                self.clone_with_shifted_positions(old_node, strategy.position_adjustment);
            if &replacement != new_node {
                continue;
            }

            materialized_map.insert(*old_position, strategy.clone());
            replacements
                .entry(strategy.target_position)
                .or_default()
                .push((new_node.clone(), replacement));
        }

        (materialized_map, replacements)
    }

    fn find_analyzed_node_at_start(node: &Node, start: usize) -> Option<&Node> {
        let mut found = (node.location.start == start).then_some(node);
        let mut inspect = |child: &Node| {
            if let Some(candidate) = Self::find_analyzed_node_at_start(child, start) {
                found = Some(candidate);
            }
        };

        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for statement in statements {
                    inspect(statement);
                }
            }
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                inspect(variable);
                if let Some(initializer) = initializer {
                    inspect(initializer);
                }
            }
            NodeKind::Binary { left, right, .. } => {
                inspect(left);
                inspect(right);
            }
            NodeKind::Unary { operand, .. } => inspect(operand),
            NodeKind::FunctionCall { args, .. } => {
                for argument in args {
                    inspect(argument);
                }
            }
            NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
                inspect(condition);
                inspect(then_branch);
                for (condition, branch) in elsif_branches {
                    inspect(condition);
                    inspect(branch);
                }
                if let Some(branch) = else_branch {
                    inspect(branch);
                }
            }
            _ => {}
        }

        found
    }

    fn materialize_advanced_reuse_tree(
        &self,
        new_node: &Node,
        replacements: &HashMap<usize, Vec<(Node, Node)>>,
    ) -> Node {
        if let Some(candidates) = replacements.get(&new_node.location.start)
            && let Some((_, replacement)) =
                candidates.iter().find(|(target, _)| target == new_node)
        {
            return replacement.clone();
        }

        let kind = match &new_node.kind {
            NodeKind::Program { statements } => NodeKind::Program {
                statements: statements
                    .iter()
                    .map(|statement| {
                        self.materialize_advanced_reuse_tree(statement, replacements)
                    })
                    .collect(),
            },
            NodeKind::Block { statements } => NodeKind::Block {
                statements: statements
                    .iter()
                    .map(|statement| {
                        self.materialize_advanced_reuse_tree(statement, replacements)
                    })
                    .collect(),
            },
            NodeKind::VariableDeclaration { declarator, variable, attributes, initializer } => {
                NodeKind::VariableDeclaration {
                    declarator: declarator.clone(),
                    variable: Box::new(
                        self.materialize_advanced_reuse_tree(variable, replacements),
                    ),
                    attributes: attributes.clone(),
                    initializer: initializer.as_ref().map(|initializer| {
                        Box::new(
                            self.materialize_advanced_reuse_tree(initializer, replacements),
                        )
                    }),
                }
            }
            NodeKind::Binary { op, left, right } => NodeKind::Binary {
                op: op.clone(),
                left: Box::new(self.materialize_advanced_reuse_tree(left, replacements)),
                right: Box::new(self.materialize_advanced_reuse_tree(right, replacements)),
            },
            NodeKind::Unary { op, operand } => NodeKind::Unary {
                op: op.clone(),
                operand: Box::new(self.materialize_advanced_reuse_tree(operand, replacements)),
            },
            NodeKind::FunctionCall { name, args } => NodeKind::FunctionCall {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|argument| {
                        self.materialize_advanced_reuse_tree(argument, replacements)
                    })
                    .collect(),
            },
            NodeKind::If {
                condition,
                then_branch,
                elsif_branches,
                else_branch,
                keyword,
                ..
            } => NodeKind::If {
                condition: Box::new(
                    self.materialize_advanced_reuse_tree(condition, replacements),
                ),
                then_branch: Box::new(
                    self.materialize_advanced_reuse_tree(then_branch, replacements),
                ),
                elsif_branches: elsif_branches
                    .iter()
                    .map(|(condition, branch)| {
                        (
                            Box::new(
                                self.materialize_advanced_reuse_tree(condition, replacements),
                            ),
                            Box::new(
                                self.materialize_advanced_reuse_tree(branch, replacements),
                            ),
                        )
                    })
                    .collect(),
                else_branch: else_branch.as_ref().map(|branch| {
                    Box::new(self.materialize_advanced_reuse_tree(branch, replacements))
                }),
                keyword: keyword.clone(),
            },
            _ => new_node.kind.clone(),
        };

        Node::new(kind, new_node.location)
    }

'''
parser = parser[:advanced_start] + advanced_block + parser[advanced_end:]

unit_anchor = "    #[test]\n    fn test_performance_timing_detailed() -> ParseResult<()> {\n"
if parser.count(unit_anchor) != 1:
    raise SystemExit("incremental-v2 unit-test anchor drifted")
unit_test = '''    #[test]
    fn advanced_reuse_materializes_only_exact_old_subtrees() {
        let parser = IncrementalParserV2::new();
        let location = |start, end| SourceLocation { start, end };
        let old_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number { value: "1".to_string() },
                        location(1, 2),
                    ),
                    Node::new(
                        NodeKind::Number { value: "2".to_string() },
                        location(3, 4),
                    ),
                ],
            },
            location(0, 4),
        );
        let new_tree = Node::new(
            NodeKind::Program {
                statements: vec![
                    Node::new(
                        NodeKind::Number { value: "1".to_string() },
                        location(1, 2),
                    ),
                    Node::new(
                        NodeKind::Number { value: "3".to_string() },
                        location(3, 4),
                    ),
                ],
            },
            location(0, 4),
        );
        let reuse_map = HashMap::from([
            (
                1,
                ReuseStrategy {
                    target_position: 1,
                    reuse_type: ReuseType::Direct,
                    confidence_score: 1.0,
                    position_adjustment: 0,
                },
            ),
            (
                3,
                ReuseStrategy {
                    target_position: 3,
                    reuse_type: ReuseType::ContentUpdate,
                    confidence_score: 0.8,
                    position_adjustment: 0,
                },
            ),
        ]);

        let (materialized_map, replacements) =
            parser.collect_materializable_reuse(&old_tree, &new_tree, &reuse_map);
        assert_eq!(materialized_map.len(), 1);
        assert!(matches!(
            materialized_map.get(&1).map(|strategy| &strategy.reuse_type),
            Some(ReuseType::Direct)
        ));
        assert!(!materialized_map.contains_key(&3));
        assert_eq!(
            parser.materialize_advanced_reuse_tree(&new_tree, &replacements),
            new_tree
        );
    }

'''
parser = parser.replace(unit_anchor, unit_test + unit_anchor, 1)
PARSER.write_text(parser, encoding="utf-8")

accuracy = ACCURACY.read_text(encoding="utf-8")
accuracy_import = "use perl_parser::incremental_advanced_reuse::ReuseConfig;\n"
if accuracy.count(accuracy_import) != 1:
    raise SystemExit("accuracy import anchor drifted")
accuracy = accuracy.replace(
    accuracy_import,
    "use perl_parser::incremental_advanced_reuse::{ReuseConfig, ReuseType};\n",
    1,
)

advanced_requirement = '''        if !incremental.used_advanced_reuse() {
            return Err(format!(
                "{expectation_id}: edit must take the advanced incremental-reuse path"
            )
            .into());
        }
'''
if accuracy.count(advanced_requirement) != 1:
    raise SystemExit("generic advanced-path requirement drifted")
accuracy = accuracy.replace(advanced_requirement, "", 1)

scope_start = accuracy.index("fn reuse_analysis_is_scoped_to_the_last_parse()")
scope_end = accuracy.index("\n#[test]\n", scope_start)
scope = accuracy[scope_start:scope_end]
scope_parser = "    let mut incremental = IncrementalParserV2::new();\n"
scope_configured = '''    let config = ReuseConfig { min_confidence: 0.2, ..ReuseConfig::default() };
    let mut incremental = IncrementalParserV2::with_reuse_config(config);
'''
if scope.count(scope_parser) != 1:
    raise SystemExit("analysis-scope parser anchor drifted")
scope = scope.replace(scope_parser, scope_configured, 1)
accuracy = accuracy[:scope_start] + scope + accuracy[scope_end:]

materialization_test_anchor = (
    "#[test]\nfn rejected_advanced_analysis_is_not_exposed_as_last_parse() -> TestResult {\n"
)
if accuracy.count(materialization_test_anchor) != 1:
    raise SystemExit("materialization integration-test anchor drifted")
materialization_test = '''#[test]
fn selected_advanced_reuse_contains_only_materialized_subtrees() -> TestResult {
    let config = ReuseConfig { min_confidence: 0.2, ..ReuseConfig::default() };
    let mut incremental = IncrementalParserV2::with_reuse_config(config);
    let source = concat!(
        "my $before = 1;\n",
        "my $value = 20;\n",
        "my $after = 3;\n",
    );
    incremental.parse(source)?;
    let edited = apply_incremental_edit(
        &mut incremental,
        source,
        "$value = 20",
        "$value = 200",
        "materialized-advanced-reuse",
    )?;
    let incremental_ast = incremental.parse(&edited)?;
    assert_incremental_outcome(
        &incremental,
        &incremental_ast,
        "materialized-advanced-reuse",
    )?;
    if !incremental.used_advanced_reuse() {
        return Err("the low-threshold proof must select materialized advanced reuse".into());
    }
    let analysis = incremental
        .get_last_reuse_analysis()
        .ok_or("selected advanced reuse must expose its accepted analysis")?;
    if analysis.reuse_map.is_empty() {
        return Err("selected advanced reuse must materialize at least one old subtree".into());
    }
    if analysis.reuse_map.values().any(|strategy| {
        !matches!(strategy.reuse_type, ReuseType::Direct | ReuseType::PositionShift)
    }) {
        return Err("selected advanced reuse exposed a non-materializable strategy".into());
    }
    let fresh_ast = Parser::new(&edited).parse()?;
    assert_ast_equivalent(
        &incremental_ast,
        &fresh_ast,
        "materialized advanced reuse",
    )?;
    Ok(())
}

'''
accuracy = accuracy.replace(
    materialization_test_anchor,
    materialization_test + materialization_test_anchor,
    1,
)
ACCURACY.write_text(accuracy, encoding="utf-8")

Path(__file__).unlink()
