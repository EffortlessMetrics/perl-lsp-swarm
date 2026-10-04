//! Request-level preservation of public dependency-manager proof (#17226).
//! Carmel rollout uses a sentinel file and local/lib/perl5, not vendor/lib/perl5.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn initialize_with_project_paths(
    workspace: &TempWorkspace,
    files: &[(&str, &str)],
) -> Result<(LspHarness, String), String> {
    // Explicitly exclude the install base from manual roots. The manager's
    // detected root must reach the real provider despite this project setting.
    workspace.write(".perl-lsp.toml", "[perl]\ninclude_paths = [\"lib\"]\n")?;
    for (path, content) in files {
        workspace.write(path, content)?;
    }
    let mut harness = LspHarness::new_raw();
    harness.initialize_ready(&workspace.root_uri, None)?;
    Ok((harness, workspace.uri("main.pl")))
}

fn module_definition(
    harness: &mut LspHarness,
    main_uri: &str,
    module: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let source = format!("use {module};\n");
    harness.open(main_uri, &source)?;
    harness.barrier();
    let position = source
        .find(module)
        .ok_or("module missing from test source")? as u32;
    Ok(harness.request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 0, "character": position }
        }),
    )?)
}

#[test]
fn carton_root_reaches_module_completion() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let (mut harness, main_uri) = initialize_with_project_paths(
        &workspace,
        &[
            ("cpanfile", "# Carton project marker\n"),
            ("carton.lock", "snapshot\n"),
            (
                "local/lib/perl5/Carton/Only.pm",
                "package Carton::Only;\n1;\n",
            ),
        ],
    )?;
    let source = "use Carton::O";
    harness.open(&main_uri, source)?;
    harness.barrier();
    let result = harness.completion_at(&main_uri, 0, source.len() as u32)?;
    let items = result
        .as_array()
        .or_else(|| result.get("items").and_then(Value::as_array))
        .ok_or_else(|| std::io::Error::other(format!("expected completion list, got {result}")))?;
    assert!(
        items
            .iter()
            .any(|item| item.get("label").and_then(Value::as_str) == Some("Carton::Only")),
        "Carton-detected local module should reach completion, got {items:?}"
    );
    Ok(())
}

#[test]
fn carmel_rollout_root_reaches_module_definition() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let (mut harness, main_uri) = initialize_with_project_paths(
        &workspace,
        &[
            ("cpanfile", "requires 'Carmel::Only';\n"),
            // Upstream rollout touches this FILE. A directory is not a sentinel.
            ("local/.carmel", ""),
            (
                "local/lib/perl5/Carmel/Only.pm",
                "package Carmel::Only;\n1;\n",
            ),
        ],
    )?;
    let result = module_definition(&mut harness, &main_uri, "Carmel::Only")?;
    let locations = result
        .as_array()
        .ok_or_else(|| std::io::Error::other(format!("expected definition array, got {result}")))?;
    let expected_uri = workspace.uri("local/lib/perl5/Carmel/Only.pm");
    assert!(
        locations.iter().any(|location| {
            location.get("uri").and_then(Value::as_str) == Some(expected_uri.as_str())
        }),
        "Carmel rollout module should resolve to {expected_uri}, got {locations:?}"
    );
    Ok(())
}

#[test]
fn carmel_rollout_root_does_not_activate_without_cpanfile() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let (mut harness, main_uri) = initialize_with_project_paths(
        &workspace,
        &[
            ("local/.carmel", ""),
            (
                "local/lib/perl5/Carmel/Only.pm",
                "package Carmel::Only;\n1;\n",
            ),
        ],
    )?;
    let result = module_definition(&mut harness, &main_uri, "Carmel::Only")?;
    assert!(
        result.as_array().is_some_and(Vec::is_empty),
        "rollout path must stay inactive without the declaration, got {result}"
    );
    Ok(())
}

#[test]
fn vendor_path_is_not_inferred_as_carmel_rollout() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let (mut harness, main_uri) = initialize_with_project_paths(
        &workspace,
        &[
            ("cpanfile", "requires 'Carmel::Only';\n"),
            (
                "vendor/lib/perl5/Carmel/Only.pm",
                "package Carmel::Only;\n1;\n",
            ),
        ],
    )?;
    let result = module_definition(&mut harness, &main_uri, "Carmel::Only")?;
    assert!(
        result.as_array().is_some_and(Vec::is_empty),
        "a vendor directory must not infer a Carmel install-base root, got {result}"
    );
    Ok(())
}
