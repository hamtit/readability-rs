// Inputs of compare.ts: the upstream test pages (../tests/test-pages,
// scripts/fetch-test-pages.sh) and, optionally, local pages (see localPages).
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

export interface Page {
  set: 'test-pages' | 'local';
  name: string;
  url: string;
  html: string;
}

/**
 * HTML files in the directory named by PAGES_DIR (none if unset). An
 * optional index.json there, `[{ "file": "a.html", "url": "https://..." }]`,
 * gives each file its document URL; other files get a placeholder URL.
 * Nothing is downloaded: the pages are whatever was saved there.
 */
export function localPages(): Page[] {
  const dir = process.env.PAGES_DIR;
  if (!dir) return [];
  const urls = new Map<string, string>();
  const index = join(dir, 'index.json');
  if (existsSync(index)) {
    for (const { file, url } of JSON.parse(readFileSync(index, 'utf8'))) urls.set(file, url);
  }
  return readdirSync(dir)
    .filter((f) => f.endsWith('.html'))
    .sort()
    .map((file) => ({
      set: 'local' as const,
      name: file,
      url: urls.get(file) ?? 'http://localhost/page.html',
      html: readFileSync(join(dir, file), 'utf8'),
    }));
}

export function loadPages(): Page[] {
  const pages: Page[] = [];
  const tp = join(import.meta.dir, '..', 'tests', 'test-pages');
  if (existsSync(tp)) {
    for (const name of readdirSync(tp).sort()) {
      const file = join(tp, name, 'source.html');
      if (!existsSync(file)) continue;
      pages.push({ set: 'test-pages', name, url: 'http://fakehost/test/page.html', html: readFileSync(file, 'utf8') });
    }
  }
  return pages.concat(localPages());
}
