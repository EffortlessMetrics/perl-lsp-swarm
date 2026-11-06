//! Activation-site types for Function::Parameters / Method::Signatures (#16808).
//!
//! Site extraction shares the declaration walk so import classification cannot
//! drift from lexical keyword enablement.

pub use super::signature_keyword_declarations::{
    SignatureKeywordActivationSite, extract_signature_keyword_activation_sites,
};
