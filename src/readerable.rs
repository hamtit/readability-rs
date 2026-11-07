//! Port of `Readability-readerable.js`: a quick check whether
//! [`parse`](crate::parse) is likely to find an article.

use std::sync::LazyLock;

use html5ever::{local_name, ns};
use regex::Regex;

use crate::dom::{Document, NodeId};
use crate::js;

// NOTE: These two regular expressions are duplicated in readability.rs.
static UNLIKELY_CANDIDATES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote").unwrap()
});
static OK_MAYBE_ITS_A_CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)and|article|body|column|content|main|shadow").unwrap());

/// Options of `isProbablyReaderable(doc, options)`.
#[derive(Debug, Clone, Copy)]
pub struct ReaderableOptions {
    /// The minimum node content length used to decide if the document is readerable.
    pub min_content_length: usize,
    /// The minimum cumulated 'score' used to determine if the document is readerable.
    pub min_score: f64,
}

impl Default for ReaderableOptions {
    fn default() -> Self {
        ReaderableOptions { min_content_length: 140, min_score: 20.0 }
    }
}

fn is_node_visible(doc: &Document, node: NodeId) -> bool {
    // Have to null-check node.style and node.className.includes to deal with SVG and MathML nodes.
    let svg = doc.namespace(node) == Some(&ns!(svg));
    if let Some(style) = doc.attr(node, "style")
        && (svg || doc.namespace(node) == Some(&ns!(html)))
        && crate::readability::style_display_is_none(style)
    {
        return false;
    }
    if doc.has_attr(node, "hidden") {
        return false;
    }
    // check for "fallback-image" so that wikimedia math images are displayed
    match doc.attr(node, "aria-hidden") {
        Some("true") => !svg && doc.class_name(node).contains("fallback-image"),
        _ => true,
    }
}

/// Decides whether or not the document is reader-able without parsing the whole thing.
pub fn is_probably_readerable_doc(doc: &Document, options: &ReaderableOptions) -> bool {
    let root = doc.document();
    let mut nodes: Vec<NodeId> = doc
        .descendants(root)
        .filter(|&n| {
            matches!(doc.local_name(n), Some(&local_name!("p") | &local_name!("pre") | &local_name!("article")))
        })
        .collect();

    // Get <div> nodes which have <br> node(s) and append them into the `nodes` variable.
    // Some articles' DOM structures might look like
    // <div>
    //   Sentences<br>
    //   <br>
    //   Sentences<br>
    // </div>
    let mut seen: std::collections::HashSet<NodeId> = nodes.iter().copied().collect();
    for br in doc.descendants(root) {
        if doc.local_name(br) != Some(&local_name!("br")) {
            continue;
        }
        if let Some(parent) = doc.parent(br)
            && doc.local_name(parent) == Some(&local_name!("div"))
            && seen.insert(parent)
        {
            nodes.push(parent);
        }
    }

    let mut score = 0.0;
    nodes.into_iter().any(|node| {
        if !is_node_visible(doc, node) {
            return false;
        }

        let class =
            if doc.namespace(node) == Some(&ns!(svg)) { "[object SVGAnimatedString]" } else { doc.class_name(node) };
        let match_string = format!("{} {}", class, doc.id_attr(node));
        if UNLIKELY_CANDIDATES.is_match(&match_string) && !OK_MAYBE_ITS_A_CANDIDATE.is_match(&match_string) {
            return false;
        }

        // node.matches("li p")
        if doc.local_name(node) == Some(&local_name!("p")) {
            let mut a = doc.parent(node);
            while let Some(p) = a {
                if doc.local_name(p) == Some(&local_name!("li")) {
                    return false;
                }
                a = doc.parent(p);
            }
        }

        let text_content_length = js::len(js::trim(&doc.text_content(node)));
        if text_content_length < options.min_content_length {
            return false;
        }

        score += ((text_content_length - options.min_content_length) as f64).sqrt();

        score > options.min_score
    })
}
