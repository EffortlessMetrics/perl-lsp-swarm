//! Render the DBIx::QuickORM API return matrix from the checked registry (#13374).
//!
//! The registry in `perl-semantic-facts` is the only authority. This task is a
//! deterministic projection of it, so a row can never be edited here without
//! editing the reviewed contract first.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use perl_semantic_facts::framework_adapters::quickorm_api::{
    QUICKORM_API_CASES, QUICKORM_API_RETURN_REGISTRY_VERSION, QUICKORM_FRAMEWORK_NAME,
    QUICKORM_UPSTREAM_COMMIT, QUICKORM_UPSTREAM_REPOSITORY, QUICKORM_UPSTREAM_VERSION,
};
use std::fs;

/// Checked-in projection of the QuickORM API return registry.
pub const MATRIX_PATH: &str = "docs/specs/quickorm-api-return-matrix.md";

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let path = root.join(MATRIX_PATH);
    let generated = render_matrix();

    if check {
        let existing =
            fs::read_to_string(&path).with_context(|| format!("failed to read {MATRIX_PATH}"))?;
        if normalize_newlines(&existing) != generated {
            bail!("{MATRIX_PATH} is stale; run `cargo xtask generate-quickorm-api-matrix`");
        }
        println!("QuickORM API return matrix is up to date: {} rows", QUICKORM_API_CASES.len());
        return Ok(());
    }

    fs::write(&path, generated).with_context(|| format!("failed to write {MATRIX_PATH}"))?;
    println!("Wrote {MATRIX_PATH} with {} rows", QUICKORM_API_CASES.len());
    Ok(())
}

fn render_matrix() -> String {
    let mut output = String::new();
    output.push_str("# DBIx::QuickORM API Return Matrix\n\n");
    output.push_str("Status: generated\n");
    output.push_str("Generator: `cargo xtask generate-quickorm-api-matrix`\n");
    output.push_str("Check: `cargo xtask generate-quickorm-api-matrix --check`\n");
    output.push_str(
        "Authority: `perl-semantic-facts::framework_adapters::quickorm_api` (#13374)\n\n",
    );
    output.push_str(&format!("Registry: `{QUICKORM_API_RETURN_REGISTRY_VERSION}`\n"));
    output.push_str(&format!(
        "Upstream: {QUICKORM_FRAMEWORK_NAME} `{QUICKORM_UPSTREAM_VERSION}` at \
         [`{short}`]({QUICKORM_UPSTREAM_REPOSITORY}/tree/{QUICKORM_UPSTREAM_COMMIT})\n\n",
        short = &QUICKORM_UPSTREAM_COMMIT[..12]
    ));
    output.push_str(
        "This matrix pins, for one exact upstream revision, which QuickORM methods return a \
         handle that preserves the receiver's source and row type parameters, which transform \
         them, and which are row terminals, iterators, plain data, metadata, counts, or side \
         effects. It is reviewed reference data consumed by later type propagation; it is not \
         produced by this repository's own inference and it changes no runtime behavior.\n\n",
    );
    output.push_str(
        "Receiver identity and argument cohort are both load-bearing: the same method name \
         means different things on different receivers, and a large family of handle methods \
         return stored state with no arguments but a refined clone with arguments.\n\n",
    );
    output.push_str("| Case | Package | Method | Receiver | Arguments | Return class | Multiplicity | Type params | Mode | Void | Boundary | Receiver constraints | Evidence |\n");
    output.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );

    for case in QUICKORM_API_CASES {
        output.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | `{}:{}` |\n",
            case.api_case_id,
            case.package,
            case.method,
            case.receiver.as_str(),
            case.arguments.as_str(),
            case.return_class.as_str(),
            case.multiplicity.as_str(),
            case.type_params.as_str(),
            case.mode.as_str(),
            case.void_context.as_str(),
            case.boundary.as_str(),
            render_constraints(case.receiver_constraints),
            case.evidence.file,
            case.evidence.line,
        ));
    }

    output.push_str("\n## Notes\n\n");
    for case in QUICKORM_API_CASES {
        output.push_str(&format!("- `{}`: {}\n", case.api_case_id, case.notes));
    }

    output
}

/// Render the preconditions upstream enforces, so the projection does not make a
/// conditional call boundary look unconditional.
fn render_constraints(constraints: &[&str]) -> String {
    if constraints.is_empty() {
        return "—".to_string();
    }
    constraints.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join("; ")
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}
