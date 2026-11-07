//! JavaScript string semantics that Readability's heuristics depend on.
//!
//! Readability measures text with `String.prototype.length` (UTF-16 code
//! units), trims with `String.prototype.trim` and matches `\s` with the
//! ECMAScript white space set. These helpers reproduce that behaviour on
//! UTF-8 strings so thresholds such as "25 characters" mean the same thing.

use std::borrow::Cow;

/// ECMAScript `WhiteSpace` + `LineTerminator` (what `\s` and `trim()` use).
#[inline]
pub fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// The `\s` class as a regex character class body.
pub const WS_CLASS: &str =
    r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

/// `String.prototype.trim`.
#[inline]
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// `String.prototype.length` (UTF-16 code units).
#[inline]
pub fn len(s: &str) -> usize {
    // Fast path: ASCII bytes and 3-byte sequences are one unit, 4-byte
    // sequences are two. Counting non-continuation bytes gives the number of
    // code points; add one for each 4-byte lead byte.
    let mut n = 0usize;
    for &b in s.as_bytes() {
        if (b & 0xC0) != 0x80 {
            n += 1;
        }
        if b >= 0xF0 {
            n += 1;
        }
    }
    n
}

/// Is the string empty or only white space (`/^\s*$/`)?
#[inline]
pub fn is_blank(s: &str) -> bool {
    s.chars().all(is_ws)
}

/// `s.replace(/\s{2,}/g, " ")`.
pub fn normalize_spaces(s: &str) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut run_start = 0usize;
    let mut run_len = 0usize;
    let mut copied = 0usize;
    for (i, c) in s.char_indices() {
        if is_ws(c) {
            if run_len == 0 {
                run_start = i;
            }
            run_len += 1;
        } else {
            if run_len >= 2 {
                let o = out.get_or_insert_with(|| String::with_capacity(s.len()));
                o.push_str(&s[copied..run_start]);
                o.push(' ');
                copied = i;
            }
            run_len = 0;
        }
    }
    if run_len >= 2 {
        let o = out.get_or_insert_with(|| String::with_capacity(s.len()));
        o.push_str(&s[copied..run_start]);
        o.push(' ');
        copied = s.len();
    }
    match out {
        Some(mut o) => {
            o.push_str(&s[copied..]);
            Cow::Owned(o)
        }
        None => Cow::Borrowed(s),
    }
}

/// Byte length of the ECMAScript white space character starting at `b[0]`,
/// or 0 if it is not white space. `b` must start at a char boundary.
#[inline(always)]
fn ws_len(b: &[u8]) -> usize {
    match b[0] {
        b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' => 1,
        0x00..=0x7F => 0,
        0xC2 => usize::from(b.get(1) == Some(&0xA0)) * 2,
        0xE1 => usize::from(b.get(1) == Some(&0x9A) && b.get(2) == Some(&0x80)) * 3,
        0xE2 => match (b.get(1), b.get(2)) {
            (Some(0x80), Some(0x80..=0x8A | 0xA8 | 0xA9 | 0xAF)) | (Some(0x81), Some(0x9F)) => 3,
            _ => 0,
        },
        0xE3 => usize::from(b.get(1) == Some(&0x80) && b.get(2) == Some(&0x80)) * 3,
        0xEF => usize::from(b.get(1) == Some(&0xBB) && b.get(2) == Some(&0xBF)) * 3,
        _ => 0,
    }
}

#[inline(always)]
fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Streams text pieces and measures them as `normalize_spaces(trim(text))`
/// would: UTF-16 length, plus the number of `,`. Pieces are concatenated,
/// so white space runs may span them.
#[derive(Default)]
pub struct TextMeasure {
    pub len: usize,
    pub commas: usize,
    seen: bool,
    pending_ws: bool,
}

impl TextMeasure {
    #[inline]
    pub fn feed(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let w = ws_len(&b[i..]);
            if w > 0 {
                if self.seen {
                    self.pending_ws = true;
                }
                i += w;
                continue;
            }
            if self.pending_ws {
                self.len += 1;
                self.pending_ws = false;
            }
            self.seen = true;
            let lead = b[i];
            if lead == b',' {
                self.commas += 1;
            }
            let l = utf8_len(lead);
            self.len += if l == 4 { 2 } else { 1 };
            i += l;
        }
    }
}

/// What [`TextMeasure::feed`] needs to know about one string, so a string
/// that is measured repeatedly is scanned once.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TextSummary {
    /// UTF-16 length of `normalize_spaces(trim(s))`.
    pub core_len: u32,
    pub commas: u32,
    /// `s` has non-white-space characters.
    pub has_content: bool,
    /// `s` is not empty and starts (or, without content, consists) with white space.
    pub leading_ws: bool,
    pub trailing_ws: bool,
}

impl TextSummary {
    pub fn of(s: &str) -> TextSummary {
        let mut m = TextMeasure::default();
        m.feed(s);
        let b = s.as_bytes();
        let has_content = m.seen;
        TextSummary {
            core_len: m.len as u32,
            commas: m.commas as u32,
            has_content,
            leading_ws: !b.is_empty() && ws_len(b) > 0,
            trailing_ws: has_content && m.pending_ws,
        }
    }
}

impl TextMeasure {
    /// Same as `feed` with the string `summary` was made from.
    #[inline]
    pub fn feed_summary(&mut self, summary: TextSummary) {
        if !summary.has_content {
            if summary.leading_ws && self.seen {
                self.pending_ws = true;
            }
            return;
        }
        if summary.leading_ws && self.seen {
            self.pending_ws = true;
        }
        if self.pending_ws {
            self.len += 1;
        }
        self.len += summary.core_len as usize;
        self.commas += summary.commas as usize;
        self.seen = true;
        self.pending_ws = summary.trailing_ws;
    }
}

/// UTF-16 length of `normalize_spaces(trim(s))`, without allocating.
#[cfg(test)]
pub fn normalized_len(s: &str) -> usize {
    let mut m = TextMeasure::default();
    m.feed(s);
    m.len
}

/// `s.trim().length !== 0`, byte-wise.
#[inline]
pub fn has_non_ws(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let w = ws_len(&b[i..]);
        if w == 0 {
            return true;
        }
        i += w;
    }
    false
}

/// `str.split(/\s+/).length`: the number of white space runs plus one.
pub fn word_count(s: &str) -> usize {
    let mut runs = 0;
    let mut in_run = false;
    for c in s.chars() {
        if is_ws(c) {
            if !in_run {
                runs += 1;
                in_run = true;
            }
        } else {
            in_run = false;
        }
    }
    runs + 1
}

/// `parseInt(s, 10)`; `None` for `NaN`.
pub fn parse_int(s: &str) -> Option<f64> {
    let s = s.trim_start_matches(is_ws);
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let mut v = 0f64;
    for b in digits[..end].bytes() {
        v = v * 10.0 + f64::from(b - b'0');
    }
    Some(if neg { -v } else { v })
}

/// `String.prototype.toLowerCase` for the purposes of tokenizing.
pub fn to_lower(s: &str) -> String {
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(len("abc"), 3);
        assert_eq!(len("日本語"), 3);
        assert_eq!(len("😀"), 2);
        assert_eq!(len("é"), 1);
    }

    #[test]
    fn normalize() {
        assert_eq!(normalize_spaces("a  b\nc \n d"), "a b\nc d");
        assert_eq!(normalize_spaces("a b"), "a b");
        assert_eq!(normalize_spaces("a  "), "a ");
        assert_eq!(normalized_len("  a  b\nc \n d  "), len("a b\nc d"));
        assert_eq!(normalized_len("\u{3000}日本\u{a0}\u{a0}語😀\u{2028}"), len("日本 語😀"));
        assert_eq!(normalized_len("\u{2010}"), 1);
        assert!(!has_non_ws(" \u{feff}\u{205f}"));
        assert!(has_non_ws(" \u{2010}"));
        let mut m = TextMeasure::default();
        m.feed("a, ");
        m.feed(" b,");
        assert_eq!((m.len, m.commas), (5, 2));
        let pieces = ["", " ", "a", " a ", "  ", "b  c", "\u{3000}", "d,", " e"];
        for i in 0..pieces.len() {
            for j in 0..pieces.len() {
                for k in 0..pieces.len() {
                    let (a, b, c) = (pieces[i], pieces[j], pieces[k]);
                    let mut x = TextMeasure::default();
                    let mut y = TextMeasure::default();
                    for p in [a, b, c] {
                        x.feed(p);
                        y.feed_summary(TextSummary::of(p));
                    }
                    assert_eq!((x.len, x.commas), (y.len, y.commas), "{a:?} {b:?} {c:?}");
                    assert_eq!(x.len, len(&normalize_spaces(trim(&format!("{a}{b}{c}")))));
                }
            }
        }
    }

    #[test]
    fn words() {
        assert_eq!(word_count(""), 1);
        assert_eq!(word_count("a b"), 2);
        assert_eq!(word_count(" a  b "), 4);
    }

    #[test]
    fn ints() {
        assert_eq!(parse_int(" 2px"), Some(2.0));
        assert_eq!(parse_int("-3"), Some(-3.0));
        assert_eq!(parse_int("x"), None);
    }
}
