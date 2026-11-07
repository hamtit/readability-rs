// Runs Readability.js (on jsdom, as in its own tests) and readability-rs on
// the same pages and compares every field of the result exactly.
//
//   [PAGES_DIR=<dir>] bun compare.ts [name-filter]
import { JSDOM, VirtualConsole } from 'jsdom';
import { Readability } from '@mozilla/readability';
import * as rs from '../index.js';
import { loadPages } from './pages';

const filter = process.argv[2];
const keys = ['title', 'byline', 'dir', 'lang', 'content', 'textContent', 'length', 'excerpt', 'siteName', 'publishedTime'];
const virtualConsole = new VirtualConsole();

function firstDiff(a: string, b: string): string {
  let i = 0;
  while (i < a.length && a[i] === b[i]) i++;
  const from = Math.max(0, i - 80);
  return `at ${i}:\n      js: ${JSON.stringify(a.slice(from, i + 80))}\n      rs: ${JSON.stringify(b.slice(from, i + 80))}`;
}

let same = 0;
let total = 0;
for (const page of loadPages()) {
  if (filter && !page.name.includes(filter)) continue;
  total++;
  const doc = new JSDOM(page.html, { url: page.url, virtualConsole }).window.document;
  const js = new Readability(doc).parse();
  const ours = rs.parse(page.html, { url: page.url });
  const diffs: string[] = [];
  if (!js || !ours) {
    if (!!js !== !!ours) diffs.push(`result: js ${!!js}, rs ${!!ours}`);
  } else {
    for (const k of keys) {
      const a = (js as any)[k] ?? null;
      const b = (ours as any)[k] ?? null;
      if (a !== b) {
        diffs.push(typeof a === 'string' && typeof b === 'string' ? `${k} ${firstDiff(a, b)}` : `${k}: js ${JSON.stringify(a)}, rs ${JSON.stringify(b)}`);
      }
    }
  }
  if (diffs.length) {
    console.log(`${page.set}/${page.name} (${page.url})\n    ${diffs.join('\n    ')}`);
  } else {
    same++;
  }
}
console.log(`${same} / ${total} identical`);
if (same !== total || total === 0) process.exit(1);
