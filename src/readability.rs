//! Port of `Readability.js` 0.6.0 (mozilla/readability).
//!
//! The structure follows the JavaScript source closely: each `_method` there
//! has a counterpart here with the same name in snake case, so changes
//! upstream can be carried over. Comments from the original are kept where
//! they explain the heuristics.

use std::collections::HashMap;
use std::sync::LazyLock;

use html5ever::{LocalName, local_name, ns};
use regex::Regex;

use crate::dom::{Document, NodeId, attr_qualified_name};
use crate::js;

const FLAG_STRIP_UNLIKELYS: u32 = 0x1;
const FLAG_WEIGHT_CLASSES: u32 = 0x2;
const FLAG_CLEAN_CONDITIONALLY: u32 = 0x4;

const DEFAULT_N_TOP_CANDIDATES: usize = 5;
const DEFAULT_CHAR_THRESHOLD: usize = 500;

/// Options of `new Readability(doc, options)`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Maximum number of elements to parse; 0 means no limit.
    pub max_elems_to_parse: usize,
    /// Number of top candidates to consider (0 = default 5).
    pub nb_top_candidates: usize,
    /// Number of characters an article must have (0 = default 500).
    pub char_threshold: usize,
    /// Classes to keep on elements in addition to `page`.
    pub classes_to_preserve: Vec<String>,
    /// Keep all classes.
    pub keep_classes: bool,
    /// Do not read metadata from JSON-LD.
    pub disable_json_ld: bool,
    /// Videos whose embed attributes match this are kept. `None` uses the
    /// default (YouTube, Vimeo, ...).
    pub allowed_video_regex: Option<Regex>,
    /// Added to the link density thresholds of `_cleanConditionally`.
    pub link_density_modifier: f64,
}

/// The result of [`parse`](crate::parse) (`Readability.parse()`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Article {
    pub title: String,
    pub byline: Option<String>,
    pub dir: Option<String>,
    pub lang: Option<String>,
    /// The article HTML (`<div id="readability-page-1" class="page">...</div>`).
    pub content: String,
    pub text_content: String,
    /// `textContent.length` in UTF-16 code units, as in JavaScript.
    pub length: usize,
    pub excerpt: Option<String>,
    pub site_name: Option<String>,
    pub published_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The document has more elements than `max_elems_to_parse`.
    TooManyElements(usize),
    /// The document URL could not be parsed.
    InvalidUrl(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooManyElements(n) => write!(f, "Aborting parsing document; {n} elements found"),
            Error::InvalidUrl(u) => write!(f, "Invalid URL: {u}"),
        }
    }
}

impl std::error::Error for Error {}

fn re(pattern: &str) -> Regex {
    // `\s` in the JavaScript patterns is written as `\s` here too and
    // replaced by the ECMAScript white space set.
    let p = pattern.replace(r"\s", &format!("[{}]", js::WS_CLASS)).replace(r"\S", &format!("[^{}]", js::WS_CLASS));
    Regex::new(&p).unwrap_or_else(|e| panic!("bad regex {pattern}: {e}"))
}

macro_rules! regex {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| re($pat));
    };
}

// NOTE: These two regular expressions are duplicated in readerable.rs.
regex!(
    UNLIKELY_CANDIDATES,
    r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote"
);
regex!(OK_MAYBE_ITS_A_CANDIDATE, r"(?i)and|article|body|column|content|main|shadow");
regex!(POSITIVE, r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story");
regex!(
    NEGATIVE,
    r"(?i)-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget"
);
regex!(BYLINE, r"(?i)byline|author|dateline|writtenby|p-author");
regex!(
    VIDEOS,
    r"(?i)//(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)"
);
regex!(SRCSET_URL, r"(\S+)(\s+[0-9.]+[xw])?(\s*(?:,|$))");
regex!(B64_DATA_URL, r"(?i)^data:\s*([^\s;,]+)\s*;\s*base64\s*,");
regex!(
    JSON_LD_ARTICLE_TYPES,
    r"^Article|AdvertiserContentArticle|NewsArticle|AnalysisNewsArticle|AskPublicNewsArticle|BackgroundNewsArticle|OpinionNewsArticle|ReportageNewsArticle|ReviewNewsArticle|Report|SatiricalArticle|ScholarlyArticle|MedicalScholarlyArticle|SocialMediaPosting|BlogPosting|LiveBlogPosting|DiscussionForumPosting|TechArticle|APIReference$"
);
regex!(AD_WORDS, r"(?i)^(ad(vertising|vertisement)?|pub(licité)?|werb(ung)?|广告|Реклама|Anuncio)$");
regex!(LOADING_WORDS, r"(?i)^((loading|正在加载|Загрузка|chargement|cargando)(…|\.\.\.)?)$");
regex!(IMAGE_EXT_I, r"(?i)\.(jpg|jpeg|png|webp)");
regex!(LAZY_SRCSET, r"\.(jpg|jpeg|png|webp)\s+[0-9]");
regex!(LAZY_SRC, r"^\s*\S+\.(jpg|jpeg|png|webp)\S*\s*$");
regex!(TITLE_SEPARATOR, r" [\|\-\\/>»] ");
regex!(TITLE_HIERARCHICAL_SEPARATOR, r" [\\/>»] ");
regex!(TITLE_FIRST_PART, r"(?i)^[^\|\-\\/>»]*[\|\-\\/>»]");
regex!(TITLE_SEPARATOR_CHARS, r"[\|\-\\/>»]+");
regex!(
    META_PROPERTY,
    r"(?i)\s*(article|dc|dcterm|og|twitter)\s*:\s*(author|creator|description|published_time|title|site_name)\s*"
);
regex!(
    META_NAME,
    r"(?i)^\s*(?:(dc|dcterm|og|twitter|parsely|weibo:(article|webpage))\s*[-\.:]\s*)?(author|creator|pub-date|description|title|site_name)\s*$"
);
regex!(SCHEMA_DOT_ORG, r"^https?://schema\.org/?$");
regex!(NAMED_ENTITY, r"&(quot|amp|apos|lt|gt);");
regex!(NUMERIC_ENTITY, r"(?i)&#(?:x([0-9a-f]+)|([0-9]+));");

const UNLIKELY_ROLES: &[&str] = &["menu", "menubar", "complementary", "navigation", "alert", "alertdialog", "dialog"];

static DIV_TO_P_ELEMS: LazyLock<[LocalName; 9]> = LazyLock::new(|| {
    [
        local_name!("blockquote"),
        local_name!("dl"),
        local_name!("div"),
        local_name!("img"),
        local_name!("ol"),
        local_name!("p"),
        local_name!("pre"),
        local_name!("table"),
        local_name!("ul"),
    ]
});

fn is_div_to_p_elem(name: &LocalName) -> bool {
    DIV_TO_P_ELEMS.contains(name)
}

const ALTER_TO_DIV_EXCEPTIONS: &[LocalName] = &[
    local_name!("div"),
    local_name!("article"),
    local_name!("section"),
    local_name!("p"),
    local_name!("ol"),
    local_name!("ul"),
];

const PRESENTATIONAL_ATTRIBUTES: &[&str] = &[
    "align",
    "background",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "frame",
    "hspace",
    "rules",
    "style",
    "valign",
    "vspace",
];

const DEPRECATED_SIZE_ATTRIBUTE_ELEMS: &[LocalName] =
    &[local_name!("table"), local_name!("th"), local_name!("td"), local_name!("hr"), local_name!("pre")];

// The commented out elements qualify as phrasing content but tend to be
// removed by readability when put into paragraphs, so we ignore them here.
fn is_phrasing_elem(name: &LocalName) -> bool {
    matches!(
        *name,
        // "CANVAS", "IFRAME", "SVG", "VIDEO",
        local_name!("abbr")
            | local_name!("audio")
            | local_name!("b")
            | local_name!("bdo")
            | local_name!("br")
            | local_name!("button")
            | local_name!("cite")
            | local_name!("code")
            | local_name!("data")
            | local_name!("datalist")
            | local_name!("dfn")
            | local_name!("em")
            | local_name!("embed")
            | local_name!("i")
            | local_name!("img")
            | local_name!("input")
            | local_name!("kbd")
            | local_name!("label")
            | local_name!("mark")
            | local_name!("math")
            | local_name!("meter")
            | local_name!("noscript")
            | local_name!("object")
            | local_name!("output")
            | local_name!("progress")
            | local_name!("q")
            | local_name!("ruby")
            | local_name!("samp")
            | local_name!("script")
            | local_name!("select")
            | local_name!("small")
            | local_name!("span")
            | local_name!("strong")
            | local_name!("sub")
            | local_name!("sup")
            | local_name!("textarea")
            | local_name!("time")
            | local_name!("var")
            | local_name!("wbr")
    )
}

fn is_score_tag(name: &LocalName) -> bool {
    matches!(
        *name,
        local_name!("section")
            | local_name!("h2")
            | local_name!("h3")
            | local_name!("h4")
            | local_name!("h5")
            | local_name!("h6")
            | local_name!("p")
            | local_name!("td")
            | local_name!("pre")
    )
}

/// `/(\b|_)(share|sharedaddy)(\b|_)/i` with ASCII word boundaries.
fn is_share_element(s: &str) -> bool {
    let b = s.as_bytes();
    let word = |i: usize| -> bool { b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') };
    let lower = s.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("share") {
        let i = from + pos;
        // Before: a word boundary (previous char is not a word char) or "_".
        let before_ok = i == 0 || !word(i - 1) || b[i - 1] == b'_';
        if before_ok {
            for len in [5usize, 10] {
                if len == 10 && !lower[i..].starts_with("sharedaddy") {
                    continue;
                }
                let j = i + len;
                let after_ok = j >= lb.len() || !word(j) || b[j] == b'_';
                if after_ok {
                    return true;
                }
            }
        }
        from = i + 1;
    }
    false
}

/// `/^#.+/`.
fn is_hash_url(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next() == Some('#') && chars.next().is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

fn count_other_commas(s: &str) -> usize {
    if s.is_ascii() {
        return 0;
    }
    count_commas(s)
}

fn count_commas(s: &str) -> usize {
    s.chars()
        .filter(|c| {
            matches!(
                c,
                '\u{002C}'
                    | '\u{060C}'
                    | '\u{FE50}'
                    | '\u{FE10}'
                    | '\u{FE11}'
                    | '\u{2E41}'
                    | '\u{2E34}'
                    | '\u{2E32}'
                    | '\u{FF0C}'
            )
        })
        .count()
}

/// Tokens of `text.toLowerCase().split(/\W+/).filter(Boolean)`.
fn tokenize(s: &str) -> Vec<String> {
    js::to_lower(s)
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Compares second text to first one: 1 = same text, 0 = completely different text.
pub(crate) fn text_similarity(a: &str, b: &str) -> f64 {
    let tokens_a = tokenize(a);
    let tokens_b = tokenize(b);
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let set_a: std::collections::HashSet<&str> = tokens_a.iter().map(String::as_str).collect();
    let joined_len = |tokens: &mut dyn Iterator<Item = &String>| -> usize {
        let mut n = 0;
        let mut count = 0;
        for t in tokens {
            n += t.len();
            count += 1;
        }
        if count > 0 { n + count - 1 } else { 0 }
    };
    let uniq_b = joined_len(&mut tokens_b.iter().filter(|t| !set_a.contains(t.as_str())));
    let all_b = joined_len(&mut tokens_b.iter());
    1.0 - uniq_b as f64 / all_b as f64
}

/// Converts some of the common HTML entities in string to their corresponding characters.
fn unescape_html_entities(s: Option<String>) -> Option<String> {
    let s = s?;
    if s.is_empty() || !s.contains('&') {
        return Some(s);
    }
    let s = NAMED_ENTITY.replace_all(&s, |c: &regex::Captures| match &c[1] {
        "quot" => "\"",
        "amp" => "&",
        "apos" => "'",
        "lt" => "<",
        _ => ">",
    });
    let s = NUMERIC_ENTITY.replace_all(&s, |c: &regex::Captures| {
        let num = match (c.get(1), c.get(2)) {
            (Some(hex), _) => u32::from_str_radix(hex.as_str(), 16).ok(),
            (_, Some(dec)) => dec.as_str().parse::<u32>().ok(),
            _ => None,
        };
        // These character references are replaced by a conforming HTML parser.
        let ch = num.and_then(|n| if n == 0 { None } else { char::from_u32(n) }).unwrap_or('\u{FFFD}');
        ch.to_string()
    });
    Some(s.into_owned())
}

/// `new URL(url).href`. rust-url follows the WHATWG URL standard except for
/// normalizing a `C|` drive letter to `C:` in file URLs, done here.
pub(crate) fn href(u: url::Url) -> String {
    let s: String = u.into();
    if let Some(rest) = s.strip_prefix("file:///") {
        let b = rest.as_bytes();
        if b.len() >= 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b'|'
            && (b.len() == 2 || matches!(b[2], b'/' | b'\\' | b'?' | b'#'))
        {
            return format!("file:///{}:{}", &rest[..1], &rest[2..]);
        }
    }
    s
}

/// `new URL(uri, base).href`, or `None` where that throws.
pub(crate) fn resolve_url(base: Option<&url::Url>, uri: &str) -> Option<String> {
    match base {
        Some(b) => b.join(uri).ok().map(href),
        None => url::Url::parse(uri).ok().map(href),
    }
}

/// JavaScript truthiness of a JSON value.
fn truthy(v: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `value[key]` where `value` may be any JSON value; `Err` where JavaScript
/// would throw (`null[key]`).
fn js_get<'a>(v: &'a serde_json::Value, key: &str) -> Result<Option<&'a serde_json::Value>, ()> {
    use serde_json::Value;
    match v {
        Value::Null => Err(()),
        Value::Object(m) => Ok(m.get(key)),
        _ => Ok(None),
    }
}

/// `(it["@type"] || "").match(jsonLdArticleTypes)`; `Err` where JavaScript throws.
fn json_ld_type_matches(it: &serde_json::Value) -> Result<bool, ()> {
    let t = js_get(it, "@type")?;
    if !truthy(t) {
        return Ok(false);
    }
    match t {
        Some(serde_json::Value::String(s)) => Ok(JSON_LD_ARTICLE_TYPES.is_match(s)),
        // `.match` is not a function.
        _ => Err(()),
    }
}

fn json_str<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(serde_json::Value::as_str)
}

#[derive(Debug, Default, Clone)]
struct JsonLd {
    title: Option<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    site_name: Option<String>,
    date_published: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Metadata {
    title: Option<String>,
    byline: Option<String>,
    excerpt: Option<String>,
    site_name: Option<String>,
    published_time: Option<String>,
}

/// `a || b`: the first non-empty string.
fn or(a: Option<String>, b: Option<&String>) -> Option<String> {
    match a {
        Some(s) if !s.is_empty() => Some(s),
        _ => b.filter(|s| !s.is_empty()).cloned(),
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.is_empty())
}

/// Parses the `style` attribute just enough to read `display` and
/// `visibility` (the last valid declaration wins).
fn style_property(style: &str, name: &str) -> Option<String> {
    // CSS comments separate tokens like white space (`display: /* x */ none`
    // is `none`, `no/**/ne` is two words), and may contain `;`.
    let style: std::borrow::Cow<str> = if style.contains("/*") {
        let mut out = String::with_capacity(style.len());
        let mut rest = style;
        while let Some(i) = rest.find("/*") {
            out.push_str(&rest[..i]);
            out.push(' ');
            rest = match rest[i + 2..].find("*/") {
                Some(j) => &rest[i + 2 + j + 2..],
                None => "",
            };
        }
        out.push_str(rest);
        out.into()
    } else {
        style.into()
    };
    let mut value = None;
    for decl in style.split(';') {
        let Some((prop, val)) = decl.split_once(':') else { continue };
        if !js::trim(prop).eq_ignore_ascii_case(name) {
            continue;
        }
        let mut val = js::trim(val);
        if let Some(i) = val.to_ascii_lowercase().rfind("!important")
            && js::trim(&val[i + "!important".len()..]).is_empty()
        {
            val = js::trim(&val[..i]);
        }
        if !val.is_empty() {
            value = Some(val.to_ascii_lowercase());
        }
    }
    value
}

pub(crate) fn style_display_is_none(style: &str) -> bool {
    style_property(style, "display").as_deref() == Some("none")
}

pub(crate) struct Readability<'a> {
    doc: &'a mut Document,
    article_title: String,
    article_byline: Option<String>,
    article_dir: Option<String>,
    article_lang: Option<String>,
    metadata_byline: Option<String>,
    attempts: Vec<(NodeId, usize)>,
    /// `node.readability.contentScore`, by node index.
    scores: Vec<Option<f64>>,
    /// `table._readabilityDataTable`, by node index.
    data_tables: Vec<bool>,
    /// Cached measurements of text nodes (text is never modified in place).
    text_summaries: std::cell::RefCell<Vec<Option<js::TextSummary>>>,
    flags: u32,
    max_elems_to_parse: usize,
    nb_top_candidates: usize,
    char_threshold: usize,
    classes_to_preserve: Vec<String>,
    keep_classes: bool,
    disable_json_ld: bool,
    allowed_video_regex: Regex,
    link_density_modifier: f64,
}

impl<'a> Readability<'a> {
    pub(crate) fn new(doc: &'a mut Document, options: &Options) -> Self {
        let mut classes_to_preserve = vec!["page".to_owned()];
        classes_to_preserve.extend(options.classes_to_preserve.iter().cloned());
        Readability {
            doc,
            article_title: String::new(),
            article_byline: None,
            article_dir: None,
            article_lang: None,
            metadata_byline: None,
            attempts: Vec::new(),
            scores: Vec::new(),
            data_tables: Vec::new(),
            text_summaries: Default::default(),
            flags: FLAG_STRIP_UNLIKELYS | FLAG_WEIGHT_CLASSES | FLAG_CLEAN_CONDITIONALLY,
            max_elems_to_parse: options.max_elems_to_parse,
            nb_top_candidates: if options.nb_top_candidates == 0 {
                DEFAULT_N_TOP_CANDIDATES
            } else {
                options.nb_top_candidates
            },
            char_threshold: if options.char_threshold == 0 { DEFAULT_CHAR_THRESHOLD } else { options.char_threshold },
            classes_to_preserve,
            keep_classes: options.keep_classes,
            disable_json_ld: options.disable_json_ld,
            allowed_video_regex: options.allowed_video_regex.clone().unwrap_or_else(|| VIDEOS.clone()),
            link_density_modifier: options.link_density_modifier,
        }
    }

    // ----- score / flag storage -----

    fn score(&self, n: NodeId) -> Option<f64> {
        self.scores.get(n.index()).copied().flatten()
    }

    fn set_score(&mut self, n: NodeId, s: f64) {
        if self.scores.len() <= n.index() {
            self.scores.resize(self.doc.arena_len().max(n.index() + 1), None);
        }
        self.scores[n.index()] = Some(s);
    }

    fn is_data_table(&self, n: NodeId) -> bool {
        self.data_tables.get(n.index()).copied().unwrap_or(false)
    }

    fn set_data_table(&mut self, n: NodeId, v: bool) {
        if self.data_tables.len() <= n.index() {
            self.data_tables.resize(self.doc.arena_len().max(n.index() + 1), false);
        }
        self.data_tables[n.index()] = v;
    }

    fn flag_is_active(&self, flag: u32) -> bool {
        self.flags & flag > 0
    }

    fn remove_flag(&mut self, flag: u32) {
        self.flags &= !flag;
    }

    // ----- small DOM helpers with JavaScript semantics -----

    fn is(&self, n: NodeId, name: &LocalName) -> bool {
        self.doc.is_html(n, name)
    }

    /// `element.className` as a string; SVG elements have an
    /// `SVGAnimatedString` there, which none of the checks treat as a string.
    fn class_name(&self, n: NodeId) -> &str {
        if self.doc.namespace(n) == Some(&ns!(svg)) { "" } else { self.doc.class_name(n) }
    }

    fn match_string(&self, n: NodeId) -> String {
        let svg = self.doc.namespace(n) == Some(&ns!(svg));
        let class = if svg { "[object SVGAnimatedString]" } else { self.doc.class_name(n) };
        let id = self.doc.id_attr(n);
        let mut s = String::with_capacity(class.len() + id.len() + 1);
        s.push_str(class);
        s.push(' ');
        s.push_str(id);
        s
    }

    /// The cached [`js::TextSummary`] of a text node.
    #[inline]
    fn text_summary(&self, n: NodeId, text: &str) -> js::TextSummary {
        let mut cache = self.text_summaries.borrow_mut();
        let i = n.index();
        if cache.len() <= i {
            cache.resize(self.doc.arena_len().max(i + 1), None);
        }
        *cache[i].get_or_insert_with(|| js::TextSummary::of(text))
    }

    /// Feeds the text of every text node under `n` into `m`.
    #[inline]
    fn feed_text(&self, n: NodeId, m: &mut js::TextMeasure) {
        let mut node = self.doc.first_child(n);
        while let Some(c) = node {
            if let Some(t) = self.doc.text(c) {
                m.feed_summary(self.text_summary(c, t));
            }
            node = self.doc.next_in(c, n);
        }
    }

    /// Measures `this._getInnerText(e, true)` without building the string.
    fn measure_inner_text(&self, n: NodeId) -> js::TextMeasure {
        let mut m = js::TextMeasure::default();
        if let Some(t) = self.doc.char_data(n) {
            m.feed(t);
        } else {
            self.feed_text(n, &mut m);
        }
        m
    }

    /// `this._getInnerText(e, true).length`.
    fn inner_text_len(&self, n: NodeId) -> usize {
        self.measure_inner_text(n).len
    }

    /// `this._getInnerText(e, normalizeSpaces)`.
    fn get_inner_text(&self, n: NodeId, normalize_spaces: bool) -> String {
        let text = self.doc.text_content(n);
        let trimmed = js::trim(&text);
        if normalize_spaces { js::normalize_spaces(trimmed).into_owned() } else { trimmed.to_owned() }
    }

    /// `node.textContent.trim() !== ""`.
    fn has_text(&self, n: NodeId) -> bool {
        if let Some(t) = self.doc.char_data(n) {
            return js::has_non_ws(t);
        }
        let mut found = false;
        let mut node = self.doc.first_child(n);
        while let Some(c) = node {
            if let Some(t) = self.doc.text(c)
                && self.text_summary(c, t).has_content
            {
                found = true;
                break;
            }
            node = self.doc.next_in(c, n);
        }
        found
    }

    /// `node.textContent.trim().length`.
    fn trimmed_text_len(&self, n: NodeId) -> usize {
        js::len(js::trim(&self.doc.text_content(n)))
    }

    fn is_whitespace(&self, n: NodeId) -> bool {
        match self.doc.text(n) {
            Some(t) => js::is_blank(t),
            None => self.is(n, &local_name!("br")),
        }
    }

    /// Determine if a node qualifies as phrasing content.
    fn is_phrasing_content(&self, n: NodeId) -> bool {
        if self.doc.is_text(n) {
            return true;
        }
        let Some(name) = self.doc.html_name(n) else { return false };
        if is_phrasing_elem(name) {
            return true;
        }
        if !matches!(*name, local_name!("a") | local_name!("del") | local_name!("ins")) {
            return false;
        }
        // Every child must be phrasing content (iteratively, for deep nesting).
        let mut stack = vec![n];
        while let Some(e) = stack.pop() {
            let mut c = self.doc.first_child(e);
            while let Some(child) = c {
                if !self.doc.is_text(child) {
                    match self.doc.html_name(child) {
                        Some(name) if is_phrasing_elem(name) => {}
                        Some(&local_name!("a") | &local_name!("del") | &local_name!("ins")) => stack.push(child),
                        _ => return false,
                    }
                }
                c = self.doc.next_sibling(child);
            }
        }
        true
    }

    /// Finds the next node, starting from the given node, and ignoring
    /// whitespace in between. If the given node is an element, the same node is
    /// returned.
    fn next_node(&self, node: Option<NodeId>) -> Option<NodeId> {
        let mut next = node;
        while let Some(n) = next {
            if self.doc.is_element(n) {
                break;
            }
            // `textContent` of a doctype is null, which is not white space.
            match self.doc.char_data(n) {
                Some(t) if js::is_blank(t) => next = self.doc.next_sibling(n),
                _ => break,
            }
        }
        next
    }

    /// Traverse the DOM from node to node, starting at the node passed in.
    /// Pass true for the second parameter to indicate this node itself
    /// (and its kids) are going away, and we want the next node over.
    ///
    /// Calling this in a loop will traverse the DOM depth-first.
    fn get_next_node(&self, node: NodeId, ignore_self_and_kids: bool) -> Option<NodeId> {
        // First check for kids if those aren't being ignored
        if !ignore_self_and_kids && let Some(c) = self.doc.first_element_child(node) {
            return Some(c);
        }
        // Then for siblings...
        if let Some(s) = self.doc.next_element_sibling(node) {
            return Some(s);
        }
        // And finally, move up the parent chain *and* find a sibling
        // (because this is depth-first traversal, we will have already
        // seen the parent nodes themselves).
        let mut n = node;
        loop {
            n = self.doc.parent(n)?;
            if let Some(s) = self.doc.next_element_sibling(n) {
                return Some(s);
            }
        }
    }

    fn remove_and_get_next(&mut self, node: NodeId) -> Option<NodeId> {
        let next = self.get_next_node(node, true);
        self.doc.remove(node);
        next
    }

    fn get_node_ancestors(&self, node: NodeId, max_depth: usize) -> Vec<NodeId> {
        let mut ancestors = Vec::new();
        let mut n = node;
        while let Some(p) = self.doc.parent(n) {
            ancestors.push(p);
            if max_depth > 0 && ancestors.len() == max_depth {
                break;
            }
            n = p;
        }
        ancestors
    }

    /// Check if a given node has one of its ancestor tag name matching the
    /// provided one. `max_depth` 0 means the default (3), negative means no limit.
    fn has_ancestor_tag(
        &self,
        node: NodeId,
        tag: &LocalName,
        max_depth: i32,
        filter: Option<&dyn Fn(NodeId) -> bool>,
    ) -> bool {
        let max_depth = if max_depth == 0 { 3 } else { max_depth };
        let mut depth = 0;
        let mut n = node;
        while let Some(p) = self.doc.parent(n) {
            if max_depth > 0 && depth > max_depth {
                return false;
            }
            if self.is(p, tag) && filter.is_none_or(|f| f(p)) {
                return true;
            }
            n = p;
            depth += 1;
        }
        false
    }

    /// Check if this node has only whitespace and a single element with given tag.
    fn has_single_tag_inside_element(&self, element: NodeId, tag: &LocalName) -> bool {
        // There should be exactly 1 element child with given tag
        let mut only = None;
        let mut c = self.doc.first_child(element);
        while let Some(n) = c {
            if self.doc.is_element(n) {
                if only.is_some() {
                    return false;
                }
                only = Some(n);
            } else if let Some(t) = self.doc.text(n) {
                // And there should be no text nodes with real content (/\S$/)
                if t.chars().next_back().is_some_and(|c| !js::is_ws(c)) {
                    // Keep scanning for the element count check order of the
                    // original: both conditions must hold, so this is enough.
                    return false;
                }
            }
            c = self.doc.next_sibling(n);
        }
        only.is_some_and(|o| self.is(o, tag))
    }

    fn is_element_without_content(&self, node: NodeId) -> bool {
        if !self.doc.is_element(node) || self.has_text(node) {
            return false;
        }
        let children = self.doc.element_child_count(node);
        children == 0
            || children
                == self.doc.count_elements_by_tag(node, &local_name!("br"))
                    + self.doc.count_elements_by_tag(node, &local_name!("hr"))
    }

    /// Determine whether element has any children block level elements.
    fn has_child_block_element(&self, element: NodeId) -> bool {
        self.doc.descendants(element).any(|n| self.doc.html_name(n).is_some_and(is_div_to_p_elem))
    }

    fn set_node_tag(&mut self, node: NodeId, tag: &str) -> NodeId {
        self.doc.rename(node, tag);
        node
    }

    /// Iterates over a node list in reverse and removes the nodes for which
    /// `filter` returns true (all of them without a filter).
    fn remove_nodes(&mut self, nodes: &[NodeId], mut filter: impl FnMut(&mut Self, NodeId) -> bool) {
        for &node in nodes.iter().rev() {
            if self.doc.parent(node).is_some() && filter(self, node) {
                self.doc.remove(node);
            }
        }
    }

    fn get_all_nodes_with_tag(&self, node: NodeId, tags: &[LocalName]) -> Vec<NodeId> {
        if tags.len() == 1 { self.doc.elements_by_tag(node, &tags[0]) } else { self.doc.elements_by_tags(node, tags) }
    }

    // ----- scoring helpers -----

    /// Get the density of links as a percentage of the content.
    /// This is the amount of text that is inside a link divided by the total text in the node.
    fn get_link_density(&self, element: NodeId) -> f64 {
        // One walk measures the whole text and, separately, the text of each
        // (possibly nested) link, as the per-link `_getInnerText` calls do.
        let mut total = js::TextMeasure::default();
        let mut open: Vec<(NodeId, f64, js::TextMeasure)> = Vec::new();
        let mut link_length = 0.0;
        if let Some(t) = self.doc.char_data(element) {
            total.feed(t);
        }
        self.doc.walk(element, |n, entering| {
            if entering {
                if let Some(t) = self.doc.text(n) {
                    let summary = self.text_summary(n, t);
                    total.feed_summary(summary);
                    for (_, _, m) in open.iter_mut() {
                        m.feed_summary(summary);
                    }
                } else if self.doc.local_name(n) == Some(&local_name!("a")) {
                    let coefficient = match self.doc.attr(n, "href") {
                        Some(href) if is_hash_url(href) => 0.3,
                        _ => 1.0,
                    };
                    open.push((n, coefficient, js::TextMeasure::default()));
                }
            } else if open.last().is_some_and(|(a, _, _)| *a == n) {
                let (_, coefficient, m) = open.pop().unwrap();
                link_length += m.len as f64 * coefficient;
            }
        });
        if total.len == 0 {
            return 0.0;
        }
        link_length / total.len as f64
    }

    /// Get an elements class/id weight.
    fn get_class_weight(&self, e: NodeId) -> f64 {
        if !self.flag_is_active(FLAG_WEIGHT_CLASSES) {
            return 0.0;
        }
        let mut weight = 0.0;
        let class = self.class_name(e);
        if !class.is_empty() {
            if NEGATIVE.is_match(class) {
                weight -= 25.0;
            }
            if POSITIVE.is_match(class) {
                weight += 25.0;
            }
        }
        let id = self.doc.id_attr(e);
        if !id.is_empty() {
            if NEGATIVE.is_match(id) {
                weight -= 25.0;
            }
            if POSITIVE.is_match(id) {
                weight += 25.0;
            }
        }
        weight
    }

    /// Initialize a node with the readability object. Also checks the
    /// className/id for special names to add to its score.
    fn initialize_node(&mut self, node: NodeId) {
        let mut score = match self.doc.html_name(node) {
            Some(&local_name!("div")) => 5.0,
            Some(&local_name!("pre") | &local_name!("td") | &local_name!("blockquote")) => 3.0,
            Some(
                &local_name!("address")
                | &local_name!("ol")
                | &local_name!("ul")
                | &local_name!("dl")
                | &local_name!("dd")
                | &local_name!("dt")
                | &local_name!("li")
                | &local_name!("form"),
            ) => -3.0,
            Some(
                &local_name!("h1")
                | &local_name!("h2")
                | &local_name!("h3")
                | &local_name!("h4")
                | &local_name!("h5")
                | &local_name!("h6")
                | &local_name!("th"),
            ) => -5.0,
            _ => 0.0,
        };
        score += self.get_class_weight(node);
        self.set_score(node, score);
    }

    fn get_text_density(&self, e: NodeId, tags: &[LocalName]) -> f64 {
        let text_length = self.inner_text_len(e);
        if text_length == 0 {
            return 0.0;
        }
        let children_length: usize = self
            .doc
            .descendants(e)
            .filter(|&c| self.doc.local_name(c).is_some_and(|l| tags.contains(l)))
            .map(|c| self.inner_text_len(c))
            .sum();
        children_length as f64 / text_length as f64
    }

    fn is_probably_visible(&self, node: NodeId) -> bool {
        // Have to null-check node.style and node.className.includes to deal with SVG and MathML nodes.
        let has_style = matches!(self.doc.namespace(node), Some(n) if *n == ns!(html) || *n == ns!(svg));
        if has_style && let Some(style) = self.doc.attr(node, "style") {
            if style_property(style, "display").as_deref() == Some("none") {
                return false;
            }
            if style_property(style, "visibility").as_deref() == Some("hidden") {
                return false;
            }
        }
        if self.doc.has_attr(node, "hidden") {
            return false;
        }
        // check for "fallback-image" so that wikimedia math images are displayed
        match self.doc.attr(node, "aria-hidden") {
            Some("true") => self.class_name(node).contains("fallback-image"),
            _ => true,
        }
    }

    fn is_valid_byline(&self, node: NodeId, match_string: &str) -> bool {
        let rel = self.doc.attr(node, "rel");
        let itemprop = self.doc.attr(node, "itemprop");
        // The text length is only computed for candidates (it is the costly part).
        (rel == Some("author") || itemprop.is_some_and(|i| i.contains("author")) || BYLINE.is_match(match_string)) && {
            let byline_length = self.trimmed_text_len(node);
            byline_length > 0 && byline_length < 100
        }
    }

    /// Check if this node is an H1 or H2 element whose content is mostly
    /// the same as the article title.
    fn header_duplicates_title(&self, node: NodeId) -> bool {
        if !self.is(node, &local_name!("h1")) && !self.is(node, &local_name!("h2")) {
            return false;
        }
        let heading = self.get_inner_text(node, false);
        text_similarity(&self.article_title, &heading) > 0.75
    }

    // ----- document preparation -----

    /// Removes the class="" attribute from every element in the given
    /// subtree, except those that match CLASSES_TO_PRESERVE and
    /// the classesToPreserve array from the options object.
    fn clean_classes(&mut self, root: NodeId) {
        let mut node = Some(root);
        while let Some(n) = node {
            if self.doc.is_element(n) {
                let class = self.doc.attr(n, "class").unwrap_or("");
                let kept: Vec<&str> = class
                    .split(|c: char| js::is_ws(c))
                    .filter(|c| self.classes_to_preserve.iter().any(|p| p == c))
                    .collect();
                let class_name = kept.join(" ");
                if class_name.is_empty() {
                    self.doc.remove_attr(n, "class");
                } else {
                    self.doc.set_attr(n, "class", &class_name);
                }
            }
            node = self.doc.next_in(n, root);
            // Only elements are visited by the original; text is skipped above.
        }
    }

    /// Converts each <a> and <img> uri in the given element to an absolute URI,
    /// ignoring #ref URIs.
    fn fix_relative_uris(&mut self, article_content: NodeId) {
        let base = self.doc.base_url();
        let base_uri = base.as_ref().map_or("about:blank", |u| u.as_str()).to_owned();
        let same_base = base_uri == self.doc.document_uri();
        let to_absolute = |uri: &str| -> String {
            // Leave hash links alone if the base URI matches the document URI:
            if same_base && uri.starts_with('#') {
                return uri.to_owned();
            }
            // Otherwise, resolve against base URI:
            resolve_url(base.as_ref(), uri).unwrap_or_else(|| uri.to_owned())
        };

        let links = self.get_all_nodes_with_tag(article_content, &[local_name!("a")]);
        for link in links {
            let Some(href) = self.doc.attr(link, "href").filter(|h| !h.is_empty()).map(str::to_owned) else { continue };
            // Remove links with javascript: URIs, since
            // they won't work after scripts have been removed from the page.
            if href.starts_with("javascript:") {
                let Some(_) = self.doc.parent(link) else { continue };
                // if the link only contains simple text content, it can be converted to a text node
                let first = self.doc.first_child(link);
                if first.is_some() && first == self.doc.last_child(link) && self.doc.is_text(first.unwrap()) {
                    let text = self.doc.text_content(link);
                    let t = self.doc.create_text(&text);
                    self.doc.replace(link, t);
                } else {
                    // if the link has multiple children, they should all be preserved
                    let container = self.doc.create_element("span");
                    while let Some(c) = self.doc.first_child(link) {
                        self.doc.append_child(container, c);
                    }
                    self.doc.replace(link, container);
                }
            } else {
                let abs = to_absolute(&href);
                self.doc.set_attr(link, "href", &abs);
            }
        }

        let medias = self.get_all_nodes_with_tag(
            article_content,
            &[
                local_name!("img"),
                local_name!("picture"),
                local_name!("figure"),
                local_name!("video"),
                local_name!("audio"),
                local_name!("source"),
            ],
        );
        for media in medias {
            for name in ["src", "poster"] {
                if let Some(v) = self.doc.attr(media, name).filter(|v| !v.is_empty()).map(str::to_owned) {
                    let abs = to_absolute(&v);
                    self.doc.set_attr(media, name, &abs);
                }
            }
            if let Some(srcset) = self.doc.attr(media, "srcset").filter(|v| !v.is_empty()).map(str::to_owned) {
                let new_srcset = SRCSET_URL.replace_all(&srcset, |c: &regex::Captures| {
                    format!("{}{}{}", to_absolute(&c[1]), c.get(2).map_or("", |m| m.as_str()), &c[3])
                });
                self.doc.set_attr(media, "srcset", &new_srcset);
            }
        }
    }

    fn simplify_nested_elements(&mut self, article_content: NodeId) {
        let mut node = Some(article_content);
        while let Some(n) = node {
            if self.doc.parent(n).is_some()
                && (self.is(n, &local_name!("div")) || self.is(n, &local_name!("section")))
                && !self.doc.id_attr(n).starts_with("readability")
            {
                if self.is_element_without_content(n) {
                    node = self.remove_and_get_next(n);
                    continue;
                } else if self.has_single_tag_inside_element(n, &local_name!("div"))
                    || self.has_single_tag_inside_element(n, &local_name!("section"))
                {
                    let child = self.doc.first_element_child(n).unwrap();
                    let attrs = self.doc.attrs(n).to_vec();
                    for attr in attrs {
                        self.doc.set_attr_node(child, attr);
                    }
                    self.doc.replace(n, child);
                    node = Some(child);
                    continue;
                }
            }
            node = self.get_next_node(n, false);
        }
    }

    /// Get the article title as an H1.
    fn get_article_title(&self) -> String {
        let orig_title = js::trim(&self.doc.title()).to_owned();
        let mut cur_title = orig_title.clone();
        let mut title_had_hierarchical_separators = false;

        // If there's a separator in the title, first remove the final part
        if TITLE_SEPARATOR.is_match(&cur_title) {
            title_had_hierarchical_separators = TITLE_HIERARCHICAL_SEPARATOR.is_match(&cur_title);
            let last = TITLE_SEPARATOR.find_iter(&orig_title).last().unwrap();
            cur_title = orig_title[..last.start()].to_owned();

            // If the resulting title is too short, remove the first part instead:
            if js::word_count(&cur_title) < 3 {
                cur_title = TITLE_FIRST_PART.replace(&orig_title, "").into_owned();
            }
        } else if cur_title.contains(": ") {
            // Check if we have an heading containing this exact string, so we
            // could assume it's the full title.
            let doc = self.doc.document();
            let trimmed_title = js::trim(&cur_title).to_owned();
            let matched = self
                .doc
                .elements_by_tags(doc, &[local_name!("h1"), local_name!("h2")])
                .into_iter()
                .any(|h| js::trim(&self.doc.text_content(h)) == trimmed_title);

            // If we don't, let's extract the title out of the original title string.
            if !matched {
                cur_title = orig_title[orig_title.rfind(':').unwrap() + 1..].to_owned();

                // If the title is now too short, try the first colon instead:
                if js::word_count(&cur_title) < 3 {
                    cur_title = orig_title[orig_title.find(':').unwrap() + 1..].to_owned();
                    // But if we have too many words before the colon there's something weird
                    // with the titles and the H tags so let's just use the original title instead
                } else if js::word_count(&orig_title[..orig_title.find(':').unwrap()]) > 5 {
                    cur_title = orig_title.clone();
                }
            }
        } else if js::len(&cur_title) > 150 || js::len(&cur_title) < 15 {
            let h_ones = self.doc.elements_by_tag(self.doc.document(), &local_name!("h1"));
            if h_ones.len() == 1 {
                cur_title = self.get_inner_text(h_ones[0], true);
            }
        }

        cur_title = js::normalize_spaces(js::trim(&cur_title)).into_owned();
        // If we now have 4 words or fewer as our title, and either no
        // 'hierarchical' separators (\, /, > or ») were found in the original
        // title or we decreased the number of words by more than 1 word, use
        // the original title.
        let cur_title_word_count = js::word_count(&cur_title) as i64;
        if cur_title_word_count <= 4
            && (!title_had_hierarchical_separators
                || cur_title_word_count
                    != js::word_count(&TITLE_SEPARATOR_CHARS.replace_all(&orig_title, "")) as i64 - 1)
        {
            cur_title = orig_title;
        }
        cur_title
    }

    /// Prepare the HTML document for readability to scrape it.
    /// This includes things like stripping javascript, CSS, and handling terrible markup.
    fn prep_document(&mut self) {
        let doc = self.doc.document();
        // Remove all style tags in head
        let styles = self.get_all_nodes_with_tag(doc, &[local_name!("style")]);
        self.remove_nodes(&styles, |_, _| true);

        if let Some(body) = self.doc.body() {
            self.replace_brs(body);
        }

        for font in self.get_all_nodes_with_tag(doc, &[local_name!("font")]) {
            self.set_node_tag(font, "SPAN");
        }
    }

    /// Replaces 2 or more successive <br> elements with a single <p>.
    /// Whitespace between <br> elements are ignored. For example:
    ///   <div>foo<br>bar<br> <br><br>abc</div>
    /// will become:
    ///   <div>foo<br>bar<p>abc</p></div>
    fn replace_brs(&mut self, elem: NodeId) {
        for br in self.get_all_nodes_with_tag(elem, &[local_name!("br")]) {
            let mut next = self.doc.next_sibling(br);

            // Whether 2 or more <br> elements have been found and replaced with a
            // <p> block.
            let mut replaced = false;

            // If we find a <br> chain, remove the <br>s until we hit another node
            // or non-whitespace. This leaves behind the first <br> in the chain
            // (which will be replaced with a <p> later).
            loop {
                next = self.next_node(next);
                let Some(n) = next else { break };
                if !self.is(n, &local_name!("br")) {
                    break;
                }
                replaced = true;
                let br_sibling = self.doc.next_sibling(n);
                self.doc.remove(n);
                next = br_sibling;
            }

            // If we removed a <br> chain, replace the remaining <br> with a <p>. Add
            // all sibling nodes as children of the <p> until we hit another <br>
            // chain.
            if replaced {
                let p = self.doc.create_element("p");
                self.doc.replace(br, p);

                let mut next = self.doc.next_sibling(p);
                while let Some(n) = next {
                    // If we've hit another <br><br>, we're done adding children to this <p>.
                    if self.is(n, &local_name!("br")) {
                        let next_elem = self.next_node(self.doc.next_sibling(n));
                        if next_elem.is_some_and(|e| self.is(e, &local_name!("br"))) {
                            break;
                        }
                    }

                    if !self.is_phrasing_content(n) {
                        break;
                    }

                    // Otherwise, make this node a child of the new <p>.
                    let sibling = self.doc.next_sibling(n);
                    self.doc.append_child(p, n);
                    next = sibling;
                }

                while let Some(last) = self.doc.last_child(p) {
                    if !self.is_whitespace(last) {
                        break;
                    }
                    self.doc.remove(last);
                }

                if let Some(parent) = self.doc.parent(p)
                    && self.is(parent, &local_name!("p"))
                {
                    self.set_node_tag(parent, "DIV");
                }
            }
        }
    }

    /// Prepare the article node for display. Clean out any inline styles,
    /// iframes, forms, strip extraneous <p> tags, etc.
    fn prep_article(&mut self, article_content: NodeId) {
        self.clean_styles(article_content);

        // Check for data tables before we continue, to avoid removing items in
        // those tables, which will often be isolated even though they're
        // visually linked to other content-ful elements (text, images, etc.).
        self.mark_data_tables(article_content);

        self.fix_lazy_images(article_content);

        // Clean out junk from the article content
        self.clean_conditionally(article_content, &local_name!("form"));
        self.clean_conditionally(article_content, &local_name!("fieldset"));
        self.clean(article_content, &local_name!("object"));
        self.clean(article_content, &local_name!("embed"));
        self.clean(article_content, &local_name!("footer"));
        self.clean(article_content, &local_name!("link"));
        self.clean(article_content, &local_name!("aside"));

        // Clean out elements with little content that have "share" in their id/class combinations from final top candidates,
        // which means we don't remove the top candidates even they have "share".
        let share_element_threshold = DEFAULT_CHAR_THRESHOLD;
        for top_candidate in self.doc.children(article_content) {
            self.clean_matched_nodes(top_candidate, |this, node, match_string| {
                is_share_element(match_string) && js::len(&this.doc.text_content(node)) < share_element_threshold
            });
        }

        self.clean(article_content, &local_name!("iframe"));
        self.clean(article_content, &local_name!("input"));
        self.clean(article_content, &local_name!("textarea"));
        self.clean(article_content, &local_name!("select"));
        self.clean(article_content, &local_name!("button"));
        self.clean_headers(article_content);

        // Do these last as the previous stuff may have removed junk
        // that will affect these
        self.clean_conditionally(article_content, &local_name!("table"));
        self.clean_conditionally(article_content, &local_name!("ul"));
        self.clean_conditionally(article_content, &local_name!("div"));

        // replace H1 with H2 as H1 should be only title that is displayed separately
        for h1 in self.get_all_nodes_with_tag(article_content, &[local_name!("h1")]) {
            self.set_node_tag(h1, "h2");
        }

        // Remove extra paragraphs
        let paragraphs = self.get_all_nodes_with_tag(article_content, &[local_name!("p")]);
        self.remove_nodes(&paragraphs, |this, paragraph| {
            // At this point, nasty iframes have been removed; only embedded video
            // ones remain.
            let has_content_element = this.doc.descendants(paragraph).any(|n| {
                matches!(
                    this.doc.local_name(n),
                    Some(&local_name!("img") | &local_name!("embed") | &local_name!("object") | &local_name!("iframe"))
                )
            });
            !has_content_element && !this.has_text(paragraph)
        });

        for br in self.get_all_nodes_with_tag(article_content, &[local_name!("br")]) {
            let next = self.next_node(self.doc.next_sibling(br));
            if next.is_some_and(|n| self.is(n, &local_name!("p"))) {
                self.doc.remove(br);
            }
        }

        // Remove single-cell tables
        for table in self.get_all_nodes_with_tag(article_content, &[local_name!("table")]) {
            let tbody = if self.has_single_tag_inside_element(table, &local_name!("tbody")) {
                self.doc.first_element_child(table).unwrap()
            } else {
                table
            };
            if self.has_single_tag_inside_element(tbody, &local_name!("tr")) {
                let row = self.doc.first_element_child(tbody).unwrap();
                if self.has_single_tag_inside_element(row, &local_name!("td")) {
                    let cell = self.doc.first_element_child(row).unwrap();
                    let all_phrasing = self.doc.child_nodes(cell).into_iter().all(|c| self.is_phrasing_content(c));
                    let cell = self.set_node_tag(cell, if all_phrasing { "P" } else { "DIV" });
                    if self.doc.parent(table).is_some() {
                        self.doc.replace(table, cell);
                    }
                }
            }
        }
    }

    /// Remove the style attribute on every e and under.
    fn clean_styles(&mut self, root: NodeId) {
        let mut node = Some(root);
        while let Some(e) = node {
            if !self.doc.is_element(e) {
                node = self.doc.next_skipping_children(e, root);
                continue;
            }
            // SVG subtrees are left alone.
            if self.doc.local_name(e) == Some(&local_name!("svg")) {
                node = self.doc.next_skipping_children(e, root);
                continue;
            }
            // Remove `style` and deprecated presentational attributes
            for attr in PRESENTATIONAL_ATTRIBUTES {
                self.doc.remove_attr(e, attr);
            }
            if self.doc.html_name(e).is_some_and(|n| DEPRECATED_SIZE_ATTRIBUTE_ELEMS.contains(n)) {
                self.doc.remove_attr(e, "width");
                self.doc.remove_attr(e, "height");
            }
            node = self.doc.next_in(e, root);
        }
    }

    /// Return an object indicating how many rows and columns this table has.
    fn get_row_and_column_count(&self, table: NodeId) -> (f64, f64) {
        let mut rows = 0.0;
        let mut columns: f64 = 0.0;
        for tr in self.doc.elements_by_tag(table, &local_name!("tr")) {
            let rowspan = self.doc.attr(tr, "rowspan").and_then(js::parse_int);
            rows += match rowspan {
                Some(r) if r != 0.0 => r,
                _ => 1.0,
            };

            // Now look for column-related info
            let mut columns_in_this_row = 0.0;
            for td in self.doc.elements_by_tag(tr, &local_name!("td")) {
                let colspan = self.doc.attr(td, "colspan").and_then(js::parse_int);
                columns_in_this_row += match colspan {
                    Some(c) if c != 0.0 => c,
                    _ => 1.0,
                };
            }
            columns = columns.max(columns_in_this_row);
        }
        (rows, columns)
    }

    /// Look for 'data' (as opposed to 'layout') tables, for which we use
    /// similar checks as
    /// https://searchfox.org/mozilla-central/rev/f82d5c549f046cb64ce5602bfd894b7ae807c8f8/accessible/generic/TableAccessible.cpp#19
    fn mark_data_tables(&mut self, root: NodeId) {
        for table in self.doc.elements_by_tag(root, &local_name!("table")) {
            let v = self.is_data_table_heuristic(table);
            self.set_data_table(table, v);
        }
    }

    fn is_data_table_heuristic(&self, table: NodeId) -> bool {
        if self.doc.attr(table, "role") == Some("presentation") {
            return false;
        }
        if self.doc.attr(table, "datatable") == Some("0") {
            return false;
        }
        if self.doc.attr(table, "summary").is_some_and(|s| !s.is_empty()) {
            return true;
        }

        if let Some(caption) = self.doc.first_element_by_tag(table, &local_name!("caption"))
            && self.doc.first_child(caption).is_some()
        {
            return true;
        }

        // If the table has a descendant with any of these tags, consider a data table:
        let data_table_descendants = [
            local_name!("col"),
            local_name!("colgroup"),
            local_name!("tfoot"),
            local_name!("thead"),
            local_name!("th"),
        ];
        if data_table_descendants.iter().any(|t| self.doc.first_element_by_tag(table, t).is_some()) {
            return true;
        }

        // Nested tables indicate a layout table:
        if self.doc.first_element_by_tag(table, &local_name!("table")).is_some() {
            return false;
        }

        let (rows, columns) = self.get_row_and_column_count(table);

        if columns == 1.0 || rows == 1.0 {
            // single colum/row tables are commonly used for page layout purposes.
            return false;
        }

        if rows >= 10.0 || columns > 4.0 {
            return true;
        }
        // Now just go by size entirely:
        rows * columns > 10.0
    }

    /// `img.src`: the attribute resolved against the document base URL.
    fn img_src(&self, elem: NodeId, base: Option<&url::Url>) -> Option<String> {
        let raw = self.doc.attr(elem, "src")?;
        if !self.is(elem, &local_name!("img")) {
            return None;
        }
        Some(resolve_url(base, raw).unwrap_or_else(|| raw.to_owned()))
    }

    /// convert images and figures that have properties like data-src into images that can be loaded without JS
    fn fix_lazy_images(&mut self, root: NodeId) {
        let elems =
            self.get_all_nodes_with_tag(root, &[local_name!("img"), local_name!("picture"), local_name!("figure")]);
        // Only images change below, so the base URL stays the same.
        let base = if elems.is_empty() { None } else { self.doc.base_url() };
        for elem in elems {
            // Only <img> has `src` / `srcset` properties.
            let is_img = self.is(elem, &local_name!("img"));
            // In some sites (e.g. Kotaku), they put 1px square image as base64 data uri in the src attribute.
            // So, here we check if the data uri is too short, just might as well remove it.
            if let Some(src) = self.img_src(elem, base.as_ref()).filter(|s| !s.is_empty())
                && let Some(parts) = B64_DATA_URL.captures(&src)
            {
                // Make sure it's not SVG, because SVG can have a meaningful image in under 133 bytes.
                if &parts[1] == "image/svg+xml" {
                    continue;
                }

                // Make sure this element has other attributes which contains image.
                // If it doesn't, then this src is important and shouldn't be removed.
                let src_could_be_removed = self
                    .doc
                    .attrs(elem)
                    .iter()
                    .any(|a| attr_qualified_name(&a.name) != "src" && IMAGE_EXT_I.is_match(&a.value));

                // Here we assume if image is less than 100 bytes (or 133 after encoded to base64)
                // it will be too small, therefore it might be placeholder image.
                if src_could_be_removed {
                    let b64starts = js::len(&parts[0]);
                    let b64length = js::len(&src) - b64starts;
                    if b64length < 133 {
                        self.doc.remove_attr(elem, "src");
                    }
                }
            }

            let has_src = is_img && self.img_src(elem, base.as_ref()).is_some_and(|s| !s.is_empty());
            let has_srcset = is_img && self.doc.attr(elem, "srcset").is_some_and(|s| !s.is_empty() && s != "null");
            if (has_src || has_srcset) && !self.class_name(elem).to_lowercase().contains("lazy") {
                continue;
            }

            let mut j = 0;
            while j < self.doc.attrs(elem).len() {
                let attr = &self.doc.attrs(elem)[j];
                j += 1;
                let name = attr_qualified_name(&attr.name);
                if name == "src" || name == "srcset" || name == "alt" {
                    continue;
                }
                let value = attr.value.to_string();
                let copy_to = if LAZY_SRCSET.is_match(&value) {
                    "srcset"
                } else if LAZY_SRC.is_match(&value) {
                    "src"
                } else {
                    continue;
                };
                //if this is an img or picture, set the attribute directly
                if self.is(elem, &local_name!("img")) || self.is(elem, &local_name!("picture")) {
                    self.doc.set_attr(elem, copy_to, &value);
                } else if self.is(elem, &local_name!("figure"))
                    && self.get_all_nodes_with_tag(elem, &[local_name!("img"), local_name!("picture")]).is_empty()
                {
                    //if the item is a <figure> that does not contain an image or picture, create one and place it inside the figure
                    //see the nytimes-3 testcase for an example
                    let img = self.doc.create_element("img");
                    self.doc.set_attr(img, copy_to, &value);
                    self.doc.append_child(elem, img);
                }
            }
        }
    }

    fn embed_matches_video(&self, e: NodeId) -> bool {
        self.doc.attrs(e).iter().any(|a| self.allowed_video_regex.is_match(&a.value))
    }

    /// Clean a node of all elements of type "tag".
    /// (Unless it's a youtube/vimeo video. People love movies.)
    fn clean(&mut self, e: NodeId, tag: &LocalName) {
        let is_embed = matches!(*tag, local_name!("object") | local_name!("embed") | local_name!("iframe"));
        let nodes = self.get_all_nodes_with_tag(e, std::slice::from_ref(tag));
        self.remove_nodes(&nodes, |this, element| {
            // Allow youtube and vimeo videos through as people usually want to see those.
            // (The original also checks `element.tagName === "object"` against the
            // inner HTML, which never matches an upper-case tag name.)
            !(is_embed && this.embed_matches_video(element))
        });
    }

    /// Clean an element of all tags of type "tag" if they look fishy.
    /// "Fishy" is an algorithm based on content length, classnames, link density, number of images & embeds, etc.
    fn clean_conditionally(&mut self, e: NodeId, tag: &LocalName) {
        if !self.flag_is_active(FLAG_CLEAN_CONDITIONALLY) {
            return;
        }

        // Gather counts for other typical elements embedded within.
        // Traverse backwards so we can remove nodes at the same time
        // without effecting the traversal.
        //
        // TODO: Consider taking into account original contentScore here.
        let nodes = self.get_all_nodes_with_tag(e, std::slice::from_ref(tag));
        self.remove_nodes(&nodes, |this, node| this.should_clean_conditionally(node, tag));
    }

    fn should_clean_conditionally(&self, node: NodeId, tag: &LocalName) -> bool {
        let doc = &*self.doc;
        let mut is_list = matches!(*tag, local_name!("ul") | local_name!("ol"));
        if !is_list {
            let list_length: usize = doc
                .descendants(node)
                .filter(|&n| matches!(doc.local_name(n), Some(&local_name!("ul") | &local_name!("ol"))))
                .map(|n| self.inner_text_len(n))
                .sum();
            let node_length = self.inner_text_len(node);
            is_list = if node_length == 0 { list_length > 0 } else { list_length as f64 / node_length as f64 > 0.9 };
        }

        // First check if this node IS data table, in which case don't remove it.
        if *tag == local_name!("table") && self.is_data_table(node) {
            return false;
        }

        // Next check if we're inside a data table, in which case don't remove it as well.
        if self.has_ancestor_tag(node, &local_name!("table"), -1, Some(&|t| self.is_data_table(t))) {
            return false;
        }

        if self.has_ancestor_tag(node, &local_name!("code"), 0, None) {
            return false;
        }

        // keep element if it has a data tables
        if doc.descendants(node).any(|t| doc.local_name(t) == Some(&local_name!("table")) && self.is_data_table(t)) {
            return false;
        }

        let weight = self.get_class_weight(node);
        let content_score = 0.0;
        if weight + content_score < 0.0 {
            return true;
        }

        let measure = self.measure_inner_text(node);
        if measure.commas >= 10 {
            return false;
        }

        // If there are not very many commas, and the number of
        // non-paragraph elements is more than paragraphs or other
        // ominous signs, remove the element.
        let mut p = 0usize;
        let mut img = 0usize;
        let mut li_total = 0usize;
        let mut input = 0usize;
        let mut embed_count = 0usize;
        for n in doc.descendants(node) {
            match doc.local_name(n) {
                Some(&local_name!("p")) => p += 1,
                Some(&local_name!("img")) => img += 1,
                Some(&local_name!("li")) => li_total += 1,
                Some(&local_name!("input")) => input += 1,
                Some(&local_name!("object") | &local_name!("embed") | &local_name!("iframe")) => {
                    // If this embed has attribute that matches video regex, don't delete it.
                    if self.embed_matches_video(n) {
                        return false;
                    }
                    embed_count += 1;
                }
                _ => {}
            }
        }
        let li = li_total as f64 - 100.0;
        let heading_density = self.get_text_density(
            node,
            &[
                local_name!("h1"),
                local_name!("h2"),
                local_name!("h3"),
                local_name!("h4"),
                local_name!("h5"),
                local_name!("h6"),
            ],
        );

        // toss any node whose inner text contains nothing but suspicious words
        // (The longest suspicious word is 13 characters.)
        let content_length = measure.len;
        if content_length < 32 {
            let text = self.get_inner_text(node, true);
            if AD_WORDS.is_match(&text) || LOADING_WORDS.is_match(&text) {
                return true;
            }
        }

        let link_density = self.get_link_density(node);
        let textish_tags = [
            local_name!("span"),
            local_name!("li"),
            local_name!("td"),
            local_name!("blockquote"),
            local_name!("dl"),
            local_name!("div"),
            local_name!("img"),
            local_name!("ol"),
            local_name!("p"),
            local_name!("pre"),
            local_name!("table"),
            local_name!("ul"),
        ];
        let text_density = self.get_text_density(node, &textish_tags);
        let is_figure_child = self.has_ancestor_tag(node, &local_name!("figure"), 0, None);

        // apply shadiness checks, then check for exceptions
        let (pf, imgf) = (p as f64, img as f64);
        let have_to_remove = (!is_figure_child && img > 1 && pf / imgf < 0.5)
            || (!is_list && li > pf)
            || (input as f64 > (pf / 3.0).floor())
            || (!is_list
                && !is_figure_child
                && heading_density < 0.9
                && content_length < 25
                && (img == 0 || img > 2)
                && link_density > 0.0)
            || (!is_list && weight < 25.0 && link_density > 0.2 + self.link_density_modifier)
            || (weight >= 25.0 && link_density > 0.5 + self.link_density_modifier)
            || ((embed_count == 1 && content_length < 75) || embed_count > 1)
            || (img == 0 && text_density == 0.0);

        // Allow simple lists of images to remain in pages
        if is_list && have_to_remove {
            let mut c = doc.first_element_child(node);
            while let Some(child) = c {
                // Don't filter in lists with li's that contain more than one child
                if doc.element_child_count(child) > 1 {
                    return have_to_remove;
                }
                c = doc.next_element_sibling(child);
            }
            let li_count = doc.count_elements_by_tag(node, &local_name!("li"));
            // Only allow the list to remain if every li contains an image
            if img == li_count {
                return false;
            }
        }
        have_to_remove
    }

    /// Clean out elements that match the specified conditions
    fn clean_matched_nodes(&mut self, e: NodeId, filter: impl Fn(&Self, NodeId, &str) -> bool) {
        let end_of_search_marker_node = self.get_next_node(e, true);
        let mut next = self.get_next_node(e, false);
        while let Some(n) = next {
            if Some(n) == end_of_search_marker_node {
                break;
            }
            let match_string = self.match_string(n);
            if filter(self, n, &match_string) {
                next = self.remove_and_get_next(n);
            } else {
                next = self.get_next_node(n, false);
            }
        }
    }

    /// Clean out spurious headers from an Element.
    fn clean_headers(&mut self, e: NodeId) {
        let heading_nodes = self.get_all_nodes_with_tag(e, &[local_name!("h1"), local_name!("h2")]);
        self.remove_nodes(&heading_nodes, |this, node| this.get_class_weight(node) < 0.0);
    }

    // ----- metadata -----

    /// Try to extract metadata from JSON-LD object.
    /// For now, only Schema.org objects of type Article or its subtypes are supported.
    fn get_json_ld(&self) -> JsonLd {
        let scripts = self.doc.elements_by_tag(self.doc.document(), &local_name!("script"));
        for script in scripts {
            if self.doc.attr(script, "type") != Some("application/ld+json") {
                continue;
            }
            if let Ok(Some(m)) = self.json_ld_from_script(script) {
                return m;
            }
        }
        JsonLd::default()
    }

    /// `Err` and `Ok(None)` both mean "try the next script".
    fn json_ld_from_script(&self, script: NodeId) -> Result<Option<JsonLd>, ()> {
        use serde_json::Value;
        // Strip CDATA markers if present
        let text = self.doc.text_content(script);
        let mut content: &str = &text;
        let lead = content.trim_start_matches(js::is_ws);
        if let Some(rest) = lead.strip_prefix("<![CDATA[") {
            content = rest;
        }
        if let Some(i) = content.rfind("]]>")
            && js::is_blank(&content[i + 3..])
        {
            content = &content[..i];
        }
        let parsed: Value = serde_json::from_str(content).map_err(|_| ())?;

        let mut parsed: Value = if let Value::Array(items) = parsed {
            let mut found = None;
            for it in items {
                let t = js_get(&it, "@type")?;
                if truthy(t) && json_ld_type_matches(&it)? {
                    found = Some(it);
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => return Ok(None),
            }
        } else {
            parsed
        };

        let context = js_get(&parsed, "@context")?;
        let matches = match context {
            Some(Value::String(s)) => SCHEMA_DOT_ORG.is_match(s),
            Some(Value::Null) => return Err(()),
            Some(c @ (Value::Object(_) | Value::Array(_))) => {
                matches!(js_get(c, "@vocab")?, Some(Value::String(v)) if SCHEMA_DOT_ORG.is_match(v))
            }
            _ => false,
        };
        if !matches {
            return Ok(None);
        }

        if !truthy(js_get(&parsed, "@type")?)
            && let Some(Value::Array(graph)) = js_get(&parsed, "@graph")?
        {
            let mut found = None;
            for it in graph {
                if json_ld_type_matches(it)? {
                    found = Some(it.clone());
                    break;
                }
            }
            match found {
                Some(f) => parsed = f,
                None => return Ok(None),
            }
        }

        if !json_ld_type_matches(&parsed)? {
            return Ok(None);
        }

        let mut metadata = JsonLd::default();
        let name = json_str(&parsed, "name");
        let headline = json_str(&parsed, "headline");
        match (name, headline) {
            (Some(name), Some(headline)) if name != headline => {
                // we have both name and headline element in the JSON-LD. They should both be the same but some websites like aktualne.cz
                // put their own name into "name" and the article title to "headline" which confuses Readability. So we try to check if either
                // "name" or "headline" closely matches the html title, and if so, use that one. If not, then we use "name" by default.
                let title = self.get_article_title();
                let name_matches = text_similarity(name, &title) > 0.75;
                let headline_matches = text_similarity(headline, &title) > 0.75;
                metadata.title = Some(if headline_matches && !name_matches { headline } else { name }.to_owned());
            }
            (Some(name), _) => metadata.title = Some(js::trim(name).to_owned()),
            (None, Some(headline)) => metadata.title = Some(js::trim(headline).to_owned()),
            (None, None) => {}
        }
        if let Some(author) = parsed.get("author").filter(|a| truthy(Some(a))) {
            if let Some(name) = json_str(author, "name") {
                metadata.byline = Some(js::trim(name).to_owned());
            } else if let Value::Array(authors) = author
                && authors.first().is_some_and(|a| truthy(Some(a)) && json_str(a, "name").is_some())
            {
                let names: Vec<&str> = authors.iter().filter_map(|a| json_str(a, "name")).map(js::trim).collect();
                metadata.byline = Some(names.join(", "));
            }
        }
        if let Some(d) = json_str(&parsed, "description") {
            metadata.excerpt = Some(js::trim(d).to_owned());
        }
        if let Some(publisher) = parsed.get("publisher").filter(|p| truthy(Some(p)))
            && let Some(n) = json_str(publisher, "name")
        {
            metadata.site_name = Some(js::trim(n).to_owned());
        }
        if let Some(d) = json_str(&parsed, "datePublished") {
            metadata.date_published = Some(js::trim(d).to_owned());
        }
        Ok(Some(metadata))
    }

    /// Attempts to get excerpt and byline metadata for the article.
    fn get_article_metadata(&self, jsonld: JsonLd) -> Metadata {
        let mut values: HashMap<String, String> = HashMap::new();
        let doc = self.doc.document();

        for element in self.doc.elements_by_tag(doc, &local_name!("meta")) {
            let element_name = self.doc.attr(element, "name");
            let element_property = self.doc.attr(element, "property");
            let Some(content) = self.doc.attr(element, "content").filter(|c| !c.is_empty()) else { continue };
            let mut matched = false;

            if let Some(property) = element_property.filter(|p| !p.is_empty())
                && let Some(m) = META_PROPERTY.find(property)
            {
                matched = true;
                // Convert to lowercase, and remove any whitespace
                // so we can match below.
                let name: String = m.as_str().to_lowercase().chars().filter(|c| !js::is_ws(*c)).collect();
                // multiple authors
                values.insert(name, js::trim(content).to_owned());
            }
            if !matched && let Some(name) = element_name.filter(|n| !n.is_empty() && META_NAME.is_match(n)) {
                // Convert to lowercase, remove any whitespace, and convert dots
                // to colons so we can match below.
                let name: String = name
                    .to_lowercase()
                    .chars()
                    .filter(|c| !js::is_ws(*c))
                    .map(|c| if c == '.' { ':' } else { c })
                    .collect();
                values.insert(name, js::trim(content).to_owned());
            }
        }

        let get = |k: &str| values.get(k);
        let first = |init: Option<String>, keys: &[&str]| -> Option<String> {
            keys.iter().fold(non_empty(init), |acc, k| or(acc, get(k)))
        };

        // get title
        let mut title = first(
            jsonld.title,
            &[
                "dc:title",
                "dcterm:title",
                "og:title",
                "weibo:article:title",
                "weibo:webpage:title",
                "title",
                "twitter:title",
                "parsely-title",
            ],
        );
        if title.is_none() {
            title = Some(self.get_article_title());
        }

        let article_author = get("article:author").filter(|a| url::Url::parse(a).is_err()).cloned();

        // get author
        let byline = or(
            first(jsonld.byline, &["dc:creator", "dcterm:creator", "author", "parsely-author"]),
            article_author.as_ref(),
        );

        // get description
        let excerpt = first(
            jsonld.excerpt,
            &[
                "dc:description",
                "dcterm:description",
                "og:description",
                "weibo:article:description",
                "weibo:webpage:description",
                "description",
                "twitter:description",
            ],
        );

        // get site name
        let site_name = first(jsonld.site_name, &["og:site_name"]);

        // get article published time
        let published_time = first(jsonld.date_published, &["article:published_time", "parsely-pub-date"]);

        // in many sites the meta value is escaped with HTML entities,
        // so here we need to unescape it
        Metadata {
            title: unescape_html_entities(title),
            byline: unescape_html_entities(byline),
            excerpt: unescape_html_entities(excerpt),
            site_name: unescape_html_entities(site_name),
            published_time: unescape_html_entities(published_time),
        }
    }

    /// Check if node is image, or if node contains exactly only one image
    /// whether as a direct child or as its descendants.
    fn is_single_image(&self, node: NodeId) -> bool {
        let mut node = Some(node);
        while let Some(n) = node {
            if self.is(n, &local_name!("img")) {
                return true;
            }
            if self.doc.element_child_count(n) != 1 || self.has_text(n) {
                return false;
            }
            node = self.doc.first_element_child(n);
        }
        false
    }

    /// Find all <noscript> that are located after <img> nodes, and which contain only one
    /// <img> element. Replace the first image with the image from inside the <noscript> tag,
    /// and remove the <noscript> tag. This improves the quality of the images we use on
    /// some sites (e.g. Medium).
    fn unwrap_noscript_images(&mut self) {
        let doc = self.doc.document();
        // Find img without source or attributes that might contains image, and remove it.
        // This is done to prevent a placeholder img is replaced by img from noscript in next step.
        for img in self.doc.elements_by_tag(doc, &local_name!("img")) {
            let keep = self.doc.attrs(img).iter().any(|a| {
                matches!(&*attr_qualified_name(&a.name), "src" | "srcset" | "data-src" | "data-srcset")
                    || IMAGE_EXT_I.is_match(&a.value)
            });
            if !keep {
                self.doc.remove(img);
            }
        }

        // Next find noscript and try to extract its image
        for noscript in self.doc.elements_by_tag(doc, &local_name!("noscript")) {
            // Parse content of noscript and make sure it only contains image
            if !self.is_single_image(noscript) {
                continue;
            }
            // If noscript has previous sibling and it only contains image,
            // replace it with noscript content. However we also keep old
            // attributes that might contains image.
            let Some(prev_element) = self.doc.prev_element_sibling(noscript) else { continue };
            if !self.is_single_image(prev_element) {
                continue;
            }
            let tmp = self.doc.create_element("div");
            for c in self.doc.child_nodes(noscript) {
                let copy = self.doc.deep_clone(c);
                self.doc.append_child(tmp, copy);
            }

            let prev_img = if self.is(prev_element, &local_name!("img")) {
                prev_element
            } else {
                self.doc.first_element_by_tag(prev_element, &local_name!("img")).unwrap()
            };

            let Some(new_img) = self.doc.first_element_by_tag(tmp, &local_name!("img")) else { continue };
            for attr in self.doc.attrs(prev_img).to_vec() {
                if attr.value.is_empty() {
                    continue;
                }
                let name = attr_qualified_name(&attr.name).into_owned();
                if name == "src" || name == "srcset" || IMAGE_EXT_I.is_match(&attr.value) {
                    if self.doc.attr(new_img, &name) == Some(&*attr.value) {
                        continue;
                    }
                    let attr_name = if self.doc.has_attr(new_img, &name) { format!("data-old-{name}") } else { name };
                    self.doc.set_attr(new_img, &attr_name, &attr.value);
                }
            }

            if let Some(replacement) = self.doc.first_element_child(tmp) {
                self.doc.replace(prev_element, replacement);
            }
        }
    }

    /// Removes script tags from the document.
    fn remove_scripts(&mut self) {
        let doc = self.doc.document();
        let scripts = self.get_all_nodes_with_tag(doc, &[local_name!("script"), local_name!("noscript")]);
        self.remove_nodes(&scripts, |_, _| true);
    }

    // ----- the main algorithm -----

    /// grabArticle - Using a variety of metrics (content score, classname, element types), find the content that is
    /// most likely to be the stuff a user wants to read. Then return it wrapped up in a div.
    fn grab_article(&mut self) -> Option<NodeId> {
        let page = self.doc.body()?;
        // `page.innerHTML` is saved as a copy of the subtree and restored by copying.
        let page_cache = self.doc.deep_clone(page);

        loop {
            let strip_unlikely_candidates = self.flag_is_active(FLAG_STRIP_UNLIKELYS);

            // First, node prepping. Trash nodes that look cruddy (like ones with the
            // class name "comment", etc), and turn divs into P tags where they have been
            // used inappropriately (as in, where they contain no other block level elements.)
            let mut elements_to_score: Vec<NodeId> = Vec::new();
            let mut node = self.doc.document_element();
            let mut should_remove_title_header = true;

            while let Some(mut n) = node {
                if self.is(n, &local_name!("html")) {
                    self.article_lang = self.doc.attr(n, "lang").map(str::to_owned);
                }

                let match_string = self.match_string(n);

                if !self.is_probably_visible(n) {
                    node = self.remove_and_get_next(n);
                    continue;
                }

                // User is not able to see elements applied with both "aria-modal = true" and "role = dialog"
                if self.doc.attr(n, "aria-modal") == Some("true") && self.doc.attr(n, "role") == Some("dialog") {
                    node = self.remove_and_get_next(n);
                    continue;
                }

                // If we don't have a byline yet check to see if this node is a byline; if it is store the byline and remove the node.
                if self.article_byline.is_none()
                    && self.metadata_byline.is_none()
                    && self.is_valid_byline(n, &match_string)
                {
                    // Find child node matching [itemprop="name"] and use that if it exists for a more accurate author name byline
                    let end_of_search_marker_node = self.get_next_node(n, true);
                    let mut next = self.get_next_node(n, false);
                    let mut item_prop_name_node = None;
                    while let Some(x) = next {
                        if Some(x) == end_of_search_marker_node {
                            break;
                        }
                        if self.doc.attr(x, "itemprop").is_some_and(|i| i.contains("name")) {
                            item_prop_name_node = Some(x);
                            break;
                        }
                        next = self.get_next_node(x, false);
                    }
                    let text = self.doc.text_content(item_prop_name_node.unwrap_or(n));
                    self.article_byline = Some(js::trim(&text).to_owned());
                    node = self.remove_and_get_next(n);
                    continue;
                }

                if should_remove_title_header && self.header_duplicates_title(n) {
                    should_remove_title_header = false;
                    node = self.remove_and_get_next(n);
                    continue;
                }

                // Remove unlikely candidates
                if strip_unlikely_candidates {
                    if UNLIKELY_CANDIDATES.is_match(&match_string)
                        && !OK_MAYBE_ITS_A_CANDIDATE.is_match(&match_string)
                        && !self.has_ancestor_tag(n, &local_name!("table"), 0, None)
                        && !self.has_ancestor_tag(n, &local_name!("code"), 0, None)
                        && !self.is(n, &local_name!("body"))
                        && !self.is(n, &local_name!("a"))
                    {
                        node = self.remove_and_get_next(n);
                        continue;
                    }

                    if self.doc.attr(n, "role").is_some_and(|r| UNLIKELY_ROLES.contains(&r)) {
                        node = self.remove_and_get_next(n);
                        continue;
                    }
                }

                // Remove DIV, SECTION, and HEADER nodes without any content(e.g. text, image, video, or iframe).
                if matches!(
                    self.doc.html_name(n),
                    Some(
                        &local_name!("div")
                            | &local_name!("section")
                            | &local_name!("header")
                            | &local_name!("h1")
                            | &local_name!("h2")
                            | &local_name!("h3")
                            | &local_name!("h4")
                            | &local_name!("h5")
                            | &local_name!("h6")
                    )
                ) && self.is_element_without_content(n)
                {
                    node = self.remove_and_get_next(n);
                    continue;
                }

                if self.doc.html_name(n).is_some_and(is_score_tag) {
                    elements_to_score.push(n);
                }

                // Turn all divs that don't have children block level elements into p's
                if self.is(n, &local_name!("div")) {
                    // Put phrasing content into paragraphs.
                    let mut p: Option<NodeId> = None;
                    let mut child_node = self.doc.first_child(n);
                    while let Some(c) = child_node {
                        let next_sibling = self.doc.next_sibling(c);
                        if self.is_phrasing_content(c) {
                            if let Some(p) = p {
                                self.doc.append_child(p, c);
                            } else if !self.is_whitespace(c) {
                                let new_p = self.doc.create_element("p");
                                self.doc.replace(c, new_p);
                                self.doc.append_child(new_p, c);
                                p = Some(new_p);
                            }
                        } else if let Some(pp) = p {
                            while let Some(l) = self.doc.last_child(pp) {
                                if !self.is_whitespace(l) {
                                    break;
                                }
                                self.doc.remove(l);
                            }
                            p = None;
                        }
                        child_node = next_sibling;
                    }

                    // Sites like http://mobile.slate.com encloses each paragraph with a DIV
                    // element. DIVs with only a P element inside and no text content can be
                    // safely converted into plain P elements to avoid confusing the scoring
                    // algorithm with DIVs with are, in practice, paragraphs.
                    if self.has_single_tag_inside_element(n, &local_name!("p")) && self.get_link_density(n) < 0.25 {
                        let new_node = self.doc.first_element_child(n).unwrap();
                        self.doc.replace(n, new_node);
                        n = new_node;
                        elements_to_score.push(n);
                    } else if !self.has_child_block_element(n) {
                        n = self.set_node_tag(n, "P");
                        elements_to_score.push(n);
                    }
                }
                node = self.get_next_node(n, false);
            }

            // Loop through all paragraphs, and assign a score to them based on how content-y they look.
            // Then add their score to their parent node.
            //
            // A score is determined by things like number of commas, class names, etc. Maybe eventually link density.
            let mut candidates: Vec<NodeId> = Vec::new();
            for &element_to_score in &elements_to_score {
                match self.doc.parent(element_to_score) {
                    Some(p) if self.doc.is_element(p) => {}
                    _ => continue,
                }

                // If this paragraph is less than 25 characters, don't even count it.
                let measure = self.measure_inner_text(element_to_score);
                let inner_len = measure.len;
                if inner_len < 25 {
                    continue;
                }

                // Exclude nodes with no ancestor.
                let ancestors = self.get_node_ancestors(element_to_score, 5);
                if ancestors.is_empty() {
                    continue;
                }

                let mut content_score = 0.0;

                // Add a point for the paragraph itself as a base.
                content_score += 1.0;

                // Add points for any commas within this paragraph.
                let commas = if measure.commas == 0 && inner_len < 200 {
                    // Other comma characters are not ASCII; skip the scan for plain text.
                    let mut other = 0;
                    self.doc.for_each_text(element_to_score, |t| other += count_other_commas(t));
                    other
                } else {
                    let mut all = 0;
                    self.doc.for_each_text(element_to_score, |t| all += count_commas(t));
                    all
                };
                content_score += (commas + 1) as f64;

                // For every 100 characters in this paragraph, add another point. Up to 3 points.
                content_score += ((inner_len / 100) as f64).min(3.0);

                // Initialize and score ancestors.
                for (level, &ancestor) in ancestors.iter().enumerate() {
                    if !self.doc.is_element(ancestor) {
                        continue;
                    }
                    match self.doc.parent(ancestor) {
                        Some(p) if self.doc.is_element(p) => {}
                        _ => continue,
                    }

                    if self.score(ancestor).is_none() {
                        self.initialize_node(ancestor);
                        candidates.push(ancestor);
                    }

                    // Node score divider:
                    // - parent:             1 (no division)
                    // - grandparent:        2
                    // - great grandparent+: ancestor level * 3
                    let score_divider = match level {
                        0 => 1.0,
                        1 => 2.0,
                        _ => level as f64 * 3.0,
                    };
                    let s = self.score(ancestor).unwrap() + content_score / score_divider;
                    self.set_score(ancestor, s);
                }
            }

            // After we've calculated scores, loop through all of the possible
            // candidate nodes we found and find the one with the highest score.
            let mut top_candidates: Vec<NodeId> = Vec::new();
            for &candidate in &candidates {
                // Scale the final candidates score based on link density. Good content
                // should have a relatively small link density (5% or less) and be mostly
                // unaffected by this operation.
                let candidate_score = self.score(candidate).unwrap() * (1.0 - self.get_link_density(candidate));
                self.set_score(candidate, candidate_score);

                for t in 0..self.nb_top_candidates {
                    let better = match top_candidates.get(t) {
                        None => true,
                        Some(&a) => candidate_score > self.score(a).unwrap(),
                    };
                    if better {
                        top_candidates.insert(t, candidate);
                        if top_candidates.len() > self.nb_top_candidates {
                            top_candidates.pop();
                        }
                        break;
                    }
                }
            }

            let top_candidate = top_candidates.first().copied();
            let needed_to_create_top_candidate;
            let mut parent_of_top_candidate;

            // If we still have no top candidate, just use the body as a last resort.
            // We also have to copy the body node so it is something we can modify.
            let tc = match top_candidate {
                Some(tc) if !self.is(tc, &local_name!("body")) => {
                    needed_to_create_top_candidate = false;
                    let mut tc = tc;
                    // Find a better top candidate node if it contains (at least three) nodes which belong to `topCandidates` array
                    // and whose scores are quite closed with current `topCandidate` node.
                    let tc_score = self.score(tc).unwrap();
                    let alternative_candidate_ancestors: Vec<Vec<NodeId>> = top_candidates[1..]
                        .iter()
                        .filter(|&&c| self.score(c).unwrap() / tc_score >= 0.75)
                        .map(|&c| self.get_node_ancestors(c, 0))
                        .collect();
                    const MINIMUM_TOPCANDIDATES: usize = 3;
                    if alternative_candidate_ancestors.len() >= MINIMUM_TOPCANDIDATES {
                        parent_of_top_candidate = self.doc.parent(tc);
                        while let Some(p) = parent_of_top_candidate {
                            if self.is(p, &local_name!("body")) || !self.doc.is_element(p) {
                                break;
                            }
                            let mut lists_containing_this_ancestor = 0;
                            for ancestors in &alternative_candidate_ancestors {
                                if lists_containing_this_ancestor >= MINIMUM_TOPCANDIDATES {
                                    break;
                                }
                                lists_containing_this_ancestor += usize::from(ancestors.contains(&p));
                            }
                            if lists_containing_this_ancestor >= MINIMUM_TOPCANDIDATES {
                                tc = p;
                                break;
                            }
                            parent_of_top_candidate = self.doc.parent(p);
                        }
                    }
                    if self.score(tc).is_none() {
                        self.initialize_node(tc);
                    }

                    // Because of our bonus system, parents of candidates might have scores
                    // themselves. They get half of the node. There won't be nodes with higher
                    // scores than our topCandidate, but if we see the score going *up* in the first
                    // few steps up the tree, that's a decent sign that there might be more content
                    // lurking in other places that we want to unify in. The sibling stuff
                    // below does some of that - but only if we've looked high enough up the DOM
                    // tree.
                    parent_of_top_candidate = self.doc.parent(tc);
                    let mut last_score = self.score(tc).unwrap();
                    // The scores shouldn't get too low.
                    let score_threshold = last_score / 3.0;
                    while let Some(p) = parent_of_top_candidate {
                        if self.is(p, &local_name!("body")) || !self.doc.is_element(p) {
                            break;
                        }
                        let Some(parent_score) = self.score(p) else {
                            parent_of_top_candidate = self.doc.parent(p);
                            continue;
                        };
                        if parent_score < score_threshold {
                            break;
                        }
                        if parent_score > last_score {
                            // Alright! We found a better parent to use.
                            tc = p;
                            break;
                        }
                        last_score = parent_score;
                        parent_of_top_candidate = self.doc.parent(p);
                    }

                    // If the top candidate is the only child, use parent instead. This will help sibling
                    // joining logic when adjacent content is actually located in parent's sibling node.
                    parent_of_top_candidate = self.doc.parent(tc);
                    while let Some(p) = parent_of_top_candidate {
                        if self.is(p, &local_name!("body"))
                            || !self.doc.is_element(p)
                            || self.doc.element_child_count(p) != 1
                        {
                            break;
                        }
                        tc = p;
                        parent_of_top_candidate = self.doc.parent(tc);
                    }
                    if self.score(tc).is_none() {
                        self.initialize_node(tc);
                    }
                    tc
                }
                _ => {
                    // Move all of the page's children into topCandidate
                    let tc = self.doc.create_element("DIV");
                    needed_to_create_top_candidate = true;
                    // Move everything (not just elements, also text nodes etc.) into the container
                    // so we even include text directly in the body:
                    while let Some(c) = self.doc.first_child(page) {
                        self.doc.append_child(tc, c);
                    }
                    self.doc.append_child(page, tc);
                    self.initialize_node(tc);
                    tc
                }
            };
            let top_candidate = tc;

            // Now that we have the top candidate, look through its siblings for content
            // that might also be related. Things like preambles, content split by ads
            // that we removed, etc.
            let mut article_content = self.doc.create_element("DIV");

            let top_score = self.score(top_candidate).unwrap();
            let sibling_score_threshold = f64::max(10.0, top_score * 0.2);
            // Keep potential top candidate's parent node to try to get text direction of it later.
            let parent_of_top_candidate = self.doc.parent(top_candidate);
            let mut siblings = parent_of_top_candidate.map(|p| self.doc.children(p)).unwrap_or_default();
            let top_class = self.class_name(top_candidate).to_owned();

            let mut s = 0;
            while s < siblings.len() {
                let mut sibling = siblings[s];
                let mut append = false;

                if sibling == top_candidate {
                    append = true;
                } else {
                    let mut content_bonus = 0.0;

                    // Give a bonus if sibling nodes and top candidates have the example same classname
                    if self.doc.namespace(sibling) != Some(&ns!(svg))
                        && self.class_name(sibling) == top_class
                        && !top_class.is_empty()
                    {
                        content_bonus += top_score * 0.2;
                    }

                    if self.score(sibling).is_some_and(|sc| sc + content_bonus >= sibling_score_threshold) {
                        append = true;
                    } else if self.is(sibling, &local_name!("p")) {
                        let link_density = self.get_link_density(sibling);
                        let node_content = self.get_inner_text(sibling, true);
                        let node_length = js::len(&node_content);

                        #[allow(clippy::if_same_then_else)] // as in Readability.js
                        if node_length > 80 && link_density < 0.25 {
                            append = true;
                        } else if node_length < 80
                            && node_length > 0
                            && link_density == 0.0
                            && (node_content.contains(". ") || node_content.ends_with('.'))
                        {
                            append = true;
                        }
                    }
                }

                if append {
                    if !self.doc.html_name(sibling).is_some_and(|n| ALTER_TO_DIV_EXCEPTIONS.contains(n)) {
                        // We have a node that isn't a common block level element, like a form or td tag.
                        // Turn it into a div so it doesn't get filtered out later by accident.
                        sibling = self.set_node_tag(sibling, "DIV");
                    }

                    self.doc.append_child(article_content, sibling);
                    // The sibling has been removed from `siblings` (the parent's
                    // children); revisit this index since the nodes have shifted.
                    siblings.remove(s);
                    continue;
                }
                s += 1;
            }

            // So we have all of the content that we need. Now we clean it up for presentation.
            self.prep_article(article_content);

            if needed_to_create_top_candidate {
                // We already created a fake div thing, and there wouldn't have been any siblings left
                // for the previous loop, so there's no point trying to create a new div, and then
                // move all the children over. Just assign IDs and class names here. No need to append
                // because that already happened anyway.
                self.doc.set_attr(top_candidate, "id", "readability-page-1");
                self.doc.set_attr(top_candidate, "class", "page");
            } else {
                let div = self.doc.create_element("DIV");
                self.doc.set_attr(div, "id", "readability-page-1");
                self.doc.set_attr(div, "class", "page");
                while let Some(c) = self.doc.first_child(article_content) {
                    self.doc.append_child(div, c);
                }
                self.doc.append_child(article_content, div);
            }

            let mut parse_successful = true;

            // Now that we've gone through the full algorithm, check to see if
            // we got any meaningful content. If we didn't, we may need to re-run
            // grabArticle with different flags set. This gives us a higher likelihood of
            // finding the content, and the sieve approach gives us a higher likelihood of
            // finding the -right- content.
            let text_length = self.inner_text_len(article_content);
            if text_length < self.char_threshold {
                parse_successful = false;
                self.doc.clear_children(page);
                for c in self.doc.child_nodes(page_cache) {
                    let copy = self.doc.deep_clone(c);
                    self.doc.append_child(page, copy);
                }

                self.attempts.push((article_content, text_length));

                if self.flag_is_active(FLAG_STRIP_UNLIKELYS) {
                    self.remove_flag(FLAG_STRIP_UNLIKELYS);
                } else if self.flag_is_active(FLAG_WEIGHT_CLASSES) {
                    self.remove_flag(FLAG_WEIGHT_CLASSES);
                } else if self.flag_is_active(FLAG_CLEAN_CONDITIONALLY) {
                    self.remove_flag(FLAG_CLEAN_CONDITIONALLY);
                } else {
                    // No luck after removing flags, just return the longest text we found during the different loops
                    let mut best = self.attempts[0];
                    for &a in &self.attempts[1..] {
                        if a.1 > best.1 {
                            best = a;
                        }
                    }

                    // But first check if we actually have something
                    if best.1 == 0 {
                        return None;
                    }

                    article_content = best.0;
                    parse_successful = true;
                }
            }

            if parse_successful {
                // Find out text direction from ancestors of final top candidate.
                let mut ancestors = Vec::new();
                ancestors.extend(parent_of_top_candidate);
                ancestors.push(top_candidate);
                if let Some(p) = parent_of_top_candidate {
                    ancestors.extend(self.get_node_ancestors(p, 0));
                }
                for ancestor in ancestors {
                    if !self.doc.is_element(ancestor) {
                        continue;
                    }
                    if let Some(dir) = self.doc.attr(ancestor, "dir").filter(|d| !d.is_empty()) {
                        self.article_dir = Some(dir.to_owned());
                        break;
                    }
                }
                return Some(article_content);
            }
        }
    }

    fn post_process_content(&mut self, article_content: NodeId) {
        // Readability cannot open relative uris so we convert them to absolute uris.
        self.fix_relative_uris(article_content);

        self.simplify_nested_elements(article_content);

        if !self.keep_classes {
            // Remove classes.
            self.clean_classes(article_content);
        }
    }

    /// Runs readability.
    pub(crate) fn parse(&mut self) -> Result<Option<Article>, Error> {
        // Avoid parsing too large documents, as per configuration option
        if self.max_elems_to_parse > 0 {
            let num_tags = self.doc.descendants(self.doc.document()).filter(|&n| self.doc.is_element(n)).count();
            if num_tags > self.max_elems_to_parse {
                return Err(Error::TooManyElements(num_tags));
            }
        }

        // Unwrap image from noscript
        self.unwrap_noscript_images();

        // Extract JSON-LD metadata before removing scripts
        let json_ld = if self.disable_json_ld { JsonLd::default() } else { self.get_json_ld() };

        // Remove script tags from the document.
        self.remove_scripts();

        self.prep_document();

        let mut metadata = self.get_article_metadata(json_ld);
        self.metadata_byline = non_empty(metadata.byline.clone());
        self.article_title = metadata.title.clone().unwrap_or_default();

        let Some(article_content) = self.grab_article() else { return Ok(None) };

        self.post_process_content(article_content);

        // If we haven't found an excerpt in the article's metadata, use the article's
        // first paragraph as the excerpt. This is used for displaying a preview of
        // the article's content.
        if metadata.excerpt.as_deref().is_none_or(str::is_empty)
            && let Some(p) = self.doc.first_element_by_tag(article_content, &local_name!("p"))
        {
            metadata.excerpt = Some(js::trim(&self.doc.text_content(p)).to_owned());
        }

        let text_content = self.doc.text_content(article_content);
        Ok(Some(Article {
            title: self.article_title.clone(),
            byline: non_empty(metadata.byline).or_else(|| self.article_byline.clone()),
            dir: self.article_dir.clone(),
            lang: self.article_lang.clone(),
            content: self.doc.inner_html(article_content),
            length: js::len(&text_content),
            text_content,
            excerpt: metadata.excerpt,
            site_name: non_empty(metadata.site_name),
            published_time: non_empty(metadata.published_time),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_regex() {
        assert!(is_share_element("share"));
        assert!(is_share_element("post-share"));
        assert!(is_share_element("x_share_y"));
        assert!(is_share_element("sharedaddy x"));
        assert!(!is_share_element("shared"));
        assert!(!is_share_element("timeshare"));
    }

    #[test]
    fn similarity() {
        assert_eq!(text_similarity("a b c", "a b c"), 1.0);
        assert_eq!(text_similarity("a b c", "d e f"), 0.0);
    }

    #[test]
    fn entities() {
        assert_eq!(unescape_html_entities(Some("&amp;lt; &#x41;&#66; &#0;".into())).unwrap(), "&lt; AB \u{FFFD}");
    }

    #[test]
    fn style() {
        assert_eq!(style_property("color: red; display : NONE !important", "display").as_deref(), Some("none"));
        assert_eq!(style_property("display:block;display:none", "display").as_deref(), Some("none"));
        // As jsdom (CSSOM) reads them.
        for (style, display) in [
            ("display: /* hidden */ none", Some("none")),
            ("display:none/*x*/", Some("none")),
            ("/*a;b*/display:none", Some("none")),
            ("display:none;display:block", Some("block")),
        ] {
            assert_eq!(style_property(style, "display").as_deref(), display, "{style}");
        }
        assert_ne!(style_property("display: no/**/ne", "display").as_deref(), Some("none"));
        assert_ne!(style_property("display: \"none\"", "display").as_deref(), Some("none"));
        assert_eq!(style_property("visibility: /*c*/hidden", "visibility").as_deref(), Some("hidden"));
    }
}
