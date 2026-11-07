// readability-rs for WebAssembly: the same parse / isProbablyReaderable /
// versions API as the Node-API addon, synchronous after initialization.
//
// It runs on the calling thread; use a Worker in browsers to keep the UI
// responsive.

const NULLABLE = ['byline', 'dir', 'lang', 'excerpt', 'siteName', 'publishedTime'];
const encoder = new TextEncoder();
const decoder = new TextDecoder();

async function loadBytes() {
  const url = new URL('./dist/readability.wasm', import.meta.url);
  if (url.protocol === 'file:') {
    const { readFile } = await import('node:fs/promises');
    return readFile(url);
  }
  const res = await fetch(url);
  if (!res.ok) throw new Error(`readability-rs: failed to load ${url}: ${res.status}`);
  return res.arrayBuffer();
}

// A JavaScript RegExp cannot cross into Rust; pass its source instead.
function regexSource(re) {
  if (!(re instanceof RegExp)) return re;
  const flags = ['i', 'm', 's'].filter((f) => re.flags.includes(f)).join('');
  return flags ? `(?${flags})${re.source}` : re.source;
}

/**
 * Instantiate the module. `options.wasmBinary` (bytes) or `options.module`
 * (a compiled WebAssembly.Module) skip loading dist/readability.wasm.
 */
export default async function createReadability(options = {}) {
  let module = options.module;
  if (!module) module = await WebAssembly.compile(options.wasmBinary ?? (await loadBytes()));
  const { exports: w } = await WebAssembly.instantiate(module, {});

  // Copy `s` into the wasm heap as UTF-8 and run `fn(ptr, len)`.
  function withString(s, fn) {
    const bytes = encoder.encode(s);
    const ptr = w.rs_alloc(bytes.length) >>> 0;
    try {
      new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
      return fn(ptr, bytes.length);
    } finally {
      w.rs_free(ptr, bytes.length);
    }
  }

  function takeResult(handle) {
    try {
      const [ptr, len] = new Uint32Array(w.memory.buffer, handle >>> 0, 2);
      return JSON.parse(decoder.decode(new Uint8Array(w.memory.buffer, ptr, len)));
    } finally {
      w.rs_free_result(handle);
    }
  }

  return {
    /** Extract the article; `null` when none is found. */
    parse(html, parseOptions) {
      if (typeof html !== 'string') throw new TypeError('html must be a string');
      const opts = parseOptions ? JSON.stringify({ ...parseOptions, allowedVideoRegex: regexSource(parseOptions.allowedVideoRegex) }) : '';
      const out = withString(html, (hp, hl) => withString(opts, (op, ol) => takeResult(w.rs_parse(hp, hl, op, ol))));
      if (out.error) throw new Error(out.error);
      const article = out.article;
      if (article) for (const key of NULLABLE) article[key] ??= null;
      return article;
    },
    /** Decides whether or not the document is reader-able without parsing the whole thing. */
    isProbablyReaderable(html, readerableOptions = {}) {
      return (
        withString(html, (p, l) =>
          w.rs_is_probably_readerable(p, l, readerableOptions.minContentLength ?? -1, readerableOptions.minScore ?? -1),
        ) !== 0
      );
    },
    versions() {
      return takeResult(w.rs_versions());
    },
  };
}
