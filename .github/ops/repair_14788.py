from pathlib import Path
import re

root = Path('target')


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding='utf-8')
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{path}: expected one replacement, found {count}')
    path.write_text(text.replace(old, new), encoding='utf-8')


lower = root / 'crates/perl-parser-core/src/pir/lower.rs'
replace_once(
    lower,
    '''            HirKind::DynamicBoundary(boundary) => {
                self.lower_dynamic_boundary(
                    item,
                    map_boundary_kind(boundary.kind),
                    boundary.reason.clone(),
                );
            }
''',
    '''            HirKind::DynamicBoundary(boundary) => {
                self.lower_dynamic_boundary(item, boundary.kind, boundary.reason.clone());
            }
''',
)
replace_once(
    lower,
    '''    fn lower_dynamic_boundary(
        &mut self,
        item: &HirItem,
        kind: PirDynamicBoundaryKind,
        reason: String,
    ) -> PirId {
        let anchor = PirSourceAnchor::dynamic_boundary(item.range, item.id);
        let id = self.push_node(
            item,
            anchor,
            PirOperation::DynamicBoundary { kind, reason },
            PirContext::Unknown,
            None,
        );
''',
    '''    fn lower_dynamic_boundary(
        &mut self,
        item: &HirItem,
        hir_kind: DynamicBoundaryKind,
        reason: String,
    ) -> PirId {
        let kind = map_boundary_kind(hir_kind);
        let anchor = PirSourceAnchor::dynamic_boundary(item.range, item.id);
        let operation = PirOperation::DynamicBoundary { kind, reason };
        let id = if matches!(
            hir_kind,
            DynamicBoundaryKind::TiedPlaceBinding | DynamicBoundaryKind::TiedPlaceRelease
        ) {
            // Tie and untie are the post-order boundary forms in flat HIR:
            // their operands precede the hidden dispatch boundary. When one is
            // nested, splice that boundary before the already-lowered consumer
            // without changing adjacency for the pre-order boundary families.
            self.push_node_maybe_operand(item, anchor, operation, None)
        } else {
            self.push_node(item, anchor, operation, PirContext::Unknown, None)
        };
''',
)

tests = root / 'crates/perl-parser-core/tests/hir_lowering_tie_boundary.rs'
text = tests.read_text(encoding='utf-8')
pattern = re.compile(
    r'/// Known limitation, pinned deliberately:.*?\n}\n\n/// Honest claim boundary',
    re.DOTALL,
)
replacement = '''/// A tie used as an initializer is an operand of the assignment. Flat PIR
/// must evaluate the tie operands, reach the hidden-dispatch boundary, and only
/// then reach the consuming `Assign` node.
#[test]
fn nested_tie_boundary_precedes_its_initializer_consumer_in_flat_pir() -> TestResult {
    let mut parser =
        Parser::new("package Main;\\nmy $obj = tie %hash, 'Tie::StdHash', build_args();\\n");
    let output = parser.parse_with_recovery();
    assert_eq!(output.diagnostics.len(), 0, "the nested tie form must parse cleanly");
    let graph = lower_hir(&lower_ast(&output.ast));

    let call_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::Call { callee: PirCallee::Named { name, .. }, .. } if name == "build_args")
            .then_some(node.id)
    }));
    let boundary_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::DynamicBoundary { .. }).then_some(node.id)
    }));
    let assign_id = must_some(
        graph
            .nodes
            .iter()
            .find_map(|node| matches!(&node.operation, PirOperation::Assign).then_some(node.id)),
    );

    let order = fallthrough_order(&graph);
    let call_step = must_some(order.iter().position(|id| *id == call_id));
    let boundary_step = must_some(order.iter().position(|id| *id == boundary_id));
    let assign_step = must_some(order.iter().position(|id| *id == assign_id));
    assert!(
        call_step < boundary_step && boundary_step < assign_step,
        "nested tie order must be operands -> boundary -> consumer; chain {order:?}"
    );
    Ok(())
}

/// `untie` has the same expression-position obligation: release happens before
/// an enclosing assignment consumes its return value.
#[test]
fn nested_untie_boundary_precedes_its_initializer_consumer_in_flat_pir() -> TestResult {
    let mut parser = Parser::new("package Main;\\nmy $released = untie %hash;\\n");
    let output = parser.parse_with_recovery();
    assert_eq!(output.diagnostics.len(), 0, "the nested untie form must parse cleanly");
    let graph = lower_hir(&lower_ast(&output.ast));

    let boundary_id = must_some(graph.nodes.iter().find_map(|node| {
        matches!(&node.operation, PirOperation::DynamicBoundary { .. }).then_some(node.id)
    }));
    let assign_id = must_some(
        graph
            .nodes
            .iter()
            .find_map(|node| matches!(&node.operation, PirOperation::Assign).then_some(node.id)),
    );

    let order = fallthrough_order(&graph);
    let boundary_step = must_some(order.iter().position(|id| *id == boundary_id));
    let assign_step = must_some(order.iter().position(|id| *id == assign_id));
    assert!(
        boundary_step < assign_step,
        "nested untie order must be boundary -> consumer; chain {order:?}"
    );
    Ok(())
}

/// Honest claim boundary'''
text, count = pattern.subn(replacement, text, count=1)
if count != 1:
    raise SystemExit(f'{tests}: expected one limitation-test replacement, found {count}')
tests.write_text(text, encoding='utf-8')

contract = root / 'contracts/compiler/perl_compiler_concepts.v1.toml'
replace_once(
    contract,
    'claim_boundary = "Flat HIR marks the tie site itself with DynamicBoundary(TiedPlaceBinding), emitted after the operands. In a nested expression the flat PIR chain still reaches the consuming operation before the boundary, because dynamic boundaries skip operand splicing (#14803). Canonical PIR-A lowers from the body arenas, where the construct is still an opaque call, so PIR-A carries no tie boundary. Only the binding site is marked: subsequent reads and writes of the tied place remain indistinguishable from ordinary storage. Tie lifecycle, target place identity, hidden TIE* method effects, and capability boundaries are not represented."',
    'claim_boundary = "Flat HIR marks the tie site itself with DynamicBoundary(TiedPlaceBinding), emitted after the operands. When tie is nested in another flat-HIR expression, flat PIR splices that boundary before the consuming operation while preserving the conservative DynamicExit edge. Canonical PIR-A lowers from the body arenas, where the construct is still an opaque call, so PIR-A carries no tie boundary. Only the binding site is marked: subsequent reads and writes of the tied place remain indistinguishable from ordinary storage. Tie lifecycle, target place identity, hidden TIE* method effects, and capability boundaries are not represented."',
)
replace_once(
    contract,
    'claim_boundary = "Flat HIR marks the untie site itself with DynamicBoundary(TiedPlaceRelease), emitted after the target expression. In a nested expression the flat PIR chain still reaches the consuming operation before the boundary (#14803). Canonical PIR-A lowers from the body arenas, where the construct is still an opaque call, so PIR-A carries no untie boundary. Only the release site is marked: the tied place\'s storage character before and after it is not tracked. Tied-place lifetime and hidden UNTIE/DESTROY consequences are not represented."',
    'claim_boundary = "Flat HIR marks the untie site itself with DynamicBoundary(TiedPlaceRelease), emitted after the target expression. When untie is nested in another flat-HIR expression, flat PIR splices that boundary before the consuming operation while preserving the conservative DynamicExit edge. Canonical PIR-A lowers from the body arenas, where the construct is still an opaque call, so PIR-A carries no untie boundary. Only the release site is marked: the tied place\'s storage character before and after it is not tracked. Tied-place lifetime and hidden UNTIE/DESTROY consequences are not represented."',
)
