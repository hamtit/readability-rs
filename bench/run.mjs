// Times one scenario over a set of pages in this process and prints JSON.
//
//   bun run.mjs <scenario> <set> [rounds]     (also works with node)
//
// scenario: jsdom | linkedom | rs | rs-async
// set:      test-pages | local (HTML files in PAGES_DIR, see pages.ts)
//
// Each round parses every page once (HTML string -> article, including
// building the DOM). The first round is a warm-up and is not counted.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const [scenario, set, roundsArg] = process.argv.slice(2);
const rounds = Number(roundsArg ?? 5);
const concurrency = Number(process.env.CONCURRENCY ?? 4);

function loadPages() {
  const pages = [];
  if (set === 'test-pages') {
    const tp = join(here, '..', 'tests', 'test-pages');
    for (const name of readdirSync(tp).sort()) {
      const file = join(tp, name, 'source.html');
      if (existsSync(file)) pages.push({ url: 'http://fakehost/test/page.html', html: readFileSync(file, 'utf8') });
    }
  } else {
    const dir = process.env.PAGES_DIR;
    if (!dir) throw new Error('set PAGES_DIR to a directory of HTML files');
    const index = join(dir, 'index.json');
    const urls = new Map(existsSync(index) ? JSON.parse(readFileSync(index, 'utf8')).map((e) => [e.file, e.url]) : []);
    for (const file of readdirSync(dir).filter((f) => f.endsWith('.html')).sort()) {
      pages.push({ url: urls.get(file) ?? 'http://localhost/page.html', html: readFileSync(join(dir, file), 'utf8') });
    }
  }
  return pages;
}

function makeParser() {
  if (scenario === 'jsdom') {
    const { JSDOM, VirtualConsole } = require('jsdom');
    const { Readability } = require('@mozilla/readability');
    const virtualConsole = new VirtualConsole();
    return (p) => new Readability(new JSDOM(p.html, { url: p.url, virtualConsole }).window.document).parse();
  }
  if (scenario === 'linkedom') {
    const { DOMParser } = require('linkedom');
    const { Readability } = require('@mozilla/readability');
    const parser = new DOMParser();
    return (p) => {
      const doc = parser.parseFromString(p.html, 'text/html');
      // linkedom documents have no URL; Readability reads these two.
      Object.defineProperty(doc, 'documentURI', { value: p.url });
      Object.defineProperty(doc, 'baseURI', { value: p.url });
      return new Readability(doc).parse();
    };
  }
  const rs = require('../index.js');
  if (scenario === 'rs') return (p) => rs.parse(p.html, { url: p.url });
  if (scenario === 'rs-async') return (p) => rs.parseAsync(p.html, { url: p.url });
  throw new Error(`unknown scenario ${scenario}`);
}

function peakRssMb() {
  try {
    const status = readFileSync('/proc/self/status', 'utf8');
    return Number(status.match(/VmHWM:\s+(\d+)/)[1]) / 1024;
  } catch {
    return NaN;
  }
}

const pages = loadPages();
const parse = makeParser();
const bytes = pages.reduce((n, p) => n + Buffer.byteLength(p.html), 0);

async function round(perPage) {
  const start = performance.now();
  if (scenario === 'rs-async') {
    let next = 0;
    const worker = async () => {
      while (next < pages.length) {
        const p = pages[next++];
        const t = performance.now();
        await parse(p);
        perPage?.push(performance.now() - t);
      }
    };
    await Promise.all(Array.from({ length: concurrency }, worker));
  } else {
    for (const p of pages) {
      const t = performance.now();
      parse(p);
      perPage?.push(performance.now() - t);
    }
  }
  return performance.now() - start;
}

const cpu0 = process.cpuUsage();
await round(null); // warm-up
const totals = [];
const perPage = [];
for (let i = 0; i < rounds; i++) totals.push(await round(i === 0 ? perPage : null));
const cpu = process.cpuUsage(cpu0);
totals.sort((a, b) => a - b);
perPage.sort((a, b) => a - b);
const q = (arr, f) => arr[Math.min(arr.length - 1, Math.floor(arr.length * f))];
console.log(
  JSON.stringify({
    scenario,
    set,
    runtime: process.versions.bun ? `bun ${process.versions.bun}` : `node ${process.versions.node}`,
    pages: pages.length,
    mb: bytes / 1e6,
    totalMs: q(totals, 0.5),
    medianPageMs: q(perPage, 0.5),
    p95PageMs: q(perPage, 0.95),
    maxPageMs: perPage[perPage.length - 1],
    cpuMsPerRound: (cpu.user + cpu.system) / 1000 / (rounds + 1),
    peakRssMb: peakRssMb(),
  }),
);
