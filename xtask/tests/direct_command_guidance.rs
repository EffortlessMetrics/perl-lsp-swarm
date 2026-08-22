use std::fs;
use std::path::{Path, PathBuf};

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
    let mut scan_file = |path: &Path| -> Result<(), Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path).map_err(|error| {
            std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
        })?;
        for (line_number, line) in contents.lines().enumerate() {
            if contains_word(line, "rtk") {
                violations.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    line_number + 1,
                    line.trim()
                ));
            }
        }
        Ok(())
    };

    // The tracked root instructions are an operational copy/paste surface too.
    scan_file(&root.join("AGENTS.md"))?;

    for relative_root in operational_doc_roots {
        for entry in WalkDir::new(root.join(relative_root)) {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|ext| ext.to_str()) != Some("md")
            {
                continue;
            }
            scan_file(path)?;
        }
    }

    violations.sort();
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

#[test]
fn contains_word_matches_standalone_case_insensitively() {
    assert!(contains_word("rtk", "rtk"));
    assert!(contains_word("RTK-command", "rtk"));
    assert!(!contains_word("rtk_command", "rtk"));
    assert!(!contains_word("rtk2", "rtk"));
    assert!(!contains_word("myrtk", "rtk"));
}
