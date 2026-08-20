import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, lstatSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// The same 25 MiB release budget the DMG and the NSIS installer answer to
// (CONTRIBUTING.md). Applied per ABI, which is why the release build splits
// rather than shipping one universal APK carrying every architecture.
const MAX_APK_BYTES = 25 * 1024 * 1024;
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const outputsDirectory = resolve(
  process.argv[2] ??
    join(repositoryRoot, 'apps/desktop/src-tauri/gen/android/app/build/outputs/apk')
);

// Licences ride inside the APK rather than on disk beside it, so the presence
// check reads the archive's own table of contents.
const requiredLicenseFiles = [
  'assets/licenses/THIRD_PARTY_NOTICES.md',
  'assets/licenses/DEPENDENCY_LICENSES.txt',
  'assets/licenses/LICENSE-MIT',
  'assets/licenses/LICENSE-APACHE'
];

function mib(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

function releaseApks(root) {
  if (!existsSync(root)) {
    throw new Error(`No Android APK output directory at ${root}.`);
  }
  const found = [];
  for (const abi of readdirSync(root, { withFileTypes: true })) {
    if (!abi.isDirectory()) continue;
    const releaseDirectory = join(root, abi.name, 'release');
    if (!existsSync(releaseDirectory)) continue;
    for (const entry of readdirSync(releaseDirectory)) {
      if (entry.endsWith('.apk')) found.push(join(releaseDirectory, entry));
    }
  }
  if (found.length === 0) {
    throw new Error(`No release APKs under ${root}.`);
  }
  return found.sort();
}

// `unzip -Z1` lists entry names only, and ships on every runner this builds on.
function entryNames(apk) {
  return new Set(
    execFileSync('unzip', ['-Z1', apk], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean)
  );
}

const apks = releaseApks(outputsDirectory);
const rows = [];
const oversized = [];

for (const apk of apks) {
  const bytes = lstatSync(apk).size;
  const names = entryNames(apk);
  for (const required of requiredLicenseFiles) {
    if (!names.has(required)) {
      throw new Error(`Required bundled license resource is missing from ${basename(apk)}: ${required}`);
    }
  }
  // The ESP32 images are desktop-only: nothing on a phone can flash a board
  // over USB, so shipping them would be a megabyte and a half of dead weight.
  const firmware = [...names].filter((name) => name.startsWith('assets/firmware/'));
  if (firmware.length > 0) {
    throw new Error(
      `${basename(apk)} carries ${firmware.length} firmware assets that Android cannot flash. ` +
        'Delete gen/android/app/src/main/assets/firmware and rebuild.'
    );
  }
  rows.push({ name: basename(apk), bytes });
  if (bytes > MAX_APK_BYTES) oversized.push({ name: basename(apk), bytes });
}

console.log(
  [
    ...rows.map((row) => `APK: ${row.name} — ${row.bytes} bytes (${mib(row.bytes)} MiB)`),
    `Budget: ${MAX_APK_BYTES} bytes (25.00 MiB) per ABI`,
    `Licenses: ${requiredLicenseFiles.length} required resources present in each APK`
  ].join('\n')
);

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    [
      '### Android artifact size',
      '',
      '| Artifact | Exact bytes | MiB |',
      '| --- | ---: | ---: |',
      ...rows.map((row) => `| ${row.name} | ${row.bytes} | ${mib(row.bytes)} |`),
      '',
      `Per-ABI APK budget: **25.00 MiB** (${MAX_APK_BYTES} bytes).`,
      `Required bundled license resources: **${requiredLicenseFiles.length} present**.`,
      ''
    ].join('\n')
  );
}

if (oversized.length > 0) {
  throw new Error(
    oversized
      .map((row) => `${row.name} is ${mib(row.bytes)} MiB, above the 25.00 MiB release budget.`)
      .join('\n')
  );
}
