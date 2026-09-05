use super::super::super::{LspServer, source_path_from_uri};
use perl_module::{
    no_lib_cancelled_paths_at_offset, resolve_use_lib_paths_from_source,
    resolve_use_lib_paths_from_source_at_offset,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) fn lexical_paths(
    server: &LspServer,
    doc_uri: Option<&str>,
    doc_text: Option<&str>,
    doc_offset: Option<usize>,
    root: &Path,
) -> Vec<String> {
    let Some(text) = doc_text else {
        return Vec::new();
    };

    let file_dir = file_dir(doc_uri);
    if file_dir.is_none() && doc_uri.is_some() {
        tracing::trace!("Effective @INC context failed to resolve doc_uri: {:?}", doc_uri);
    }

    let source_paths = if let Some(offset) = doc_offset {
        resolve_use_lib_paths_from_source_at_offset(text, offset, root, file_dir.as_deref())
    } else {
        resolve_use_lib_paths_from_source(text, root, file_dir.as_deref())
    };

    let (hir_paths, hir_cancelled_paths) = server.cached_hir_use_lib_paths_and_cancelled(
        doc_uri,
        text,
        root,
        file_dir.as_deref(),
        doc_offset,
    );
    let hir_cancelled_keys: HashSet<String> =
        hir_cancelled_paths.iter().map(|path| include_path_key(path)).collect();
    let mut paths = hir_paths;
    for path in source_paths.into_iter().rev() {
        if hir_cancelled_keys.contains(&include_path_key(&path)) {
            continue;
        }
        paths.retain(|existing| existing != &path);
        paths.insert(0, path);
    }

    paths
}

pub(super) fn include_paths_with_cancellations(
    server: &LspServer,
    doc_uri: Option<&str>,
    doc_text: Option<&str>,
    doc_offset: Option<usize>,
    root: &Path,
    raw_include_paths: Vec<String>,
) -> Vec<String> {
    let (Some(offset), Some(text)) = (doc_offset, doc_text) else {
        return raw_include_paths;
    };

    let file_dir = file_dir(doc_uri);
    let source_cancelled =
        no_lib_cancelled_paths_at_offset(text, offset, root, file_dir.as_deref());
    let (_, hir_cancelled) = server.cached_hir_use_lib_paths_and_cancelled(
        doc_uri,
        text,
        root,
        file_dir.as_deref(),
        Some(offset),
    );
    if source_cancelled.is_empty() && hir_cancelled.is_empty() {
        raw_include_paths
    } else {
        let cancelled_keys: HashSet<String> = source_cancelled
            .iter()
            .chain(hir_cancelled.iter())
            .map(|path| include_path_key(path))
            .collect();
        raw_include_paths
            .into_iter()
            .filter(|path| !cancelled_keys.contains(&include_path_key(path)))
            .collect()
    }
}

fn include_path_key(path: &str) -> String {
    Path::new(path)
        .components()
        .fold(PathBuf::new(), |mut acc, component| {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::RootDir
                | std::path::Component::Prefix(_)
                | std::path::Component::ParentDir
                | std::path::Component::Normal(_) => acc.push(component.as_os_str()),
            }
            acc
        })
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

fn file_dir(doc_uri: Option<&str>) -> Option<PathBuf> {
    doc_uri
        .and_then(source_path_from_uri)
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
}
