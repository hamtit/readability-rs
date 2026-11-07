//! C ABI exports for the WebAssembly build, used by `packages/wasm/index.js`.
//!
//! Strings cross the boundary as UTF-8 in buffers allocated with
//! `rs_alloc`. Options and results are JSON. A result is returned as a
//! pointer to `[ptr, len]` (two u32) describing a UTF-8 buffer, freed with
//! `rs_free_result`.

use std::mem::MaybeUninit;

use serde_json::{Value, json};

use crate::{Article, Options, ReaderableOptions};

/// Allocates `len` bytes (at least 1) for the caller to fill.
#[unsafe(no_mangle)]
pub extern "C" fn rs_alloc(len: usize) -> *mut u8 {
    // A boxed slice has exactly `len` bytes, which is what `rs_free` relies on to give them back.
    Box::into_raw(Box::<[u8]>::new_uninit_slice(len.max(1))).cast()
}

/// Frees a buffer from `rs_alloc` (`len` as passed there).
///
/// # Safety
/// `ptr` must come from `rs_alloc(len)` and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_free(ptr: *mut u8, len: usize) {
    // SAFETY: the caller passes a pointer from `rs_alloc(len)`, a leaked `Box<[MaybeUninit<u8>]>` of
    // exactly `len.max(1)` bytes, and frees it only once.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr.cast::<MaybeUninit<u8>>(), len.max(1))) });
}

/// Frees a result returned by `rs_parse` / `rs_versions`.
///
/// # Safety
/// `result` must come from one of those functions and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_free_result(result: *mut u32) {
    // SAFETY: the caller passes a pointer returned by `into_result`, i.e. a leaked `Box<[u32; 2]>`
    // holding the address and length of a leaked `Box<[u8]>`, and frees it only once.
    unsafe {
        let r = Box::from_raw(result.cast::<[u32; 2]>());
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(r[0] as *mut u8, r[1] as usize)));
    }
}

fn into_result(s: String) -> *mut u32 {
    // A boxed slice has exactly `len` bytes, which `rs_free_result` relies on to give them back.
    let bytes = s.into_bytes().into_boxed_slice();
    let len = bytes.len();
    let ptr = Box::into_raw(bytes).cast::<u8>();
    Box::into_raw(Box::new([ptr as u32, len as u32])).cast()
}

/// # Safety
/// `ptr..ptr+len` must be valid UTF-8 written by the caller.
unsafe fn str_arg<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    // SAFETY: the caller guarantees `ptr..ptr+len` is readable and valid UTF-8 (see `# Safety`).
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
}

fn options(o: &Value) -> Result<(Option<String>, Options), String> {
    let num = |k: &str| o.get(k).and_then(Value::as_f64);
    let flag = |k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
    let allowed_video_regex = match o.get("allowedVideoRegex").and_then(Value::as_str) {
        Some(src) => Some(regex::Regex::new(src).map_err(|e| format!("Invalid allowedVideoRegex: {e}"))?),
        None => None,
    };
    Ok((
        o.get("url").and_then(Value::as_str).map(str::to_owned),
        Options {
            max_elems_to_parse: num("maxElemsToParse").unwrap_or(0.0).max(0.0) as usize,
            nb_top_candidates: num("nbTopCandidates").unwrap_or(0.0).max(0.0) as usize,
            char_threshold: num("charThreshold").unwrap_or(0.0).max(0.0) as usize,
            classes_to_preserve: o
                .get("classesToPreserve")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            keep_classes: flag("keepClasses"),
            disable_json_ld: flag("disableJSONLD"),
            allowed_video_regex,
            link_density_modifier: num("linkDensityModifier").unwrap_or(0.0),
        },
    ))
}

fn article_json(a: Article) -> Value {
    json!({
        "title": a.title,
        "byline": a.byline,
        "dir": a.dir,
        "lang": a.lang,
        "content": a.content,
        "textContent": a.text_content,
        "length": a.length,
        "excerpt": a.excerpt,
        "siteName": a.site_name,
        "publishedTime": a.published_time,
    })
}

/// `parse(html, options)`. Returns `{"article": {...} | null}` or `{"error": "..."}`.
///
/// # Safety
/// Both arguments must be valid UTF-8 buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_parse(html: *const u8, html_len: usize, opts: *const u8, opts_len: usize) -> *mut u32 {
    // SAFETY: the caller guarantees both buffers are valid UTF-8 (see `# Safety`).
    let html = unsafe { str_arg(html, html_len) };
    // SAFETY: as above.
    let opts = unsafe { str_arg(opts, opts_len) };
    let out = (|| -> Result<Value, String> {
        let o: Value =
            if opts.is_empty() { Value::Null } else { serde_json::from_str(opts).map_err(|e| e.to_string())? };
        let (url, options) = options(&o)?;
        let article = crate::parse(html, url.as_deref(), &options).map_err(|e| e.to_string())?;
        Ok(json!({ "article": article.map(article_json) }))
    })();
    let v = out.unwrap_or_else(|e| json!({ "error": e }));
    into_result(v.to_string())
}

/// `isProbablyReaderable(html, options)`; negative values mean the defaults.
///
/// # Safety
/// `html` must be a valid UTF-8 buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_is_probably_readerable(
    html: *const u8,
    html_len: usize,
    min_content_length: f64,
    min_score: f64,
) -> u32 {
    // SAFETY: the caller guarantees `html` is a valid UTF-8 buffer (see `# Safety`).
    let html = unsafe { str_arg(html, html_len) };
    let mut o = ReaderableOptions::default();
    if min_content_length >= 0.0 {
        o.min_content_length = min_content_length as usize;
    }
    if min_score >= 0.0 {
        o.min_score = min_score;
    }
    u32::from(crate::is_probably_readerable(html, &o))
}

/// `{"readabilityRs": ..., "readabilityJs": ...}`.
#[unsafe(no_mangle)]
pub extern "C" fn rs_versions() -> *mut u32 {
    into_result(
        json!({ "readabilityRs": env!("CARGO_PKG_VERSION"), "readabilityJs": crate::READABILITY_JS_VERSION })
            .to_string(),
    )
}
