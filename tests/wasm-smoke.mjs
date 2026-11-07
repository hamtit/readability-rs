// Smoke test for the readability-rs-wasm package (installed, or a path).
//   node tests/wasm-smoke.mjs [package name or path]
import assert from 'node:assert/strict';

const pkg = process.argv[2] ?? 'readability-rs-wasm';
const { default: createReadability } = await import(pkg);
const r = await createReadability();

const paragraph = 'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore. ';
const html = `<html lang="ja"><head><title>記事のタイトル | サイト</title><meta name="author" content="山田"></head>
<body><nav class="menu">menu</nav><article><p>${paragraph.repeat(6)}</p><p><a href="/next">次へ</a> ${paragraph.repeat(3)}</p></article></body></html>`;

const a = r.parse(html, { url: 'https://example.com/posts/1' });
assert.equal(a.title, '記事のタイトル | サイト');
assert.equal(a.byline, '山田');
assert.equal(a.lang, 'ja');
assert.equal(a.dir, null);
assert.equal(a.length, a.textContent.length);
assert.ok(a.content.startsWith('<div id="readability-page-1" class="page">'));
assert.ok(a.content.includes('href="https://example.com/next"'));
assert.ok(!a.content.includes('menu'));
assert.equal(r.parse(''), null);
assert.throws(() => r.parse('<p>x</p>', { maxElemsToParse: 1 }), /Aborting parsing document; 4 elements found/);
assert.throws(() => r.parse('<p>x</p>', { allowedVideoRegex: '(' }), /allowedVideoRegex/);
const iframe = `<p>${paragraph}</p><iframe src="https://mycustomdomain.com/x"></iframe>`;
assert.ok(r.parse(iframe, { charThreshold: 20, allowedVideoRegex: /.*mycustomdomain.com.*/ }).content.includes('<iframe'));
assert.equal(r.isProbablyReaderable(html), true);
assert.equal(r.isProbablyReaderable('<p>short</p>'), false);
assert.equal(r.isProbablyReaderable(`<p>${'hello there '.repeat(11)}</p>`, { minContentLength: 120, minScore: 0 }), true);
assert.equal(r.versions().readabilityJs, '0.6.0');
console.log('wasm smoke test passed', r.versions());
