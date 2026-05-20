use crate::loading::parse_file;
use crate::metadata::Section;
use anyhow::Result;
use std::path::Path;

fn has_hidden_or_private_component(dir: &Path, path: &Path) -> bool {
    path.strip_prefix(dir).ok().is_some_and(|relative| {
        relative.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|part| part.starts_with('.') || part.starts_with('_'))
        })
    })
}

pub fn parse_dir(dir: &Path) -> Result<Vec<Section>> {
    let mut all = Vec::new();
    let pattern = format!("{}/**/*.txt", dir.display());

    for entry in glob::glob(&pattern)? {
        let path = entry?;
        if has_hidden_or_private_component(dir, &path) {
            continue;
        }
        all.extend(parse_file(&path)?);
    }

    all.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.id.cmp(&b.id)));
    Ok(all)
}
