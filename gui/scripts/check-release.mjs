import { readFileSync, statSync } from 'node:fs';
import assert from 'node:assert/strict';

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const pkg = JSON.parse(read('package.json'));
const lock = JSON.parse(read('package-lock.json'));
const config = JSON.parse(read('src-tauri/tauri.conf.json'));
const cargo = read('src-tauri/Cargo.toml');
assert.equal(pkg.version, config.version, 'package/Tauri version mismatch');
assert.equal(pkg.version, lock.version, 'lockfile version mismatch');
assert.equal(cargo.split('[lib]')[0].match(/^version = "([^"]+)"/m)?.[1], pkg.version, 'Cargo/package version mismatch');
if (process.env.GITHUB_REF_TYPE === 'tag') {
  assert.equal(process.env.GITHUB_REF_NAME, `yuki-v${pkg.version}`, 'tag/version mismatch');
}
for (const path of Object.keys(config.bundle.resources)) {
  statSync(new URL(`../src-tauri/${path}`,import.meta.url));
  if (!path.endsWith('/')) read(`src-tauri/${path}`);
}
assert.equal(config.bundle.externalBin, undefined, 'release policy excludes bundled runtime binaries');
console.log(`Yuki ${pkg.version}: release metadata and notices OK`);
