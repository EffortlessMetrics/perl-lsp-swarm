//! Public-parser regression for repeated copying of owned fat-arrow operands.
#![allow(clippy::print_stderr)] // Measured allocator work is deliberate test output.

#[path = "support/resource_counter.rs"]
mod resource_counter;

use perl_parser_core::{
    Node, NodeKind, ParseBudget, ParseCoreDimension, ParseStopCause, Parser, ParserConfigIdentity,
};
use resource_counter::{CountingAllocator, measure};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn nested_source(depth: usize, width: usize) -> String {
    let mut source = String::from("my $h = ");
    for level in 0..depth {
        source.push_str(&format!("(key{level} => "));
    }
    source.push('[');
    for number in 0..width {
        if number != 0 {
            source.push(',');
        }
        source.push_str(&number.to_string());
    }
    source.push(']');
    source.extend(std::iter::repeat_n(')', depth));
    source.push_str("; my $after = 7;");
    source
}

fn nested_key_source(depth: usize, width: usize) -> String {
    let mut source = String::from("my $h = ");
    source.extend(std::iter::repeat_n('(', depth));
    source.push('[');
    for number in 0..width {
        if number != 0 {
            source.push(',');
        }
        source.push_str(&number.to_string());
    }
    source.push(']');
    for _ in 0..depth {
        source.push_str(" => 0)");
    }
    source.push_str("; my $after = 7;");
    source
}

fn initializer(ast: &Node) -> Result<&Node, String> {
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected a program".into());
    };
    assert_eq!(statements.len(), 2, "the following declaration must survive");
    let NodeKind::VariableDeclaration { variable, initializer: Some(after), .. } =
        &statements[1].kind
    else {
        return Err("expected the following initialized declaration".into());
    };
    assert!(
        matches!(&variable.kind, NodeKind::Variable { name, sigil } if name == "after" && sigil == "$")
    );
    assert!(matches!(&after.kind, NodeKind::Number { value } if value == "7"));
    let NodeKind::VariableDeclaration { initializer: Some(value), .. } = &statements[0].kind else {
        return Err("expected an initialized lexical".into());
    };
    Ok(value)
}

fn assert_nested_shape(ast: &Node, source: &str, depth: usize, width: usize) -> Result<(), String> {
    let mut value = initializer(ast)?;
    for level in 0..depth {
        let key = format!("key{level}");
        let start = source.find(&format!("({key} => ")).ok_or("missing generated opener")?;
        let end = source.find("; my $after").ok_or("missing generated trailer")? - level;
        assert_eq!((value.location.start, value.location.end), (start, end));
        let NodeKind::HashLiteral { pairs } = &value.kind else {
            return Err(format!("expected a nested hash at level {level}"));
        };
        assert_eq!(pairs.len(), 1);
        assert!(
            matches!(&pairs[0].0.kind, NodeKind::String { value, interpolated: false } if value == &key)
        );
        assert_eq!(pairs[0].0.location.start, start + 1);
        assert_eq!(pairs[0].0.location.end, start + 1 + key.len());
        value = &pairs[0].1;
    }
    let NodeKind::ArrayLiteral { elements } = &value.kind else {
        return Err("expected the leaf array".into());
    };
    assert_eq!(elements.len(), width);
    for (number, element) in elements.iter().enumerate() {
        assert!(
            matches!(&element.kind, NodeKind::Number { value } if value == &number.to_string())
        );
        assert_eq!(&source[element.location.start..element.location.end], number.to_string());
    }
    Ok(())
}

#[test]
fn nested_fat_arrow_lists_do_not_recopy_the_wide_leaf() -> Result<(), String> {
    let mut observations = Vec::new();
    for depth in [4, 8, 16] {
        let source = nested_source(depth, 256);
        let (output, work) = measure(|| Parser::new(&source).parse_with_recovery());
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        assert!(output.stop_cause().is_none());
        assert_nested_shape(&output.ast, &source, depth, 256)?;
        assert!(
            work.calls > 0 && work.requested_bytes > 0,
            "allocator instrument must observe work"
        );
        eprintln!("depth={depth} calls={} requested_bytes={}", work.calls, work.requested_bytes);
        observations.push(work);
    }
    // The fixed 256-node leaf dominates construction. Twelve extra two-node
    // wrappers must not duplicate that leaf at each ancestor. This generous
    // ratio leaves room for lexer/parser scaffolding while killing the old
    // whole-subtree cloning path; it is independent of wall-clock scheduling.
    assert!(observations[2].calls < 2 * observations[0].calls, "{observations:?}");
    assert!(
        observations[2].requested_bytes < 2 * observations[0].requested_bytes,
        "{observations:?}"
    );
    Ok(())
}

#[test]
fn comma_only_and_odd_fat_arrow_lists_retain_every_operand() -> Result<(), String> {
    for source in ["my $h = (1, 2, 3); my $after = 7;", "my $h = (a => 1, 2); my $after = 7;"] {
        let output = Parser::new(source).parse_with_recovery();
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        let NodeKind::ArrayLiteral { elements } = &initializer(&output.ast)?.kind else {
            return Err("an odd or comma-only list must stay an array".into());
        };
        assert_eq!(elements.len(), 3);
        assert!(
            matches!(&elements[1].kind, NodeKind::Number { value } if value == if source.contains("=>") { "1" } else { "2" })
        );
        assert!(
            matches!(&elements[2].kind, NodeKind::Number { value } if value == if source.contains("=>") { "2" } else { "3" })
        );
    }
    Ok(())
}

#[test]
fn adjacent_pairs_keep_order_values_and_spans() -> Result<(), String> {
    let source = "my $h = (alpha => 1, beta => 2, gamma => 3); my $after = 7;";
    let output = Parser::new(source).parse_with_recovery();
    assert!(output.diagnostics.is_empty());
    let value = initializer(&output.ast)?;
    assert_eq!((value.location.start, value.location.end), (8, 43));
    let NodeKind::HashLiteral { pairs } = &value.kind else {
        return Err("expected three ordered pairs".into());
    };
    assert_eq!(pairs.len(), 3);
    for ((key, value), (name, number, start, end, value_start)) in pairs.iter().zip([
        ("alpha", "1", 9, 14, 18),
        ("beta", "2", 21, 25, 29),
        ("gamma", "3", 32, 37, 41),
    ]) {
        assert!(
            matches!(&key.kind, NodeKind::String { value, interpolated: false } if value == name)
        );
        assert!(matches!(&value.kind, NodeKind::Number { value } if value == number));
        assert_eq!((key.location.start, key.location.end), (start, end));
        assert_eq!((value.location.start, value.location.end), (value_start, value_start + 1));
    }
    assert_eq!(output.budget_usage.nodes_constructed, 16);
    assert_eq!(output.budget_usage.tokens_consumed, 22);
    let mut budget = ParseBudget::default();
    budget.max_nodes_constructed = 10;
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    let refused = Parser::with_production_config(source, config).parse_with_recovery();
    assert_eq!(
        refused.stop_cause(),
        Some(ParseStopCause::CoreBudgetExhausted {
            dimension: ParseCoreDimension::NodesConstructed,
            limit: 10,
            usage: 10,
        })
    );
    Ok(())
}

#[test]
fn missing_closer_retains_the_recovered_pairs_and_next_declaration() -> Result<(), String> {
    let source = "my $h = (alpha => [1,2], beta => 3; my $after = 7;";
    let output = Parser::new(source).parse_with_recovery();
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        format!("{:?}", output.diagnostics[0]),
        "Recovered { site: ArgList, kind: InsertedCloser, location: 34 }"
    );
    assert!(output.stop_cause().is_none());
    let value = initializer(&output.ast)?;
    assert_eq!((value.location.start, value.location.end), (8, 32));
    let NodeKind::HashLiteral { pairs } = &value.kind else {
        return Err("expected the recovered two-pair list".into());
    };
    assert_eq!(pairs.len(), 2);
    assert!(matches!(&pairs[1].0.kind, NodeKind::String { value, .. } if value == "beta"));
    assert!(matches!(&pairs[1].1.kind, NodeKind::Number { value } if value == "3"));
    assert_eq!(output.budget_usage.nodes_constructed, 15);
    assert_eq!(output.budget_usage.tokens_consumed, 21);
    Ok(())
}

#[test]
fn even_comma_only_list_stays_an_array() -> Result<(), String> {
    let source = "my $h = (1, 2, 3, 4); my $after = 7;";
    let output = Parser::new(source).parse_with_recovery();
    assert!(output.diagnostics.is_empty());
    let NodeKind::ArrayLiteral { elements } = &initializer(&output.ast)?.kind else {
        return Err("comma-only even list must remain an array".into());
    };
    assert_eq!(elements.len(), 4);
    for (index, node) in elements.iter().enumerate() {
        assert!(
            matches!(&node.kind, NodeKind::Number { value } if value == &(index + 1).to_string())
        );
    }
    Ok(())
}

#[test]
fn nested_fat_arrow_keys_do_not_recopy_the_wide_leaf() -> Result<(), String> {
    let mut observations = Vec::new();
    for depth in [4, 8, 16] {
        let source = nested_key_source(depth, 256);
        let (output, work) = measure(|| Parser::new(&source).parse_with_recovery());
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        assert!(output.stop_cause().is_none());
        let mut key = initializer(&output.ast)?;
        let outer_end = source.find("; my $after").ok_or("missing generated trailer")?;
        for level in 0..depth {
            let end = outer_end - 6 * level;
            assert_eq!((key.location.start, key.location.end), (8 + level, end));
            let NodeKind::HashLiteral { pairs } = &key.kind else {
                return Err("expected the nested key hash".into());
            };
            assert_eq!(pairs.len(), 1);
            assert!(matches!(&pairs[0].1.kind, NodeKind::Number { value } if value == "0"));
            assert_eq!((pairs[0].1.location.start, pairs[0].1.location.end), (end - 2, end - 1));
            key = &pairs[0].0;
        }
        let NodeKind::ArrayLiteral { elements } = &key.kind else {
            return Err("expected the wide key leaf".into());
        };
        assert_eq!(elements.len(), 256);
        for (number, element) in elements.iter().enumerate() {
            assert!(
                matches!(&element.kind, NodeKind::Number { value } if value == &number.to_string())
            );
            assert_eq!(&source[element.location.start..element.location.end], number.to_string());
        }
        assert!(work.calls > 0 && work.requested_bytes > 0);
        eprintln!(
            "key_depth={depth} calls={} requested_bytes={}",
            work.calls, work.requested_bytes
        );
        observations.push(work);
    }
    assert!(observations[2].calls < 2 * observations[0].calls, "{observations:?}");
    assert!(
        observations[2].requested_bytes < 2 * observations[0].requested_bytes,
        "{observations:?}"
    );
    Ok(())
}
