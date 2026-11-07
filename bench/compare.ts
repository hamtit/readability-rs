// Runs Readability.js (on jsdom, as in its own tests) and readability-rs on
// the same pages and compares every field of the result exactly: the
// Node-API addon, and the WebAssembly build when it has been built
// (scripts/build-wasm.sh out/wasm, or WASM=<path to readability.wasm>).
//
//   [PAGES_DIR=<dir>] bun compare.ts [name-filter]
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { JSDOM, VirtualConsole } from 'jsdom';
import { Readability } from '@mozilla/readability';
// The addon is loaded lazily: WASM_ONLY=1 compares only the wasm build
// (where the addon is not built).
const rs = process.env.WASM_ONLY ? null : await import('../index.js');
import createReadability from '../packages/wasm/index.js';
import { loadPages } from './pages';

const wasmFile = process.env.WASM ?? join(import.meta.dir, '..', 'out', 'wasm', 'dist', 'readability.wasm');
const wasm = existsSync(wasmFile) ? await createReadability({ wasmBinary: readFileSync(wasmFile) }) : null;
if (process.env.WASM_ONLY && !wasm) throw new Error(`WASM_ONLY: ${wasmFile} not found`);
console.log(wasm ? `comparing the addon and ${wasmFile}` : 'comparing the addon (no wasm build found)');

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
  const diffs: string[] = [];
  const candidates: [string, unknown][] = [];
  if (!process.env.WASM_ONLY) candidates.push(['addon', rs!.parse(page.html, { url: page.url })]);
  if (wasm) candidates.push(['wasm', wasm.parse(page.html, { url: page.url })]);
  for (const [label, ours] of candidates) {
    if (!js || !ours) {
      if (!!js !== !!ours) diffs.push(`${label} result: js ${!!js}, rs ${!!ours}`);
      continue;
    }
    for (const k of keys) {
      const a = (js as any)[k] ?? null;
      const b = (ours as any)[k] ?? null;
      if (a !== b) {
        diffs.push(
          typeof a === 'string' && typeof b === 'string'
            ? `${label} ${k} ${firstDiff(a, b)}`
            : `${label} ${k}: js ${JSON.stringify(a)}, rs ${JSON.stringify(b)}`,
        );
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
