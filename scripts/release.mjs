import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { appendFileSync, copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const versionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/;

export function releaseVersion(root, tag) {
  const version = tag?.startsWith('v') ? tag.slice(1) : '';
  if (!versionPattern.test(version)) throw new Error(`Invalid release tag: ${tag}`);
  const read = (path) => readFileSync(resolve(root, path), 'utf8');
  const cargo = read('src-tauri/Cargo.toml').match(/^\[package\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const lockedPackage = read('src-tauri/Cargo.lock').split('[[package]]')
    .find((section) => /^name = "goldline"$/m.test(section));
  const versions = {
    'package.json': JSON.parse(read('package.json')).version,
    'src-tauri/tauri.conf.json': JSON.parse(read('src-tauri/tauri.conf.json')).version,
    'src-tauri/Cargo.toml': cargo?.match(/^version = "([^"]+)"$/m)?.[1],
    'src-tauri/Cargo.lock': lockedPackage?.match(/^version = "([^"]+)"$/m)?.[1],
  };
  for (const [file, actual] of Object.entries(versions)) {
    if (actual !== version) throw new Error(`${file}: expected ${version}, got ${actual}`);
  }
  return { version, prerelease: version.includes('-') };
}

export function assetPaths(version) {
  return {
    'windows-x64': `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Goldline_${version}_x64-setup.exe`,
    'macos-universal': `src-tauri/target/universal-apple-darwin/release/bundle/dmg/Goldline_${version}_universal.dmg`,
  };
}

const checksum = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');

export function collectAsset(root, version, platform) {
  const source = assetPaths(version)[platform];
  if (!source) throw new Error(`Unknown platform: ${platform}`);
  const name = source.split('/').at(-1);
  const destination = resolve(root, 'release-assets');
  const path = resolve(root, source);
  if (readFileSync(path).length === 0) throw new Error(`Empty installer: ${source}`);
  mkdirSync(destination, { recursive: true });
  copyFileSync(path, resolve(destination, name));
  writeFileSync(resolve(destination, `${name}.sha256`), `${checksum(path)}  ${name}\n`);
}

export function verifyAssets(root, version) {
  const destination = resolve(root, 'release-assets');
  const names = Object.values(assetPaths(version)).map((path) => path.split('/').at(-1));
  const expected = names.flatMap((name) => [name, `${name}.sha256`]).sort();
  const actual = readdirSync(destination).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`Expected release assets: ${expected.join(', ')}; found: ${actual.join(', ')}`);
  }
  for (const name of names) {
    const path = resolve(destination, name);
    if (readFileSync(path).length === 0) throw new Error(`Empty installer: ${name}`);
    const recorded = readFileSync(resolve(destination, `${name}.sha256`), 'utf8').trim();
    if (recorded !== `${checksum(path)}  ${name}`) throw new Error(`Checksum mismatch: ${name}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = process.cwd();
  const { version, prerelease } = releaseVersion(root, process.env.TAG_NAME);
  switch (process.argv[2]) {
    case 'check': {
      const branches = execFileSync('git', ['branch', '-r', '--contains', 'HEAD', '--format=%(refname)'], { encoding: 'utf8' });
      if (!branches.split('\n').some((branch) => branch.startsWith('refs/remotes/origin/') && !branch.endsWith('/HEAD'))) {
        throw new Error('Release commit must belong to a remote origin branch.');
      }
      if (process.env.GITHUB_OUTPUT) {
        appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\nprerelease=${prerelease}\n`);
      }
      console.log(`Verified ${process.env.TAG_NAME} (prerelease=${prerelease})`);
      break;
    }
    case 'collect':
      collectAsset(root, version, process.argv[3]);
      break;
    case 'verify':
      verifyAssets(root, version);
      console.log('Verified both installers and their SHA-256 checksums.');
      break;
    default:
      throw new Error('Usage: TAG_NAME=vX.Y.Z node scripts/release.mjs check|collect <platform>|verify');
  }
}
