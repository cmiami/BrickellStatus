// Keeps the vendored droidplug Java in step with the btleplug crate the
// workspace actually resolves.
//
// btleplug's Android backend is half Rust and half Java, and the halves are
// versioned together inside one crate. If Cargo.lock moves and this tree does
// not, nothing fails to compile -- the mismatch waits until a device tries to
// scan and throws NoSuchMethodError from inside JNI. `--check` runs in CI so
// that can never be the first sign of it.
//
//   node scripts/sync-droidplug-java.mjs           # copy crate -> repo
//   node scripts/sync-droidplug-java.mjs --check   # fail if they differ

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const manifestPath = join(repositoryRoot, 'apps/desktop/src-tauri/Cargo.toml');
const vendorRoot = join(repositoryRoot, 'apps/desktop/src-tauri/android/droidplug/java');

const check = process.argv.includes('--check');

// btleplug is an Android-only dependency, so the dependency graph has to be
// resolved for an Android target or the package is simply absent.
function resolveBtleplugManifest() {
  const metadata = JSON.parse(
    execFileSync(
      'cargo',
      [
        'metadata',
        '--format-version',
        '1',
        '--filter-platform',
        'aarch64-linux-android',
        '--manifest-path',
        manifestPath
      ],
      { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }
    )
  );
  const matches = metadata.packages.filter((pkg) => pkg.name === 'btleplug');
  if (matches.length !== 1) {
    throw new Error(
      `expected exactly one btleplug package in the Android dependency graph, found ${matches.length}`
    );
  }
  return matches[0];
}

function javaFiles(root) {
  const found = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory).sort()) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (entry.endsWith('.java')) found.push(path);
    }
  };
  if (existsSync(root)) walk(root);
  return found;
}

function fingerprint(root) {
  const digest = createHash('sha256');
  for (const file of javaFiles(root)) {
    digest.update(relative(root, file).split('\\').join('/'));
    digest.update(readFileSync(file));
  }
  return `${javaFiles(root).length}:${digest.digest('hex')}`;
}

const btleplug = resolveBtleplugManifest();
const source = join(dirname(btleplug.manifest_path), 'src/droidplug/java/src/main/java');
if (!existsSync(source)) {
  throw new Error(`btleplug ${btleplug.version} has no droidplug Java at ${source}`);
}

const sourcePrint = fingerprint(source);
const vendoredPrint = fingerprint(vendorRoot);

if (check) {
  if (sourcePrint === vendoredPrint) {
    console.log(`droidplug Java matches btleplug ${btleplug.version} (${javaFiles(source).length} files)`);
    process.exit(0);
  }
  console.error(
    [
      `Vendored droidplug Java does not match btleplug ${btleplug.version}.`,
      `  crate: ${sourcePrint}`,
      `  repo:  ${vendoredPrint}`,
      '',
      'Run `node apps/console/scripts/sync-droidplug-java.mjs` and commit the result.'
    ].join('\n')
  );
  process.exit(1);
}

rmSync(vendorRoot, { recursive: true, force: true });
cpSync(source, vendorRoot, { recursive: true });
console.log(
  `Copied ${javaFiles(vendorRoot).length} droidplug Java files from btleplug ${btleplug.version}`
);
