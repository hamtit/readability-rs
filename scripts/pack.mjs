#!/usr/bin/env node
// Assemble an npm package from build outputs and `npm pack` it.
//
//   node scripts/pack.mjs native      <out-dir> <addon-x64.node> <addon-arm64.node>
//   node scripts/pack.mjs native-arch <out-dir> <x64|arm64> <addon.node>
//   node scripts/pack.mjs wasm        <out-dir> <wasm-build-dir>
//
// <addon.node> is readability-rs.linux-<arch>-gnu.node. licenses/
// (scripts/collect-licenses.sh) must exist for the native kinds;
// <wasm-build-dir> is the output of scripts/build-wasm.sh. The version is
// the one in Cargo.toml.
//
// Writes <out-dir>/readability-rs[-linux-<arch>|-wasm]-<version>.tgz
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function cargoVersion() {
  const toml = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const m = toml.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms);
  if (!m) throw new Error('version not found in Cargo.toml');
  return m[1];
}

function copy(from, to) {
  fs.cpSync(from, to, { recursive: true, dereference: true });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [kind, outDir, ...inputs] = process.argv.slice(2);
  const version = cargoVersion();
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), `readability-rs-${kind}-`));
  const base = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));

  // Fields shared by the native packages; scripts and devDependencies
  // belong to the repository only.
  const nativePackage = (name, cpu) => ({
    name,
    version,
    description: base.description,
    license: base.license,
    repository: { type: 'git', url: 'git+https://github.com/hamtit/readability-rs.git' },
    main: base.main,
    types: base.types,
    files: base.files,
    os: base.os,
    cpu,
    private: true,
  });

  let pkg;
  if (kind === 'wasm') {
    const dir = path.resolve(inputs[0]);
    const template = path.join(root, 'packages', 'wasm');
    for (const f of ['index.js', 'README.md']) copy(path.join(template, f), path.join(stage, f));
    // The shared types (Article, ParseOptions, ...) come from the addon's index.d.ts.
    const shared = fs.readFileSync(path.join(root, 'index.d.ts'), 'utf8').split('\n/** Extracts the article.')[0];
    const own = fs.readFileSync(path.join(template, 'index.d.ts'), 'utf8').replace(/^import type .*\n\nexport type .*\n/m, '');
    fs.writeFileSync(path.join(stage, 'index.d.ts'), `${shared.trimEnd()}\n${own}`);
    copy(path.join(dir, 'dist'), path.join(stage, 'dist'));
    copy(path.join(dir, 'licenses'), path.join(stage, 'licenses'));
    for (const f of ['LICENSE', 'NOTICE']) copy(path.join(root, f), path.join(stage, f));
    pkg = JSON.parse(fs.readFileSync(path.join(template, 'package.json'), 'utf8'));
    pkg.version = version;
    pkg.files = ['index.js', 'index.d.ts', 'dist/', 'licenses/', 'LICENSE', 'NOTICE'];
  } else {
    let addons; // [arch, file]
    if (kind === 'native') {
      addons = [['x64', inputs[0]], ['arm64', inputs[1]]];
      pkg = nativePackage('readability-rs', base.cpu);
    } else if (kind === 'native-arch') {
      const arch = inputs[0];
      if (!['x64', 'arm64'].includes(arch)) throw new Error(`unknown arch: ${arch}`);
      addons = [[arch, inputs[1]]];
      pkg = nativePackage(`readability-rs-linux-${arch}`, [arch]);
    } else {
      throw new Error(`unknown package kind: ${kind}`);
    }
    for (const f of ['index.js', 'index.d.ts', 'LICENSE', 'NOTICE']) copy(path.join(root, f), path.join(stage, f));
    copy(path.join(root, 'licenses'), path.join(stage, 'licenses'));
    for (const [arch, file] of addons) {
      copy(path.resolve(file), path.join(stage, `readability-rs.linux-${arch}-gnu.node`));
    }
    fs.writeFileSync(
      path.join(stage, 'README.md'),
      `# ${pkg.name}\n\n` +
        (kind === 'native'
          ? 'Node-API addon for Linux x64 and arm64 (glibc).\n'
          : `Node-API addon for Linux ${inputs[0]} only (glibc); the \`os\`/\`cpu\` fields let package managers skip it elsewhere.\n`) +
        '\nSee https://github.com/hamtit/readability-rs for usage.\n',
    );
  }
  fs.writeFileSync(path.join(stage, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');

  fs.mkdirSync(outDir, { recursive: true });
  const dest = path.resolve(outDir);
  const [{ filename }] = JSON.parse(
    execFileSync('npm', ['pack', '--json', '--pack-destination', dest], { cwd: stage, encoding: 'utf8' }),
  );
  const expected = `${pkg.name}-${version}.tgz`;
  if (path.basename(filename) !== expected) {
    fs.renameSync(path.join(dest, path.basename(filename)), path.join(dest, expected));
  }
  fs.rmSync(stage, { recursive: true, force: true });
  console.log(path.join(dest, expected));
}
