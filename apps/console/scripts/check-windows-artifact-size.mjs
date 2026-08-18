import { appendFileSync, existsSync, lstatSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_INSTALLER_BYTES = 25 * 1024 * 1024;
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const releaseDirectory = resolve(
  process.argv[2] ?? join(repositoryRoot, 'target/x86_64-pc-windows-msvc/release')
);

function firstEntry(directory, suffix) {
  const { readdirSync } = require('node:fs');
  const entries = readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.name.endsWith(suffix))
    .map((entry) => join(directory, entry.name))
    .sort();
  if (entries.length !== 1) {
    throw new Error(`Expected one ${suffix} in ${directory}; found ${entries.length}.`);
  }
  return entries[0];
}

function mib(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

const executable = join(releaseDirectory, 'puente-gonorrea-desktop.exe');
if (!existsSync(executable)) {
  throw new Error(`Built Windows executable is missing: ${executable}`);
}
const installer = firstEntry(join(releaseDirectory, 'bundle', 'nsis'), '-setup.exe');

// tauri-build stages mapped resources beside the executable, which is also
// where they land on an installed system (the exe directory is the Windows
// resource base directory).
const licenseDirectory = join(releaseDirectory, 'licenses');
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

const executableBytes = lstatSync(executable).size;
const installerBytes = lstatSync(installer).size;
const report = [
  `Executable: ${basename(executable)} — ${executableBytes} bytes (${mib(executableBytes)} MiB)`,
  `Installer: ${basename(installer)} — ${installerBytes} bytes (${mib(installerBytes)} MiB)`,
  `Budget: ${MAX_INSTALLER_BYTES} bytes (25.00 MiB)`,
  `Licenses: ${requiredLicenseFiles.length} required resources present`
].join('\n');

console.log(report);

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    [
      '### Unsigned Windows artifact size',
      '',
      '| Artifact | Exact bytes | MiB |',
      '| --- | ---: | ---: |',
      `| ${basename(executable)} | ${executableBytes} | ${mib(executableBytes)} |`,
      `| ${basename(installer)} | ${installerBytes} | ${mib(installerBytes)} |`,
      '',
      `Installer budget: **25.00 MiB** (${MAX_INSTALLER_BYTES} bytes).`,
      `Required bundled license resources: **${requiredLicenseFiles.length} present**.`,
      ''
    ].join('\n')
  );
}

if (installerBytes > MAX_INSTALLER_BYTES) {
  throw new Error(
    `${basename(installer)} is ${mib(installerBytes)} MiB, above the 25.00 MiB release budget.`
  );
}
