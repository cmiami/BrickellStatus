import { appendFileSync, existsSync, lstatSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_DMG_BYTES = 25 * 1024 * 1024;
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const bundleDirectory = resolve(
  process.argv[2] ?? join(repositoryRoot, 'target/release/bundle')
);

function firstEntry(directory, suffix) {
  const entries = readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.name.endsWith(suffix))
    .map((entry) => join(directory, entry.name))
    .sort();
  if (entries.length !== 1) {
    throw new Error(`Expected one ${suffix} in ${directory}; found ${entries.length}.`);
  }
  return entries[0];
}

function payloadBytes(path) {
  const stat = lstatSync(path);
  if (!stat.isDirectory()) return stat.size;
  return readdirSync(path).reduce((total, entry) => total + payloadBytes(join(path, entry)), 0);
}

function mib(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

const app = firstEntry(join(bundleDirectory, 'macos'), '.app');
const dmg = firstEntry(join(bundleDirectory, 'dmg'), '.dmg');
const licenseDirectory = join(app, 'Contents', 'Resources', 'licenses');
const requiredLicenseFiles = [
  'THIRD_PARTY_NOTICES.md',
  'DEPENDENCY_LICENSES.txt',
  'LICENSE-MIT',
  'LICENSE-APACHE'
];
for (const file of requiredLicenseFiles) {
  const path = join(licenseDirectory, file);
  if (!existsSync(path) || lstatSync(path).size === 0) {
    throw new Error(`Required bundled license resource is missing or empty: ${path}`);
  }
}
const appBytes = payloadBytes(app);
const dmgBytes = lstatSync(dmg).size;
const report = [
  `App: ${basename(app)} — ${appBytes} bytes (${mib(appBytes)} MiB payload)`,
  `DMG: ${basename(dmg)} — ${dmgBytes} bytes (${mib(dmgBytes)} MiB)`,
  `Budget: ${MAX_DMG_BYTES} bytes (25.00 MiB)`,
  `Licenses: ${requiredLicenseFiles.length} required resources present`
].join('\n');

console.log(report);

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    [
      '### Unsigned macOS artifact size',
      '',
      '| Artifact | Exact bytes | MiB |',
      '| --- | ---: | ---: |',
      `| ${basename(app)} payload | ${appBytes} | ${mib(appBytes)} |`,
      `| ${basename(dmg)} | ${dmgBytes} | ${mib(dmgBytes)} |`,
      '',
      `DMG budget: **25.00 MiB** (${MAX_DMG_BYTES} bytes).`,
      `Required bundled license resources: **${requiredLicenseFiles.length} present**.`,
      ''
    ].join('\n')
  );
}

if (dmgBytes > MAX_DMG_BYTES) {
  throw new Error(
    `${basename(dmg)} is ${mib(dmgBytes)} MiB, above the 25.00 MiB release budget.`
  );
}
