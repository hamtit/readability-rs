'use strict';
// Loads the prebuilt Node-API addon for this platform (Node.js and Bun).
const { join } = require('node:path');
const { existsSync } = require('node:fs');

function platformTriple() {
  const arch = process.arch;
  if (process.platform !== 'linux' || (arch !== 'x64' && arch !== 'arm64')) {
    throw new Error(`readability-rs: unsupported platform ${process.platform}-${arch}`);
  }
  const report = typeof process.report?.getReport === 'function' ? process.report.getReport() : null;
  const glibc = report?.header?.glibcVersionRuntime;
  // Bun does not implement process.report; treat a missing report as glibc
  // unless the musl loader is present.
  const musl = glibc ? false : existsSync(`/lib/ld-musl-${arch === 'x64' ? 'x86_64' : 'aarch64'}.so.1`);
  return `linux-${arch}-${musl ? 'musl' : 'gnu'}`;
}

const triple = platformTriple();
const file = join(__dirname, `readability-rs.${triple}.node`);
if (!existsSync(file)) {
  throw new Error(`readability-rs: no prebuilt binary for ${triple} (${file})`);
}
const native = require(file);

// A JavaScript RegExp cannot cross into Rust; pass its source instead. Simple
// patterns such as the default video regex mean the same in both syntaxes.
function nativeOptions(options) {
  if (!options || !(options.allowedVideoRegex instanceof RegExp)) return options;
  const re = options.allowedVideoRegex;
  const flags = ['i', 'm', 's'].filter((f) => re.flags.includes(f)).join('');
  return { ...options, allowedVideoRegex: flags ? `(?${flags})${re.source}` : re.source };
}

// Missing values are null, as in Readability.js.
const NULLABLE = ['byline', 'dir', 'lang', 'excerpt', 'siteName', 'publishedTime'];
function withNulls(article) {
  if (article) for (const key of NULLABLE) article[key] ??= null;
  return article;
}

module.exports = {
  parse: (html, options) => withNulls(native.parse(html, nativeOptions(options))),
  parseAsync: async (html, options, signal) => withNulls(await native.parseAsync(html, nativeOptions(options), signal)),
  isProbablyReaderable: native.isProbablyReaderable,
  versions: native.versions,
};
