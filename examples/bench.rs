//! Times HTML parsing and the full extraction over a directory of pages.
//!
//!   cargo run --release --no-default-features --example bench -- tests/test-pages [rounds]
//!
//! Each sub-directory's `source.html` (or each `*.html` file) is one page.
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "tests/test-pages".into());
    let rounds: usize = args.next().map_or(5, |r| r.parse().unwrap());
    let mut pages = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let file = if p.is_dir() { p.join("source.html") } else { p.clone() };
        if file.extension().is_some_and(|e| e == "html") && file.exists() {
            pages.push(std::fs::read_to_string(&file).unwrap());
        }
    }
    let url = Some(url::Url::parse("http://fakehost/test/page.html").unwrap());
    let options = readability_rs::Options::default();
    let mut parse_ms = Vec::new();
    let mut total_ms = Vec::new();
    for _ in 0..=rounds {
        let t = Instant::now();
        for html in &pages {
            std::hint::black_box(readability_rs::Document::parse(html, url.clone()));
        }
        parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        for html in &pages {
            let mut doc = readability_rs::Document::parse(html, url.clone());
            std::hint::black_box(readability_rs::parse_document(&mut doc, &options).unwrap());
        }
        total_ms.push(t.elapsed().as_secs_f64() * 1e3);
    }
    parse_ms.remove(0);
    total_ms.remove(0);
    parse_ms.sort_by(f64::total_cmp);
    total_ms.sort_by(f64::total_cmp);
    let mb: usize = pages.iter().map(String::len).sum();
    println!(
        "{} pages, {:.1} MB: parse {:.1} ms, parse + extract {:.1} ms (median of {rounds})",
        pages.len(),
        mb as f64 / 1e6,
        parse_ms[rounds / 2],
        total_ms[rounds / 2]
    );
}
