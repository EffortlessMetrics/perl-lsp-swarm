use perl_module::collect_module_uri_candidates_with_effective_inc;
use perl_module::path::module_name_to_path;
use perl_module::resolution::uri::{ModuleUriResolution, resolve_module_uri};
use proptest::prelude::*;
use std::time::Duration;

fn module_name_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec("[A-Za-z_][A-Za-z0-9_]{0,7}", 1..5)
        .prop_map(|segments| segments.join("::"))
}

proptest! {
    #[test]
    fn fallback_resolution_ignores_unknown_modules(module_name in module_name_strategy()) {
        let include_paths = vec![".".to_string(), "lib".to_string(), "..".to_string()];

        let result = resolve_module_uri(
            &module_name,
            &[],
            &["file:///workspace".to_string()],
            &include_paths,
            false,
            &[],
            Duration::from_millis(20),
        );

        prop_assert!(matches!(result, ModuleUriResolution::NotFound));
    }

    #[test]
    fn open_document_precedence_is_deterministic(module_name in module_name_strategy(), prefix in "[a-z]{1,8}") {
        let rel = module_name_to_path(&module_name);
        let open_uri = format!("file:///open/{prefix}/{rel}");

        let result = resolve_module_uri(
            &module_name,
            std::slice::from_ref(&open_uri),
            &["file:///workspace".to_string()],
            &["lib".to_string()],
            false,
            &[],
            Duration::from_millis(20),
        );

        prop_assert_eq!(result, ModuleUriResolution::Resolved(open_uri));
    }

    #[test]
    fn candidate_report_deduplicates_repeated_open_documents_and_keeps_stable_order(
        module_name in module_name_strategy(),
        duplicate_count in 1usize..8,
    ) {
        let relative_path = module_name_to_path(&module_name).replace('\\', "/");
        let open_uri = format!("file:///open/{relative_path}");
        let open_documents = vec![open_uri.clone(); duplicate_count];

        let report = collect_module_uri_candidates_with_effective_inc(
            &module_name,
            &open_documents,
            &[],
            &[],
            Duration::from_millis(20),
        );
        let repeated_report = collect_module_uri_candidates_with_effective_inc(
            &module_name,
            &open_documents,
            &[],
            &[],
            Duration::from_millis(20),
        );

        prop_assert_eq!(&report, &repeated_report);
        prop_assert_eq!(report.candidates.len(), 1);
        let candidate = report.candidates.first().ok_or_else(|| {
            TestCaseError::fail("candidate report unexpectedly had no matching open document")
        })?;
        prop_assert_eq!(&candidate.uri, &open_uri);
        prop_assert_eq!(&candidate.source, "open-document");
        prop_assert_eq!(candidate.search_order, 0);
        prop_assert_eq!(
            report.candidates.iter().map(|candidate| candidate.search_order).collect::<Vec<_>>(),
            vec![0]
        );
    }
}
