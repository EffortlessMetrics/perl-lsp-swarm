use perl_module::path::module_name_to_path;
use perl_module::resolution::uri::{ModuleUriResolution, resolve_module_uri};
use perl_module::{IncRoot, IncRootKind, collect_module_uri_candidates_with_effective_inc};
use std::path::PathBuf;
use std::time::Duration;

fn next_u64(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545F4914F6CDD1D)
}

fn fuzz_string(state: &mut u64, max_len: usize) -> String {
    let len = (next_u64(state) as usize % max_len).saturating_add(1);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let byte = (next_u64(state) & 0x7F) as u8;
        let ch = match byte {
            0..=31 => '_',
            b':' | b'/' | b'\\' => byte as char,
            _ => byte as char,
        };
        out.push(ch);
    }
    out
}

#[test]
fn fuzz_resolution_inputs_never_panic_and_emit_valid_uri_shape()
-> Result<(), Box<dyn std::error::Error>> {
    let mut seed = 0xC0FFEE_u64;

    for _ in 0..2000 {
        let module_name = fuzz_string(&mut seed, 40);

        let open_docs: Vec<String> =
            (0..3).map(|_| format!("file:///{}", fuzz_string(&mut seed, 24))).collect();
        let workspace_folders: Vec<String> = (0..3)
            .map(|_| {
                if (next_u64(&mut seed) & 1) == 0 {
                    format!("file:///{}", fuzz_string(&mut seed, 24))
                } else {
                    fuzz_string(&mut seed, 24)
                }
            })
            .collect();
        let include_paths: Vec<String> = (0..4).map(|_| fuzz_string(&mut seed, 12)).collect();
        let system_inc: Vec<PathBuf> =
            (0..2).map(|_| PathBuf::from(fuzz_string(&mut seed, 20))).collect();

        let result = resolve_module_uri(
            &module_name,
            &open_docs,
            &workspace_folders,
            &include_paths,
            (next_u64(&mut seed) & 1) == 0,
            &system_inc,
            Duration::from_millis(5),
        );

        if let ModuleUriResolution::Resolved(uri) = result {
            assert!(uri.starts_with("file://"));
        }

        let relative_path = module_name_to_path(&module_name).replace('\\', "/");
        let matching_open_document = format!("file:///fuzz/{relative_path}");
        let mut report_open_documents = vec![matching_open_document.clone()];
        report_open_documents.extend(open_docs.iter().cloned());
        let effective_roots: Vec<IncRoot> = include_paths
            .iter()
            .enumerate()
            .map(|(precedence, path)| IncRoot {
                kind: IncRootKind::WorkspaceRelative,
                path: PathBuf::from(path),
                precedence,
                source: "fuzz".to_string(),
            })
            .collect();
        let report = collect_module_uri_candidates_with_effective_inc(
            &module_name,
            &report_open_documents,
            &workspace_folders,
            &effective_roots,
            Duration::ZERO,
        );

        assert!(report.timed_out);
        assert!(!report.candidates.is_empty());
        let candidate = report.candidates.first().ok_or("fuzz report has no candidate")?;
        // Open-document candidates preserve the supplied URI spelling verbatim
        // (ModuleUriCandidate.uri contract).  The previous round-trip normalization
        // (Url::parse → to_file_path → from_file_path) was platform-dependent:
        // on Linux it URL-encodes raw characters and produces a different string;
        // on Windows, to_file_path() may fail for certain path shapes and silently
        // falls back to the original, hiding the divergence.  Assert the raw form
        // directly so the check is identical on every platform.
        assert_eq!(candidate.uri, matching_open_document);
        assert_eq!(candidate.source, "open-document");
        assert_eq!(candidate.search_order, 0);
        assert!(
            report
                .candidates
                .iter()
                .enumerate()
                .all(|(index, candidate)| candidate.search_order == index)
        );
        assert!(
            report
                .candidates
                .iter()
                .map(|candidate| candidate.uri.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len()
                == report.candidates.len()
        );
    }

    Ok(())
}
