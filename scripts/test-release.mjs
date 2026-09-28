import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { test } from 'node:test';
import { assetPaths, collectAsset, releaseVersion, verifyAssets } from './release.mjs';

function fixture(t, version = '1.2.3') {
  const root = mkdtempSync(resolve(tmpdir(), 'goldline-release-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(resolve(root, 'src-tauri'));
  writeFileSync(resolve(root, 'package.json'), JSON.stringify({ version }));
  writeFileSync(resolve(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ version }));
  writeFileSync(resolve(root, 'src-tauri/Cargo.toml'), `[package]\nname = "goldline"\nversion = "${version}"\n\n[dependencies]\nserde = "1"\n`);
  writeFileSync(resolve(root, 'src-tauri/Cargo.lock'), `version = 4\n[[package]]\nname = "goldline"\nversion = "${version}"\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n`);
  return root;
}

test('accepts matching stable and prerelease versions', (t) => {
  for (const version of ['1.2.3', '1.2.3-rc.1']) {
    assert.deepEqual(releaseVersion(fixture(t, version), `v${version}`), {
      version, prerelease: version.includes('-'),
    });
  }
});

test('rejects malformed tags before publishing', (t) => {
  const root = fixture(t);
  for (const tag of ['1.2.3', 'v1', 'v01.2.3', 'v1.2.3-01', 'v1.2.3+build', 'v1.2.3;echo bad', undefined]) {
    assert.throws(() => releaseVersion(root, tag), /Invalid release tag/);
  }
});

test('rejects a mismatch in each version source', (t) => {
  for (const file of ['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
    const root = fixture(t);
    const path = resolve(root, file);
    writeFileSync(path, readFileSync(path, 'utf8').replace('1.2.3', '1.2.4'));
    assert.throws(() => releaseVersion(root, 'v1.2.3'), /expected 1.2.3, got 1.2.4/);
  }
});

test('requires both installers and detects corruption; collection is retryable', (t) => {
  const root = fixture(t);
  for (const [platform, source] of Object.entries(assetPaths('1.2.3'))) {
    assert.throws(() => collectAsset(root, '1.2.3', platform), /ENOENT/);
    const path = resolve(root, source);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, '');
    assert.throws(() => collectAsset(root, '1.2.3', platform), /Empty installer/);
    writeFileSync(path, `installer for ${platform}`);
    collectAsset(root, '1.2.3', platform);
    if (platform === 'windows-x64') assert.throws(() => verifyAssets(root, '1.2.3'), /Expected release assets/);
  }
  verifyAssets(root, '1.2.3');
  writeFileSync(resolve(root, 'release-assets/Goldline_1.2.3_x64-setup.exe'), 'corrupted');
  assert.throws(() => verifyAssets(root, '1.2.3'), /Checksum mismatch/);
  collectAsset(root, '1.2.3', 'windows-x64');
  verifyAssets(root, '1.2.3');
  writeFileSync(resolve(root, 'release-assets/unexpected.exe'), 'stale build');
  assert.throws(() => verifyAssets(root, '1.2.3'), /Expected release assets/);
});
