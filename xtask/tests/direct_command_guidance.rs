use std::fs;
use std::path::PathBuf;

use walkdir::WalkDir;

fn repository_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn operational_docs_do_not_recommend_retired_command_wrapper()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    // Forensic notes retain historical incident vocabulary; these trees are
    // current operational guidance and copy/paste surfaces.
    let operational_doc_roots = [
        "docs/articles",
        "docs/project",
        "docs/proposals",
        "docs/reference",
        "docs/swarm/source-syncs",
    ];
    let mut violations = Vec::new();

    for relative_root in operational_doc_roots {
        for entry in WalkDir::new(root.join(relative_root)) {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|ext| ext.to_str()) != Some("md")
            {
                continue;
            }

            for (line_number, line) in fs::read_to_string(path)?.lines().enumerate() {
                if contains_word(line, "rtk") {
                    violations.push(format!(
                        "{}:{}: {}",
                        path.strip_prefix(&root).unwrap_or(path).display(),
                        line_number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "operational documentation must use direct commands and must not recommend the retired wrapper:\n{}",
        violations.join("\n")
    );

    Ok(())
}

fn contains_word(line: &str, word: &str) -> bool {
    line.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| token.eq_ignore_ascii_case(word))
}
