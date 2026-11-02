use perl_ast::{
    ForeachBinderView, ForeachSyntheticBinderProbe, Node, NodeKind, SourceLocation,
};

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn var_node(name: &str) -> Node {
    Node::new(
        NodeKind::Variable {
            sigil: "$".to_string(),
            name: name.to_string(),
        },
        loc(0, 1),
    )
}

fn block_node() -> Node {
    Node::new(NodeKind::Block { statements: vec![] }, loc(2, 3))
}

fn foreach_parts(variable: Node) -> (Node, Node, Node, Option<Node>) {
    (
        variable,
        var_node("list"),
        block_node(),
        None,
    )
}

#[test]
fn ordered_binders_reflect_current_single_field_shape() {
    let first = var_node("a");
    let (variable, list, body, continue_block) = foreach_parts(first.clone());
    let view = ForeachBinderView::from_parts(&variable, &list, &body, continue_block.as_ref());
    let binders = view.ordered_binder_nodes();
    assert_eq!(binders.len(), 1);
    assert_eq!(binders[0].kind, first.kind);
}

#[test]
fn structural_test_children_include_every_binder_before_list_and_body() {
    let (variable, list, body, continue_block) = foreach_parts(var_node("i"));
    let view = ForeachBinderView::from_parts(&variable, &list, &body, continue_block.as_ref());
    let children = view.structural_test_children();
    assert_eq!(children.len(), 3);
    assert!(matches!(&children[0].kind, NodeKind::Variable { name, .. } if name == "i"));
    assert!(matches!(&children[1].kind, NodeKind::Variable { name, .. } if name == "list"));
    assert!(matches!(children[2].kind, NodeKind::Block { .. }));
}

#[test]
fn synthetic_second_binder_probe_exposes_cardinality_sensitive_gap() {
    let (variable, list, body, continue_block) = foreach_parts(var_node("left"));
    let view = ForeachBinderView::from_parts(&variable, &list, &body, continue_block.as_ref());
    let extra = var_node("probe");
    let probe = ForeachSyntheticBinderProbe::new(view, &extra);
    assert_eq!(probe.legacy_only_binder_count(), 1);
    assert_eq!(probe.simulated_binder_count(), 2);
    assert_eq!(probe.ordered_binder_nodes().len(), 2);
}

#[test]
fn try_from_kind_accepts_foreach_nodes() {
    let kind = NodeKind::Foreach {
        variable: Box::new(var_node("x")),
        list: Box::new(var_node("list")),
        body: Box::new(block_node()),
        continue_block: None,
    };
    assert!(ForeachBinderView::try_from_kind(&kind).is_some());
    assert!(ForeachBinderView::try_from_kind(&NodeKind::Block { statements: vec![] }).is_none());
}
