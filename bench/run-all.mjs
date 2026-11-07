// Runs every scenario of run.mjs in fresh processes and prints a table.
//
//   [PAGES_DIR=<dir>] node run-all.mjs [runs=3] [rounds=5]
//
// Each (runtime, set, scenario) runs `runs` times in a new process; the
// median run (by total time) is reported.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const runs = Number(process.argv[2] ?? 3);
const rounds = process.argv[3] ?? '5';
const sets = (process.env.SETS ?? 'test-pages,local').split(',').filter((s) =>
  s === 'local' ? !!process.env.PAGES_DIR : existsSync(join(here, '..', 'tests', 'test-pages')),
);
const runtimes = (process.env.RUNTIMES ?? 'bun,node').split(',');
const scenarios = (process.env.SCENARIOS ?? 'jsdom,linkedom,rs,rs-async').split(',');

const results = [];
for (const set of sets) {
  for (const runtime of runtimes) {
    for (const scenario of scenarios) {
      const samples = [];
      for (let i = 0; i < runs; i++) {
        // jsdom needs more than V8's default heap limit on the larger pages.
        const args = runtime === 'node' ? ['--max-old-space-size=8192'] : [];
        const out = execFileSync(runtime, [...args, join(here, 'run.mjs'), scenario, set, rounds], {
          encoding: 'utf8',
          maxBuffer: 1 << 20,
        });
        samples.push(JSON.parse(out.trim().split('\n').pop()));
      }
      samples.sort((a, b) => a.totalMs - b.totalMs);
      const r = samples[Math.floor(samples.length / 2)];
      results.push(r);
      console.error(`${set} ${r.runtime} ${scenario}: ${r.totalMs.toFixed(0)} ms, ${r.peakRssMb.toFixed(0)} MB`);
    }
  }
}
console.log(JSON.stringify(results, null, 2));
