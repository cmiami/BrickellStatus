import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readFileSync, readdirSync } from 'node:fs';
import { basename, delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Cross-compiles the Windows x64 executable and NSIS installer from macOS
// (cargo-xwin + LLVM's clang-cl/lld-link + Homebrew makensis), or builds
// natively when run on Windows. See docs/WINDOWS_RELEASE.md.

const TARGET = 'x86_64-pc-windows-msvc';
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const tauriWrapper = join(scriptDirectory, 'tauri.mjs');

const rawArguments = process.argv.slice(2);
const buildArguments = rawArguments.filter(
  (argument) => argument !== '--no-sign' && argument !== '--ci'
);

for (const owned of ['--bundles', '--target', '--runner']) {
  if (buildArguments.some((argument) => argument === owned || argument.startsWith(`${owned}=`))) {
    throw new Error(`build-windows-installer.mjs owns ${owned}; do not pass it explicitly.`);
  }
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

function captured(command, arguments_) {
  return run(command, arguments_, { capture: true }).stdout.trim();
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

const crossArguments = [];
if (process.platform === 'darwin') {
  // Each requirement is checked up front so a missing tool fails with its
  // install command instead of deep inside the cargo build.
  const failures = [];
  if (!available('makensis', ['-VERSION'])) {
    failures.push('makensis is missing. Install it with `brew install nsis`.');
  }
  if (!available('cargo', ['xwin', '--version'])) {
    failures.push('cargo-xwin is missing. Install it with `cargo install --locked cargo-xwin`.');
  }
  if (!captured('rustup', ['target', 'list', '--installed']).split('\n').includes(TARGET)) {
    failures.push(`The ${TARGET} target is missing. Install it with \`rustup target add ${TARGET}\`.`);
  }
  const llvmBin = join(captured('brew', ['--prefix', 'llvm']), 'bin');
  if (!existsSync(join(llvmBin, 'clang-cl'))) {
    failures.push('clang-cl is missing. Install it with `brew install llvm`.');
  }
  if (failures.length > 0) {
    throw new Error(`The Windows cross toolchain is incomplete:\n- ${failures.join('\n- ')}`);
  }
  // Homebrew's LLVM is keg-only, so clang-cl and lld-link are never on PATH
  // by default; cargo-xwin resolves them from PATH.
  process.env.PATH = `${llvmBin}${delimiter}${process.env.PATH}`;
  process.env.XWIN_ACCEPT_LICENSE ??= '1';
  crossArguments.push('--runner', 'cargo-xwin');
} else if (process.platform !== 'win32') {
  throw new Error('The Windows installer builds on macOS (cross) or Windows (native) only.');
}

run(process.execPath, [
  tauriWrapper,
  'build',
  '--target',
  TARGET,
  ...crossArguments,
  '--bundles',
  'nsis',
  '--no-sign',
  '--ci',
  ...buildArguments
]);

const nsisDirectory = join(repositoryRoot, 'target', TARGET, 'release', 'bundle', 'nsis');
const installer = firstEntry(nsisDirectory, '-setup.exe');
const installerBytes = lstatSync(installer).size;
const sha256 = createHash('sha256').update(readFileSync(installer)).digest('hex');

console.log(`Installer: ${installer}`);
console.log(`Size: ${installerBytes} bytes (${(installerBytes / (1024 * 1024)).toFixed(2)} MiB)`);
console.log(`SHA-256: ${sha256}`);
console.log('Verify inside the QA VM with: certutil -hashfile <installer> SHA256');
