import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Builds the unsigned Fedora RPM natively. Unlike the Windows installer there
// is no cross-build here: WebKitGTK, libudev and libdbus are all linked against
// system libraries, and the resulting package declares runtime dependencies by
// Fedora package name, so the build has to happen on the distribution it
// targets. See docs/FEDORA_RELEASE.md.

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const tauriWrapper = join(scriptDirectory, 'tauri.mjs');
const version = JSON.parse(
  readFileSync(join(repositoryRoot, 'apps/console/package.json'), 'utf8')
).version;

const rawArguments = process.argv.slice(2);
const buildArguments = rawArguments.filter(
  (argument) => argument !== '--no-sign' && argument !== '--ci'
);

for (const owned of ['--bundles', '--runner']) {
  if (buildArguments.some((argument) => argument === owned || argument.startsWith(`${owned}=`))) {
    throw new Error(`build-linux-rpm.mjs owns ${owned}; do not pass it explicitly.`);
  }
}

if (process.platform !== 'linux') {
  throw new Error(
    `The Fedora RPM builds on Linux only; this is ${process.platform}. ` +
      'Use the fedora-linux CI job, or a Fedora 44 container: ' +
      'docker run --rm -it -v "$PWD:/src" -w /src fedora:44'
  );
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
    stdio: options.capture ? 'pipe' : 'inherit'
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    const output = [result.stdout, result.stderr].filter(Boolean).join('\n').trim();
    throw new Error(
      `${basename(command)} exited with status ${result.status}${output ? `:\n${output}` : ''}`
    );
  }
  return result;
}

function available(command, arguments_) {
  const result = spawnSync(command, arguments_, {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
    stdio: 'pipe'
  });
  return !result.error && result.status === 0;
}

// Each requirement is checked up front so a missing library fails with its dnf
// command instead of a pkg-config error a thousand lines into the cargo build.
// The three modules are checked rather than the packages that carry them,
// because only the first is named the same on every distribution.
const REQUIRED_MODULES = [
  {
    module: 'webkit2gtk-4.1',
    package: 'webkit2gtk4.1-devel',
    reason: 'the webview the desktop shell renders into'
  },
  {
    module: 'libudev',
    package: 'systemd-devel',
    reason: 'serial-port enumeration for the e-paper panel'
  },
  {
    module: 'dbus-1',
    package: 'dbus-devel',
    reason: 'the BlueZ and tray bridges'
  }
];

if (!available('pkg-config', ['--version'])) {
  throw new Error('pkg-config is missing. Install it with `sudo dnf install pkgconf-pkg-config`.');
}

const missing = REQUIRED_MODULES.filter(
  (requirement) => !available('pkg-config', ['--exists', requirement.module])
);
if (missing.length > 0) {
  throw new Error(
    [
      'The Fedora build toolchain is incomplete:',
      ...missing.map(
        (requirement) =>
          `- ${requirement.module} (${requirement.reason}) — \`sudo dnf install ${requirement.package}\``
      )
    ].join('\n')
  );
}

run(process.execPath, [
  tauriWrapper,
  'build',
  '--bundles',
  'rpm',
  '--no-sign',
  '--ci',
  ...buildArguments
]);

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

const rpmDirectory = join(repositoryRoot, 'target', 'release', 'bundle', 'rpm');
const rpmPackage = versionedEntry(rpmDirectory, version, '.rpm');
const packageBytes = lstatSync(rpmPackage).size;
const sha256 = createHash('sha256').update(readFileSync(rpmPackage)).digest('hex');

console.log(`Package: ${rpmPackage}`);
console.log(`Size: ${packageBytes} bytes (${(packageBytes / (1024 * 1024)).toFixed(2)} MiB)`);
console.log(`SHA-256: ${sha256}`);
console.log(`Verify on the test machine with: sha256sum ${basename(rpmPackage)}`);
console.log(`Install with: sudo dnf install ./${basename(rpmPackage)}`);
