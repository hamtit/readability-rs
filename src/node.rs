//! Node-API bindings (Node.js / Bun).
//!
//! `parse` runs on the calling thread; `parseAsync` runs on the libuv thread
//! pool (Bun's Node-API workers), so it does not block the JavaScript thread.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::{Article, Options, ReaderableOptions};

#[napi(object)]
#[derive(Default)]
pub struct ParseOptions {
    /// Document URL, used to resolve relative links and images.
    pub url: Option<String>,
    /// Maximum number of elements to parse (default 0: no limit).
    pub max_elems_to_parse: Option<u32>,
    /// Number of top candidates to consider (default 5).
    pub nb_top_candidates: Option<u32>,
    /// Number of characters an article must have (default 500).
    pub char_threshold: Option<u32>,
    /// Classes to keep in addition to `page`.
    pub classes_to_preserve: Option<Vec<String>>,
    /// Keep all classes (default false).
    pub keep_classes: Option<bool>,
    /// Do not read metadata from JSON-LD (default false).
    #[napi(js_name = "disableJSONLD")]
    pub disable_json_ld: Option<bool>,
    /// Regular expression source (Rust `regex` syntax) of video URLs to keep.
    pub allowed_video_regex: Option<String>,
    /// Added to the link density thresholds (default 0).
    pub link_density_modifier: Option<f64>,
}

#[napi(object)]
pub struct ParsedArticle {
    pub title: String,
    pub byline: Option<String>,
    pub dir: Option<String>,
    pub lang: Option<String>,
    pub content: String,
    pub text_content: String,
    pub length: u32,
    pub excerpt: Option<String>,
    pub site_name: Option<String>,
    pub published_time: Option<String>,
}

#[napi(object)]
pub struct ReaderableCheckOptions {
    pub min_content_length: Option<u32>,
    pub min_score: Option<f64>,
}

#[napi(object)]
pub struct Versions {
    /// Version of this package.
    pub readability_rs: String,
    /// Version of Readability.js the algorithm follows.
    pub readability_js: String,
}

struct Prepared {
    url: Option<String>,
    options: Options,
}

fn prepare(o: Option<ParseOptions>) -> Result<Prepared> {
    let o = o.unwrap_or_default();
    let allowed_video_regex = match o.allowed_video_regex {
        Some(src) => Some(
            regex::Regex::new(&src)
                .map_err(|e| Error::new(Status::InvalidArg, format!("Invalid allowedVideoRegex: {e}")))?,
        ),
        None => None,
    };
    Ok(Prepared {
        url: o.url,
        options: Options {
            max_elems_to_parse: o.max_elems_to_parse.unwrap_or(0) as usize,
            nb_top_candidates: o.nb_top_candidates.unwrap_or(0) as usize,
            char_threshold: o.char_threshold.unwrap_or(0) as usize,
            classes_to_preserve: o.classes_to_preserve.unwrap_or_default(),
            keep_classes: o.keep_classes.unwrap_or(false),
            disable_json_ld: o.disable_json_ld.unwrap_or(false),
            allowed_video_regex,
            link_density_modifier: o.link_density_modifier.unwrap_or(0.0),
        },
    })
}

fn run(html: &str, p: &Prepared) -> Result<Option<Article>> {
    crate::parse(html, p.url.as_deref(), &p.options).map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
}

fn to_js(a: Article) -> ParsedArticle {
    ParsedArticle {
        title: a.title,
        byline: a.byline,
        dir: a.dir,
        lang: a.lang,
        content: a.content,
        text_content: a.text_content,
        length: a.length as u32,
        excerpt: a.excerpt,
        site_name: a.site_name,
        published_time: a.published_time,
    }
}

/// Extract the article from an HTML document (`new Readability(doc, options).parse()`).
/// Returns `null` when no article is found.
#[napi]
pub fn parse(html: String, options: Option<ParseOptions>) -> Result<Option<ParsedArticle>> {
    let p = prepare(options)?;
    Ok(run(&html, &p)?.map(to_js))
}

pub struct ParseTask {
    html: String,
    prepared: Result<Prepared>,
}

impl Task for ParseTask {
    type Output = Option<Article>;
    type JsValue = Option<ParsedArticle>;

    fn compute(&mut self) -> Result<Self::Output> {
        match &self.prepared {
            Ok(p) => run(&self.html, p),
            Err(e) => Err(Error::new(e.status, e.reason.clone())),
        }
    }

    fn resolve(&mut self, _env: Env, out: Self::Output) -> Result<Self::JsValue> {
        Ok(out.map(to_js))
    }
}

/// Same as `parse`, on the thread pool.
#[napi]
pub fn parse_async(html: String, options: Option<ParseOptions>, signal: Option<AbortSignal>) -> AsyncTask<ParseTask> {
    AsyncTask::with_optional_signal(ParseTask { html, prepared: prepare(options) }, signal)
}

/// `isProbablyReaderable(doc, options)` on an HTML string.
#[napi]
pub fn is_probably_readerable(html: String, options: Option<ReaderableCheckOptions>) -> bool {
    let mut o = ReaderableOptions::default();
    if let Some(opts) = options {
        if let Some(m) = opts.min_content_length {
            o.min_content_length = m as usize;
        }
        if let Some(s) = opts.min_score {
            o.min_score = s;
        }
    }
    crate::is_probably_readerable(&html, &o)
}

#[napi]
pub fn versions() -> Versions {
    Versions { readability_rs: env!("CARGO_PKG_VERSION").into(), readability_js: crate::READABILITY_JS_VERSION.into() }
}
