//! Runs the test pages of mozilla/readability (`scripts/fetch-test-pages.sh`)
//! with the same checks as its `test/test-readability.js` and
//! `test/test-isProbablyReaderable.js`.
//!
//! Without the pages the test is skipped, unless `REQUIRE_TEST_PAGES=1`.

use std::fs;
use std::path::{Path, PathBuf};

use readability_rs::dom::{Document, NodeData, NodeId};
use readability_rs::{Options, ReaderableOptions};

const URI: &str = "http://fakehost/test/page.html";

fn pages_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test-pages");
    if dir.is_dir() {
        return Some(dir);
    }
    if std::env::var_os("REQUIRE_TEST_PAGES").is_some() {
        panic!("tests/test-pages is missing; run scripts/fetch-test-pages.sh");
    }
    eprintln!("tests/test-pages is missing; run scripts/fetch-test-pages.sh (skipping)");
    None
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

fn remove_comments(doc: &mut Document) {
    let comments: Vec<NodeId> =
        doc.descendants(doc.document()).filter(|&n| matches!(doc.node(n).data, NodeData::Comment(_))).collect();
    for c in comments {
        doc.remove(c);
    }
}

/// Collapse subsequent whitespace like HTML (and ignore the indentation the
/// upstream test adds with a pretty printer).
fn html_transform(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_skipped(doc: &Document, n: NodeId) -> bool {
    match &doc.node(n).data {
        NodeData::Text(t) => t.trim().is_empty(),
        NodeData::Comment(_) => true,
        _ => false,
    }
}

fn next_node(doc: &Document, from: NodeId) -> Option<NodeId> {
    let mut n = Some(from);
    loop {
        n = doc.next_in(n?, doc.document());
        match n {
            Some(x) if is_skipped(doc, x) => continue,
            other => return other,
        }
    }
}

fn node_str(doc: &Document, n: Option<NodeId>) -> String {
    let Some(n) = n else { return "(no node)".into() };
    match &doc.node(n).data {
        NodeData::Text(t) => format!("#text({})", html_transform(t)),
        NodeData::Element(e) => {
            let mut s = e.name.local.to_string();
            let id = doc.id_attr(n);
            if !id.is_empty() {
                s += &format!("#{id}");
            }
            let class = doc.class_name(n);
            if !class.is_empty() {
                s += &format!(".({class})");
            }
            s
        }
        other => format!("some other node type: {other:?}"),
    }
}

fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first = chars.next();
    first.is_some_and(|c| c.is_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.' | '\u{B7}'))
}

fn attributes(doc: &Document, n: NodeId) -> Vec<(String, String)> {
    doc.attrs(n)
        .iter()
        .map(|a| (readability_rs::dom::attr_qualified_name(&a.name).into_owned(), a.value.to_string()))
        .filter(|(name, _)| is_xml_name(name))
        .collect()
}

/// Compares the DOMs node by node; returns the first difference.
fn compare_content(actual_html: &str, expected_html: &str) -> Result<(), String> {
    let actual = Document::parse(actual_html, None);
    let expected = Document::parse(expected_html, None);
    let mut a = actual.document_element();
    let mut e = expected.document_element();
    while a.is_some() || e.is_some() {
        let (Some(an), Some(en)) = (a, e) else {
            return Err(format!(
                "Should have a node from both DOMs: {} vs {}",
                node_str(&actual, a),
                node_str(&expected, e)
            ));
        };
        let ad = node_str(&actual, a);
        let ed = node_str(&expected, e);
        if ad != ed {
            let parent = actual.parent(an).map(|p| actual.inner_html(p)).unwrap_or_default();
            let ctx: String = parent.chars().take(300).collect();
            return Err(format!("node mismatch: {ad}\n  expected: {ed}\n  in: {ctx}"));
        }
        if actual.is_element(an) {
            let aa = attributes(&actual, an);
            let ea = attributes(&expected, en);
            if aa.len() != ea.len() {
                return Err(format!("node {ad} attributes {aa:?} should match {ea:?}"));
            }
            for (name, value) in &aa {
                let ev = expected.attr(en, name).unwrap_or("");
                if ev != value {
                    return Err(format!("node {ad} attribute {name}: {value:?} != expected {ev:?}"));
                }
            }
        }
        a = next_node(&actual, an);
        e = next_node(&expected, en);
    }
    Ok(())
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_owned)
}

#[test]
fn readability_test_pages() {
    let Some(dir) = pages_dir() else { return };
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    names.sort();
    let only = std::env::var("TEST_PAGE").ok();

    let options = Options { classes_to_preserve: vec!["caption".into()], ..Default::default() };
    let mut failures = Vec::new();
    let mut count = 0;
    for name in &names {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        count += 1;
        let page = dir.join(name);
        let source = read(&page.join("source.html"));
        let expected = read(&page.join("expected.html"));
        let meta: serde_json::Value = serde_json::from_str(&read(&page.join("expected-metadata.json"))).unwrap();

        let mut doc = Document::parse(&source, Some(url::Url::parse(URI).unwrap()));
        remove_comments(&mut doc);
        let readerable = readability_rs::is_probably_readerable_document(&doc, &ReaderableOptions::default());
        let article = match readability_rs::parse_document(&mut doc, &options) {
            Ok(Some(a)) => a,
            other => {
                failures.push(format!("{name}: no article ({other:?})"));
                continue;
            }
        };

        let mut errs = Vec::new();
        if let Err(e) = compare_content(&article.content, &expected) {
            errs.push(format!("content: {e}"));
        }
        let checks: [(&str, Option<String>, bool); 4] = [
            ("title", Some(article.title.clone()), true),
            ("byline", article.byline.clone(), true),
            ("excerpt", article.excerpt.clone(), true),
            ("siteName", article.site_name.clone(), true),
        ];
        for (key, actual, _) in checks {
            let exp = json_str(&meta, key);
            if actual != exp {
                errs.push(format!("{key}: {actual:?} != expected {exp:?}"));
            }
        }
        for (key, actual) in
            [("dir", &article.dir), ("lang", &article.lang), ("publishedTime", &article.published_time)]
        {
            if let Some(exp) = json_str(&meta, key)
                && actual.as_deref() != Some(exp.as_str())
            {
                errs.push(format!("{key}: {actual:?} != expected {exp:?}"));
            }
        }
        if let Some(exp) = meta.get("readerable").and_then(|v| v.as_bool())
            && readerable != exp
        {
            errs.push(format!("readerable: {readerable} != expected {exp}"));
        }
        if !errs.is_empty() {
            failures.push(format!("{name}:\n    {}", errs.join("\n    ")));
        }
    }
    eprintln!("{} / {} test pages passed", count - failures.len(), count);
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
