import { describe, expect, test } from 'bun:test';
import { isProbablyReaderable, parse, parseAsync, versions } from '../index.js';

const paragraph =
  'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ';
const article = `<!DOCTYPE html>
<html lang="ja"><head><title>記事のタイトル | サイト名</title>
<meta property="og:site_name" content="サイト名">
<meta name="author" content="山田 太郎"></head>
<body><nav class="menu"><a href="/">トップ</a></nav>
<article><h1>記事のタイトル</h1>
<p>${paragraph.repeat(4)}</p>
<p><a href="/next">次の記事</a> と <img src="images/a.jpg"> ${paragraph.repeat(3)}</p>
<div class="share">share</div>
</article>
<footer>footer</footer></body></html>`;

describe('parse', () => {
  test('extracts the article and metadata', () => {
    const a = parse(article, { url: 'https://example.com/posts/1' })!;
    expect(a.title).toBe('記事のタイトル | サイト名');
    expect(a.byline).toBe('山田 太郎');
    expect(a.siteName).toBe('サイト名');
    expect(a.lang).toBe('ja');
    expect(a.dir).toBeNull();
    expect(a.publishedTime).toBeNull();
    expect(a.content.startsWith('<div id="readability-page-1" class="page">')).toBe(true);
    // Relative URLs are resolved against the document URL.
    expect(a.content).toContain('href="https://example.com/next"');
    expect(a.content).toContain('src="https://example.com/posts/images/a.jpg"');
    expect(a.content).not.toContain('トップ');
    expect(a.content).not.toContain('footer');
    expect(a.length).toBe(a.textContent.length);
    // The first paragraph, without metadata.
    expect(a.excerpt).toBe(paragraph.repeat(4).trim());
    // Japanese headings have no ASCII words, so it does not count as a duplicate of the title.
    expect(a.content).toContain('<h2>記事のタイトル</h2>');
  });

  test('returns null without content', () => {
    expect(parse('')).toBeNull();
  });

  test('keeps relative URLs without a document URL', () => {
    const a = parse(article)!;
    expect(a.content).toContain('href="/next"');
  });

  test('options', () => {
    const html = `<p>${paragraph}</p><iframe src="https://mycustomdomain.com/some-embeds"></iframe>`;
    const kept = parse(html, { charThreshold: 20, allowedVideoRegex: /.*mycustomdomain.com.*/ })!;
    expect(kept.content).toBe(
      `<div id="readability-page-1" class="page"><p>${paragraph}</p><iframe src="https://mycustomdomain.com/some-embeds"></iframe></div>`,
    );
    const removed = parse(html, { charThreshold: 20 })!;
    expect(removed.content).not.toContain('iframe');

    const classes = parse(`<div class="a b"><p class="x caption">${paragraph.repeat(5)}</p></div>`, {
      classesToPreserve: ['caption'],
    })!;
    expect(classes.content).toContain('<p class="caption">');
    const keep = parse(`<div class="a b"><p class="x caption">${paragraph.repeat(5)}</p></div>`, { keepClasses: true })!;
    expect(keep.content).toContain('<p class="x caption">');
  });

  test('maxElemsToParse', () => {
    expect(() => parse('<p>x</p>', { maxElemsToParse: 1 })).toThrow('Aborting parsing document; 4 elements found');
  });

  test('invalid options', () => {
    expect(() => parse('<p>x</p>', { url: 'not a url' })).toThrow('Invalid URL');
    expect(() => parse('<p>x</p>', { allowedVideoRegex: '(' })).toThrow('allowedVideoRegex');
  });
});

describe('parseAsync', () => {
  test('matches parse', async () => {
    const options = { url: 'https://example.com/posts/1' };
    const results = await Promise.all(Array.from({ length: 8 }, () => parseAsync(article, options)));
    for (const r of results) expect(r).toEqual(parse(article, options));
  });

  test('rejects on errors', async () => {
    expect(parseAsync('<p>x</p>', { maxElemsToParse: 1 })).rejects.toThrow('Aborting parsing document');
  });
});

test('isProbablyReaderable', () => {
  expect(isProbablyReaderable(article)).toBe(true);
  expect(isProbablyReaderable('<p>short</p>')).toBe(false);
  const html = `<html><p id="main">${'hello there '.repeat(11)}</p></html>`;
  expect(isProbablyReaderable(html)).toBe(false);
  expect(isProbablyReaderable(html, { minContentLength: 120, minScore: 0 })).toBe(true);
});

test('versions', () => {
  const v = versions();
  expect(v.readabilityJs).toBe('0.6.0');
  expect(v.readabilityRs).toMatch(/^\d+\.\d+\.\d+/);
});
