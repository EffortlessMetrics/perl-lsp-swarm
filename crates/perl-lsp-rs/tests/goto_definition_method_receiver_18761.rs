//! Regression tests for #18761: method definition and hover at call sites.
//!
//! Four navigation shapes from the issue's probe, each pinned at the LSP
//! surface:
//!
//! 1. a typed accessor chain (`$self->order->total_cents`, `has order` with
//!    `isa => 'Store::Order'`) resolves cross-file to the accessor's class —
//!    even though the caller's own package declares a same-named sub
//!    (Receipt.pm:21 shape);
//! 2. the same chain without `isa` refuses honestly (`[]`) instead of
//!    answering the enclosing same-named sub, and a sub-param receiver
//!    (`my ($self, $product) = @_; $product->in_stock`) refuses too instead of
//!    attributing the method to the caller's package;
//! 3. a cursor on the `->` arrow bytes of an unresolvable call refuses — the
//!    arrow names the call, so the enclosing-sub leak (#18110) must not ship;
//! 4. heredoc bodies gate through the source-region index (definition null,
//!    like hover already was), while the identical call in real code resolves.
//!
//! Plus the hover slice: a constructor-bound receiver (`my $inv =
//! Inventory->new`) now reaches the inherited-method card path, while an
//! untyped receiver's hover stays the generic token card (slice boundary
//! against #9047's richer untyped cards).

mod support;

use std::time::Duration;

use serde_json::{Value, json};
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ORDER_PM: &str = r#"package Store::Order;
use Moo;

has items => (is => 'ro', isa => 'ArrayRef');

sub total_cents {
    my ($self) = @_;
    return 4210;
}

sub add_item {
    my ($self, $product) = @_;
    return $product->in_stock;
}

sub patrol {
    my ($self) = @_;
    return $self->patrol('again');
}

1;
"#;

const RECEIPT_PM_TYPED: &str = r#"package Store::Receipt;
use Moo;

has order => (is => 'ro', isa => 'Store::Order');

sub total_cents {
    my ($self) = @_;
    return $self->order->total_cents;
}

sub render {
    my ($self) = @_;
    my $text = <<"END_RECEIPT";
Total: format_money($self->total_cents)
END_RECEIPT
    return $self->total_cents . $text;
}

1;
"#;

const RECEIPT_PM_UNTYPED: &str = r#"package Store::Receipt;
use Moo;

has order => (is => 'ro');

sub total_cents {
    my ($self) = @_;
    return $self->order->total_cents;
}

1;
"#;

const INVENTORY_PM: &str = r#"package Store::Inventory;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub in_stock {
    my ($self) = @_;
    return 1;
}

sub lookup {
    my ($self) = @_;
    return $self->in_stock;
}

1;
"#;

const HOVER_INVENTORY_PM: &str = r#"package Inventory;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub restock {
    my ($self, $count) = @_;
    return $count;
}

sub check {
    my ($self) = @_;
    my $inv = Inventory->new;
    return $inv->restock(5);
}

1;
"#;

fn pos_of(source: &str, needle: &str, occurrence: usize) -> Result<(u32, u32), String> {
    let mut seen = 0;
    for (line_idx, line) in source.lines().enumerate() {
        let mut from = 0;
        while let Some(idx) = line[from..].find(needle) {
            let abs = from + idx;
            if seen == occurrence {
                return Ok((line_idx as u32, abs as u32));
            }
            seen += 1;
            from = abs + 1;
        }
    }
    Err(format!("needle {needle:?} occurrence {occurrence} not found in:\n{source}"))
}

fn describe(result: &Value) -> String {
    match result {
        Value::Null => "null".to_string(),
        Value::Array(items) if items.is_empty() => "[]".to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let uri = item.get("uri").and_then(Value::as_str).unwrap_or("<?>");
                let point = |side: &str| {
                    item.pointer(&format!("/range/{side}"))
                        .map(|p| {
                            format!(
                                "{}:{}",
                                p.get("line").and_then(Value::as_u64).unwrap_or(99_999),
                                p.get("character").and_then(Value::as_u64).unwrap_or(99_999)
                            )
                        })
                        .unwrap_or_else(|| "?".into())
                };
                format!("{uri} @{}-{}", point("start"), point("end"))
            })
            .collect::<Vec<_>>()
            .join("; "),
        other => format!("{other}"),
    }
}

fn assert_refusal(result: &Value, label: &str) {
    let clean = result.is_null() || result.as_array().is_some_and(|items| items.is_empty());
    assert!(clean, "{label} must refuse without a target, got: {}", describe(result));
}

fn goto_def(
    harness: &mut LspHarness,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(harness.request_with_timeout(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
        Duration::from_secs(3),
    )?)
}

fn hover_at(
    harness: &mut LspHarness,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(harness.request_with_timeout(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        }),
        Duration::from_secs(3),
    )?)
}

fn hover_markdown(result: &Value) -> String {
    result
        .get("contents")
        .and_then(|contents| contents.get("value"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

fn wait_ready(
    harness: &mut LspHarness,
    workspace: &TempWorkspace,
    symbol: &str,
    relative: &str,
) -> TestResult {
    harness.wait_for_symbol(symbol, Some(&workspace.uri(relative)), Duration::from_secs(5))?;
    Ok(())
}

fn open_files(
    harness: &mut LspHarness,
    workspace: &TempWorkspace,
    files: &[(&str, &str)],
) -> TestResult {
    for (relative, source) in files {
        harness.open(&workspace.uri(relative), source)?;
    }
    harness.barrier();
    Ok(())
}

/// Shape 1 + Receipt.pm:21: the typed chain resolves to `Store::Order`'s
/// `total_cents`, not to the enclosing same-named `Store::Receipt::total_cents`.
#[test]
fn typed_chain_resolves_cross_file_to_accessor_isa_class() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Store/Order.pm", ORDER_PM)?;
    workspace.write("lib/Store/Receipt.pm", RECEIPT_PM_TYPED)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(
        &mut harness,
        &workspace,
        &[("lib/Store/Order.pm", ORDER_PM), ("lib/Store/Receipt.pm", RECEIPT_PM_TYPED)],
    )?;
    wait_ready(&mut harness, &workspace, "total_cents", "lib/Store/Order.pm")?;

    let receipt_uri = workspace.uri("lib/Store/Receipt.pm");
    // The chain selector inside `Store::Receipt::total_cents`.
    let (line, character) = pos_of(RECEIPT_PM_TYPED, "->total_cents", 0)?;
    let result = goto_def(&mut harness, &receipt_uri, line, character + 2)?;

    let locations = result.as_array().ok_or_else(|| {
        format!("expected a location array for the typed chain, got: {}", describe(&result))
    })?;
    assert!(
        !locations.is_empty(),
        "a typed accessor chain must resolve cross-file, got: {}",
        describe(&result)
    );
    let first = locations.first().ok_or("non-empty locations")?;
    let target_uri = first.get("uri").and_then(Value::as_str).ok_or("location uri")?;
    assert!(
        target_uri.ends_with("Store/Order.pm"),
        "typed chain must land in Order.pm, got: {}",
        describe(&result)
    );
    Ok(())
}

/// Shape 2a: without `isa`, the same chain refuses — never the enclosing
/// same-named `sub total_cents` (the old current-package fallback target).
#[test]
fn untyped_chain_refuses_instead_of_same_named_enclosing_sub() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Store/Order.pm", ORDER_PM)?;
    workspace.write("lib/Store/Receipt.pm", RECEIPT_PM_UNTYPED)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(
        &mut harness,
        &workspace,
        &[("lib/Store/Order.pm", ORDER_PM), ("lib/Store/Receipt.pm", RECEIPT_PM_UNTYPED)],
    )?;
    wait_ready(&mut harness, &workspace, "total_cents", "lib/Store/Order.pm")?;

    let receipt_uri = workspace.uri("lib/Store/Receipt.pm");
    let (line, character) = pos_of(RECEIPT_PM_UNTYPED, "->total_cents", 0)?;
    let result = goto_def(&mut harness, &receipt_uri, line, character + 2)?;
    assert_refusal(
        &result,
        "an untyped chain (has order without isa) must not answer the local same-named sub",
    );
    Ok(())
}

/// Shape 2b: a sub-param receiver (`$product` from `@_`) refuses on the token —
/// and on the arrow bytes too, where the #18110 regex found nothing (#18761
/// step 3: the arrow names the call).
#[test]
fn sub_param_receiver_refuses_on_token_and_on_arrow() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Store/Order.pm", ORDER_PM)?;
    workspace.write("lib/Store/Inventory.pm", INVENTORY_PM)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(
        &mut harness,
        &workspace,
        &[("lib/Store/Order.pm", ORDER_PM), ("lib/Store/Inventory.pm", INVENTORY_PM)],
    )?;
    wait_ready(&mut harness, &workspace, "in_stock", "lib/Store/Inventory.pm")?;

    let order_uri = workspace.uri("lib/Store/Order.pm");
    let (line, character) = pos_of(ORDER_PM, "return $product->in_stock", 0)?;

    let on_token =
        goto_def(&mut harness, &order_uri, line, character + "return $product->".len() as u32)?;
    assert_refusal(
        &on_token,
        "a sub-param receiver must refuse even though Store::Inventory::in_stock exists",
    );

    // The `>` of the arrow: `return $product->in_stock` — the arrow starts
    // after `return $product`.
    let on_arrow =
        goto_def(&mut harness, &order_uri, line, character + "return $product".len() as u32 + 1)?;
    assert_refusal(
        &on_arrow,
        "the arrow bytes of an unresolvable call must refuse without the enclosing sub",
    );
    Ok(())
}

/// Control for the same-name exemption: plain `$self->patrol` recursion still
/// navigates to its own declaration — the refusal must stay chain-scoped.
#[test]
fn same_name_recursion_with_variable_receiver_still_navigates() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Store/Order.pm", ORDER_PM)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(&mut harness, &workspace, &[("lib/Store/Order.pm", ORDER_PM)])?;
    wait_ready(&mut harness, &workspace, "patrol", "lib/Store/Order.pm")?;

    let order_uri = workspace.uri("lib/Store/Order.pm");
    let (line, character) = pos_of(ORDER_PM, "$self->patrol", 0)?;
    let result = goto_def(&mut harness, &order_uri, line, character + "$self->".len() as u32)?;

    let locations = result.as_array().ok_or_else(|| {
        format!("expected a location array for plain recursion, got: {}", describe(&result))
    })?;
    assert!(
        !locations.is_empty(),
        "same-name recursion for a plain variable receiver must keep navigating, got: {}",
        describe(&result)
    );
    let first = locations.first().ok_or("non-empty locations")?;
    let start_line = first.pointer("/range/start/line").and_then(Value::as_u64).ok_or("range")?;
    let patrol_decl_line = ORDER_PM
        .lines()
        .position(|l| l.trim().starts_with("sub patrol"))
        .ok_or("patrol decl line")? as u64;
    assert_eq!(
        start_line,
        patrol_decl_line,
        "recursion must land on `sub patrol` itself, got: {}",
        describe(&result)
    );
    Ok(())
}

/// Shape 3 (issue defect 5): the heredoc body gates through the source-region
/// index — definition is null there, hover stays null, and the identical call
/// in real code still resolves.
#[test]
fn heredoc_cursor_refuses_while_same_call_in_code_resolves() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Store/Order.pm", ORDER_PM)?;
    workspace.write("lib/Store/Receipt.pm", RECEIPT_PM_TYPED)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(
        &mut harness,
        &workspace,
        &[("lib/Store/Order.pm", ORDER_PM), ("lib/Store/Receipt.pm", RECEIPT_PM_TYPED)],
    )?;
    wait_ready(&mut harness, &workspace, "total_cents", "lib/Store/Order.pm")?;

    let receipt_uri = workspace.uri("lib/Store/Receipt.pm");
    // In-heredoc call text: `format_money($self->total_cents)` on the heredoc
    // line. Definition must refuse (null) and hover must refuse too.
    let heredoc_line = RECEIPT_PM_TYPED
        .lines()
        .position(|l| l.starts_with("Total: format_money("))
        .ok_or("heredoc content line not found")? as u32;
    let heredoc_col = RECEIPT_PM_TYPED
        .lines()
        .nth(heredoc_line as usize)
        .and_then(|l| l.find("$self->total_cents"))
        .ok_or("heredoc call not found")? as u32
        + "$self->".len() as u32;

    let def_in_heredoc = goto_def(&mut harness, &receipt_uri, heredoc_line, heredoc_col)?;
    assert!(
        def_in_heredoc.is_null(),
        "definition inside a heredoc body must be null, got: {}",
        describe(&def_in_heredoc)
    );
    let hover_in_heredoc = hover_at(&mut harness, &receipt_uri, heredoc_line, heredoc_col)?;
    assert!(
        hover_in_heredoc.is_null(),
        "hover inside a heredoc body must stay null, got: {}",
        describe(&hover_in_heredoc)
    );

    // Control: the identical selector in real code resolves to the local sub.
    let code_line = RECEIPT_PM_TYPED
        .lines()
        .position(|l| l.contains("return $self->total_cents . $text;"))
        .ok_or("real-code call line not found")? as u32;
    let code_col = RECEIPT_PM_TYPED
        .lines()
        .nth(code_line as usize)
        .and_then(|l| l.find("$self->total_cents"))
        .ok_or("real-code call not found")? as u32
        + "$self->".len() as u32;
    let def_in_code = goto_def(&mut harness, &receipt_uri, code_line, code_col)?;
    let locations = def_in_code.as_array().ok_or_else(|| {
        format!("expected locations for the real-code call, got: {}", describe(&def_in_code))
    })?;
    assert!(
        !locations.is_empty(),
        "the identical call in real code must still resolve, got: {}",
        describe(&def_in_code)
    );
    Ok(())
}

/// Hover slice: a constructor-bound receiver reaches the inherited-method card
/// path; an untyped receiver stays on the generic token card (slice boundary
/// against #9047).
#[test]
fn hover_on_constructor_bound_receiver_shows_signature_card() -> TestResult {
    let workspace = TempWorkspace::new()?;
    workspace.write("lib/Inventory.pm", HOVER_INVENTORY_PM)?;

    let mut harness = LspHarness::new();
    harness.initialize_with_root(&workspace.root_uri, None)?;
    open_files(&mut harness, &workspace, &[("lib/Inventory.pm", HOVER_INVENTORY_PM)])?;
    wait_ready(&mut harness, &workspace, "restock", "lib/Inventory.pm")?;

    let inventory_uri = workspace.uri("lib/Inventory.pm");
    let (line, character) = pos_of(HOVER_INVENTORY_PM, "$inv->restock", 0)?;
    let on_method =
        hover_at(&mut harness, &inventory_uri, line, character + "$inv->".len() as u32)?;
    let typed_markdown = hover_markdown(&on_method);
    assert!(
        !typed_markdown.is_empty(),
        "hover on a constructor-bound receiver call must answer, got null"
    );
    assert_ne!(
        typed_markdown.trim(),
        "**Perl**: `restock`",
        "constructor-bound receiver hover must not be the generic token card, got: {typed_markdown:?}"
    );
    assert!(
        typed_markdown.contains("restock"),
        "the card must still name the method, got: {typed_markdown:?}"
    );

    // Slice boundary: the untyped receiver (`$self->in_stock` in Inventory.pm's
    // lookup) keeps the generic card — richer untyped cards are #9047.
    let workspace2 = TempWorkspace::new()?;
    workspace2.write("lib/Store/Inventory.pm", INVENTORY_PM)?;
    let mut harness2 = LspHarness::new();
    harness2.initialize_with_root(&workspace2.root_uri, None)?;
    open_files(&mut harness2, &workspace2, &[("lib/Store/Inventory.pm", INVENTORY_PM)])?;
    wait_ready(&mut harness2, &workspace2, "in_stock", "lib/Store/Inventory.pm")?;

    let inventory2_uri = workspace2.uri("lib/Store/Inventory.pm");
    let (line2, character2) = pos_of(INVENTORY_PM, "$self->in_stock", 0)?;
    let untyped =
        hover_at(&mut harness2, &inventory2_uri, line2, character2 + "$self->".len() as u32)?;
    let untyped_markdown = hover_markdown(&untyped);
    assert!(
        untyped_markdown.contains("in_stock"),
        "untyped receiver hover must still answer something about the token, got: {untyped_markdown:?}"
    );
    Ok(())
}
