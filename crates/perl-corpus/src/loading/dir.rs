use crate::loading::parse_file;
use crate::metadata::Section;
use anyhow::Result;
use std::path::{Component, Path};

fn is_hidden_or_private(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => {
            let value = name.to_string_lossy();
            value.starts_with('.') || value.starts_with('_')
        }
        _ => false,
    })
}

pub fn parse_dir(dir: &Path) -> Result<Vec<Section>> {
    let mut all = Vec::new();
    let pattern = format!("{}/**/*.txt", dir.display());

    for entry in glob::glob(&pattern)? {
        let path = entry?;
        if is_hidden_or_private(&path) {
            continue;
        }
        all.extend(parse_file(&path)?);
    }

    all.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.id.cmp(&b.id)));
    Ok(all)
}
