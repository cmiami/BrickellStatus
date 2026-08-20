import { spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, lstatSync, readFileSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_PACKAGE_BYTES = 25 * 1024 * 1024;
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const version = JSON.parse(
  readFileSync(join(repositoryRoot, 'apps/console/package.json'), 'utf8')
).version;
const releaseDirectory = resolve(process.argv[2] ?? join(repositoryRoot, 'target/release'));

// Named by version rather than by being the only file present: the bundle
// directory keeps every package ever built there, so after a version bump the
// previous release's artifact is still sitting beside this one.
function versionedEntry(directory, version, suffix) {
  const entries = readdirSync(directory, { withFileTypes: true })
    .filter(
      (entry) => entry.isFile() && entry.name.endsWith(suffix) && entry.name.includes(`-${version}-`)
    )
    .map((entry) => join(directory, entry.name))
    .sort();
  if (entries.length !== 1) {
    throw new Error(`Expected one ${version} ${suffix} in ${directory}; found ${entries.length}.`);
  }
  return entries[0];
}

function mib(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

const executable = join(releaseDirectory, 'brickellstatus-desktop');
if (!existsSync(executable)) {
  throw new Error(`Built Linux executable is missing: ${executable}`);
}
const rpmPackage = versionedEntry(join(releaseDirectory, 'bundle', 'rpm'), version, '.rpm');

// The package payload is the only honest place to check this. Tauri's RPM
// writer is the pure-Rust `rpm` crate, not rpmbuild, so nothing between the
// config and the artifact verifies that a mapped file was found: a typo in
// `bundle.linux.rpm.files` drops the udev rule silently and the first symptom
// is a user whose board is visible but cannot be opened.
const listing = spawnSync('rpm', ['-qpl', rpmPackage], { encoding: 'utf8' });
if (listing.error || listing.status !== 0) {
  throw new Error(
    `Could not list ${basename(rpmPackage)} with rpm: ${
      listing.error?.message ?? listing.stderr?.trim() ?? `status ${listing.status}`
    }`
  );
}
const packagedPaths = listing.stdout
  .split('\n')
  .map((line) => line.trim())
  .filter(Boolean);

const requiredLicenseFiles = [
  'THIRD_PARTY_NOTICES.md',
  'DEPENDENCY_LICENSES.txt',
  'LICENSE-MIT',
  'LICENSE-APACHE'
];

// Matched by suffix rather than by absolute path: Tauri decides the resource
// root (/usr/lib/<binary>/), and pinning it here would turn a harmless upstream
// relayout into a release-blocking failure.
const requiredPayload = [
  { label: 'executable', suffix: '/bin/brickellstatus-desktop' },
  { label: 'desktop entry', suffix: '.desktop' },
  { label: 'udev rule', suffix: '/70-brickellstatus-espressif.rules' },
  { label: 'firmware manifest', suffix: '/firmware/manifest.json' },
  ...requiredLicenseFiles.map((file) => ({
    label: `license: ${file}`,
    suffix: `/licenses/${file}`
  }))
];

const absent = requiredPayload.filter(
  (requirement) => !packagedPaths.some((path) => path.endsWith(requirement.suffix))
);
if (absent.length > 0) {
  throw new Error(
    [
      `${basename(rpmPackage)} is missing required payload entries:`,
      ...absent.map((requirement) => `- ${requirement.label} (expected a path ending ${requirement.suffix})`)
    ].join('\n')
  );
}

const executableBytes = lstatSync(executable).size;
const packageBytes = lstatSync(rpmPackage).size;
const report = [
  `Executable: ${basename(executable)} — ${executableBytes} bytes (${mib(executableBytes)} MiB)`,
  `Package: ${basename(rpmPackage)} — ${packageBytes} bytes (${mib(packageBytes)} MiB)`,
  `Budget: ${MAX_PACKAGE_BYTES} bytes (25.00 MiB)`,
  `Payload: ${requiredPayload.length} required entries present of ${packagedPaths.length} packaged paths`
].join('\n');

console.log(report);

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    [
      '### Unsigned Fedora artifact size',
      '',
      '| Artifact | Exact bytes | MiB |',
      '| --- | ---: | ---: |',
      `| ${basename(executable)} | ${executableBytes} | ${mib(executableBytes)} |`,
      `| ${basename(rpmPackage)} | ${packageBytes} | ${mib(packageBytes)} |`,
      '',
      `Package budget: **25.00 MiB** (${MAX_PACKAGE_BYTES} bytes).`,
      `Required payload entries: **${requiredPayload.length} present**.`,
      ''
    ].join('\n')
  );
}

if (packageBytes > MAX_PACKAGE_BYTES) {
  throw new Error(
    `${basename(rpmPackage)} is ${mib(packageBytes)} MiB, above the 25.00 MiB release budget.`
  );
}
