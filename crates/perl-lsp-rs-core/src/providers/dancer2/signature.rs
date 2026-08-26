//! Dancer2 route-DSL signature help cell (#8928).
//!
//! Provides version-authoritative signature forms for the supported static
//! route DSL from the reviewed #8918 form table. The DSL supports several
//! forms; they are reported as distinct forms, never flattened into one.

use perl_semantic_facts::framework_adapters::dancer2_routes::{
    DANCER2_ANY_DEFAULT_METHODS, dancer2_keyword_methods,
};

/// One signature form of the reviewed route DSL.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSignatureForm {
    /// Human-readable parameter shape.
    pub parameters: &'static str,
    /// Bounded description of what the form binds.
    pub description: String,
}

/// Signature forms for one route keyword (`None` for non-route keywords).
///
/// `any` additionally documents the explicit method-list form and the
/// reviewed default method vocabulary.
#[must_use]
pub fn route_keyword_signature_forms(keyword: &str) -> Option<Vec<RouteSignatureForm>> {
    let methods = dancer2_keyword_methods(keyword)?;
    let method_names = method_list(&methods);
    Some(
        vec![
            RouteSignatureForm {
                parameters: "PATTERN, CODE",
                description: format!("route for {method_names}"),
            },
            RouteSignatureForm {
                parameters: "PATTERN, OPTIONS, CODE",
                description: format!("route for {method_names} with an options hash"),
            },
            RouteSignatureForm {
                parameters: "NAME, PATTERN, CODE",
                description: format!("named route for {method_names}"),
            },
            RouteSignatureForm {
                parameters: "NAME, PATTERN, OPTIONS, CODE",
                description: format!("named route for {method_names} with an options hash"),
            },
        ]
        .into_iter()
        .chain(if keyword == "any" {
            Some(RouteSignatureForm {
                parameters: "[METHODS], PATTERN, CODE",
                description: format!(
                    "route restricted to an explicit method list; a bare any matches the \
                 reviewed default vocabulary ({})",
                    DANCER2_ANY_DEFAULT_METHODS.join(", ")
                ),
            })
        } else {
            None
        })
        .collect(),
    )
}

fn method_list(methods: &perl_semantic_facts::route::RouteMethodSet) -> String {
    match methods {
        perl_semantic_facts::route::RouteMethodSet::Exact(names) => names.join(", "),
        perl_semantic_facts::route::RouteMethodSet::Dynamic { .. } => {
            "a computed method list (dynamic boundary)".to_string()
        }
        _ => "unknown methods".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_reports_get_and_head_semantics() {
        let forms = route_keyword_signature_forms("get").expect("get forms");
        assert!(forms.iter().any(|form| form.description.contains("GET, HEAD")));
        assert!(forms.iter().any(|form| form.parameters.contains("NAME")));
    }

    #[test]
    fn forms_are_not_flattened() {
        let forms = route_keyword_signature_forms("post").expect("post forms");
        assert!(forms.len() >= 4, "several forms stay distinct: {forms:?}");
    }

    #[test]
    fn any_documents_explicit_method_list_and_defaults() {
        let forms = route_keyword_signature_forms("any").expect("any forms");
        assert!(
            forms.iter().any(|form| form.parameters.starts_with("[METHODS]")),
            "any carries the explicit method-list form"
        );
        assert!(forms.iter().any(|form| form.description.contains("DELETE")));
    }

    #[test]
    fn non_route_keywords_have_no_route_signature() {
        assert!(route_keyword_signature_forms("hook").is_none());
        assert!(route_keyword_signature_forms("delete").is_none());
        assert!(route_keyword_signature_forms("prefix").is_none());
    }
}
