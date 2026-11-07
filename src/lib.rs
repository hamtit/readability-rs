//! A Rust port of [Mozilla's Readability](https://github.com/mozilla/readability),
//! the library behind Firefox Reader View.
//!
//! ```
//! let html = r#"<html><head><title>Hello</title></head><body><article><p>...</p></article></body></html>"#;
//! let article = readability_rs::parse(html, Some("https://example.com/post"), &Default::default()).unwrap();
//! # let _ = article;
//! ```
//!
//! The algorithm is a line-by-line port of `Readability.js` 0.6.0, run on an
//! html5ever DOM. Text lengths are measured in UTF-16 code units and white
//! space follows ECMAScript's definition, so thresholds behave as in
//! JavaScript.

pub mod dom;
mod js;
mod readability;
mod readerable;

#[cfg(feature = "napi")]
mod node;

#[cfg(feature = "wasm")]
mod wasm;

pub use dom::{Document, NodeId};
pub use readability::{Article, Error, Options};
pub use readerable::ReaderableOptions;

/// Version of the upstream Readability.js this port follows.
pub const READABILITY_JS_VERSION: &str = "0.6.0";

/// Parses `html` and extracts the article (`new Readability(doc).parse()`).
///
/// `url` is the document URL, used to resolve relative links and images.
/// Returns `Ok(None)` when no article could be found.
pub fn parse(html: &str, url: Option<&str>, options: &Options) -> Result<Option<Article>, Error> {
    let url = match url {
        Some(u) => Some(url::Url::parse(u).map_err(|_| Error::InvalidUrl(u.to_owned()))?),
        None => None,
    };
    let mut doc = Document::parse(html, url);
    parse_document(&mut doc, options)
}

/// Runs Readability on an already parsed document. The document is modified.
pub fn parse_document(doc: &mut Document, options: &Options) -> Result<Option<Article>, Error> {
    readability::Readability::new(doc, options).parse()
}

/// `isProbablyReaderable(doc, options)` on `html`.
pub fn is_probably_readerable(html: &str, options: &ReaderableOptions) -> bool {
    let doc = Document::parse(html, None);
    readerable::is_probably_readerable_doc(&doc, options)
}

/// `isProbablyReaderable` on an already parsed document.
pub fn is_probably_readerable_document(doc: &Document, options: &ReaderableOptions) -> bool {
    readerable::is_probably_readerable_doc(doc, options)
}
