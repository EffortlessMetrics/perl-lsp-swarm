mod dir;
mod file;
mod sectioned_identity;
mod typed;

pub use dir::parse_dir;
pub use file::parse_file;
pub use sectioned_identity::load_sectioned_corpus_document;
pub(crate) use typed::detect_newline_style;
pub use typed::{
    CorpusLoadError, NO_FOLLOW_REVIEWED, NewlineStyle, PlainPerlSource, SectionCaseId,
    SectionedCase, SectionedCorpusDocument, load_plain_perl_source,
};
