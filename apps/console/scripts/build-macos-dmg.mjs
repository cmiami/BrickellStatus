import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  unlinkSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, '../../..');
const tauriWrapper = join(scriptDirectory, 'tauri.mjs');
const tauriConfig = JSON.parse(
  readFileSync(join(repositoryRoot, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8')
);
const packageManifest = JSON.parse(
  readFileSync(join(repositoryRoot, 'apps/console/package.json'), 'utf8')
);

const rawArguments = process.argv.slice(2);
const buildArguments = rawArguments.filter(
  (argument) => argument !== '--no-sign' && argument !== '--ci'
);

if (buildArguments.some((argument) => argument === '--bundles' || argument.startsWith('--bundles='))) {
  throw new Error('build-macos-dmg.mjs owns --bundles; do not pass it explicitly.');
}

function argumentValue(name) {
  const equals = buildArguments.find((argument) => argument.startsWith(`${name}=`));
  if (equals) return equals.slice(name.length + 1);
  const index = buildArguments.indexOf(name);
  return index >= 0 ? buildArguments[index + 1] : undefined;
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
    stdio: options.capture ? 'pipe' : 'inherit',
    timeout: options.timeout
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

function architectureSuffix(target) {
  if (target?.startsWith('aarch64-')) return 'aarch64';
  if (target?.startsWith('x86_64-')) return 'x64';
  if (process.arch === 'arm64') return 'aarch64';
  if (process.arch === 'x64') return 'x64';
  throw new Error(`Unsupported macOS packaging architecture: ${target ?? process.arch}`);
}

function waitForNonemptyFile(path, timeoutMilliseconds) {
  const deadline = Date.now() + timeoutMilliseconds;
  do {
    if (existsSync(path) && lstatSync(path).isFile() && lstatSync(path).size > 0) {
      return true;
    }
    sleep(250);
  } while (Date.now() < deadline);
  return false;
}

function sleep(milliseconds) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds);
}

// Ejecting right after the layout script races Finder, which still holds the
// volume open for a moment after its window closes. The retries used to fire
// back to back and were all spent inside that moment, so a release could fail
// on "Resource busy" with nothing actually wrong. Wait between attempts, and
// give the forced eject the same patience rather than one shot.
function detach(device) {
  const attempts = [0, 1_000, 2_000, 4_000, 8_000];
  let lastOutput = '';
  for (const [index, wait] of attempts.entries()) {
    if (wait) sleep(wait);
    const forced = index >= attempts.length - 2;
    const result = spawnSync(
      'hdiutil',
      forced ? ['detach', '-force', device] : ['detach', device],
      { cwd: repositoryRoot, env: process.env, encoding: 'utf8', stdio: 'pipe' }
    );
    if (result.status === 0) return;
    lastOutput = [result.stdout, result.stderr].filter(Boolean).join('\n').trim();
  }
  throw new Error(
    `hdiutil could not detach ${device} after ${attempts.length} attempts${
      lastOutput ? `:\n${lastOutput}` : ''
    }`
  );
}

run(process.execPath, [
  tauriWrapper,
  'build',
  '--bundles',
  'app',
  '--no-sign',
  '--ci',
  ...buildArguments
]);

const target = argumentValue('--target');
const bundleRoot = target
  ? join(repositoryRoot, 'target', target, 'release', 'bundle')
  : join(repositoryRoot, 'target/release/bundle');
const macosDirectory = join(bundleRoot, 'macos');
const dmgDirectory = join(bundleRoot, 'dmg');
const app = firstEntry(macosDirectory, '.app');
for (const entry of readdirSync(macosDirectory, { withFileTypes: true })) {
  if (entry.isFile() && entry.name.startsWith('rw.') && entry.name.endsWith('.dmg')) {
    unlinkSync(join(macosDirectory, entry.name));
  }
}
const productName = tauriConfig.productName;
const version = packageManifest.version;
const finalDmg = join(
  dmgDirectory,
  `${productName}_${version}_${architectureSuffix(target)}.dmg`
);
const temporaryDirectory = mkdtempSync(join(tmpdir(), 'brickellstatus-dmg-'));
const sourceDirectory = join(temporaryDirectory, 'source');
const writableDmg = join(temporaryDirectory, 'writable.dmg');
let attachedDevice;
let mountDirectory;
let styled = false;

try {
  mkdirSync(sourceDirectory);
  mkdirSync(dmgDirectory, { recursive: true });

  for (const entry of readdirSync(dmgDirectory, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.dmg')) {
      unlinkSync(join(dmgDirectory, entry.name));
    }
  }

  cpSync(app, join(sourceDirectory, basename(app)), {
    recursive: true,
    preserveTimestamps: true
  });
  symlinkSync('/Applications', join(sourceDirectory, 'Applications'));
  mkdirSync(join(sourceDirectory, '.background'));
  copyFileSync(
    join(repositoryRoot, 'apps/desktop/src-tauri/icons/dmg-background.png'),
    join(sourceDirectory, '.background/dmg-background.png')
  );
  copyFileSync(
    join(repositoryRoot, 'apps/desktop/src-tauri/icons/icon.icns'),
    join(sourceDirectory, '.VolumeIcon.icns')
  );

  run('hdiutil', [
    'create',
    '-ov',
    '-srcfolder',
    sourceDirectory,
    '-volname',
    productName,
    '-fs',
    'HFS+',
    '-format',
    'UDRW',
    writableDmg
  ]);

  const attachment = run(
    'hdiutil',
    [
      'attach',
      '-readwrite',
      '-noverify',
      '-noautoopen',
      '-nobrowse',
      '-mountrandom',
      '/Volumes',
      writableDmg
    ],
    { capture: true }
  );
  attachedDevice = attachment.stdout
    .split('\n')
    .map((line) => line.trim().split(/\s+/)[0])
    .find((field) => field.startsWith('/dev/disk'));
  const mountedLine = attachment.stdout
    .split('\n')
    .find((line) => line.includes('/Volumes/'));
  mountDirectory = mountedLine?.slice(mountedLine.indexOf('/Volumes/')).trim();
  if (!attachedDevice || !mountDirectory) {
    throw new Error(`Could not identify the attached DMG device:\n${attachment.stdout}`);
  }

  const setFile = '/usr/bin/SetFile';
  if (existsSync(setFile)) {
    run(setFile, ['-c', 'icnC', join(mountDirectory, '.VolumeIcon.icns')]);
    run(setFile, ['-a', 'C', mountDirectory]);
  }

  run('/bin/sleep', ['2']);

  const layout = spawnSync(
    '/usr/bin/osascript',
    [join(scriptDirectory, 'dmg-layout.applescript'), basename(mountDirectory), basename(app)],
    {
      cwd: repositoryRoot,
      env: process.env,
      encoding: 'utf8',
      stdio: 'pipe',
      timeout: 20_000,
      killSignal: 'SIGTERM'
    }
  );
  const dsStore = join(mountDirectory, '.DS_Store');
  styled = layout.status === 0 && waitForNonemptyFile(dsStore, 5_000);

  if (styled) {
    console.log('Applied the Tender\'s Log DMG icon layout.');
  } else {
    // Without a .DS_Store the installer window falls back to Finder's default
    // arrangement, which stacks the app on top of the Applications alias and
    // makes the drag the window exists for impossible. That is a broken
    // installer, not a cosmetic downgrade, so say so loudly.
    const detail = layout.error?.message ?? layout.stderr?.trim() ?? 'Finder wrote no .DS_Store';
    console.warn(
      `DMG icon layout failed; the app and Applications alias will overlap. ${detail}`
    );
    if (existsSync(dsStore)) unlinkSync(dsStore);
    rmSync(join(mountDirectory, '.background'), { recursive: true, force: true });
  }

  rmSync(join(mountDirectory, '.fseventsd'), { recursive: true, force: true });
  rmSync(join(mountDirectory, '.Trashes'), { recursive: true, force: true });
  detach(attachedDevice);
  attachedDevice = undefined;

  run('hdiutil', [
    'convert',
    writableDmg,
    '-format',
    'UDZO',
    '-imagekey',
    'zlib-level=9',
    '-o',
    finalDmg
  ]);
  run('hdiutil', ['verify', finalDmg]);
  console.log(`Created unsigned ${styled ? 'styled' : 'plain'} DMG: ${finalDmg}`);
} finally {
  if (attachedDevice) {
    try {
      detach(attachedDevice);
    } catch (error) {
      console.warn(`Could not detach temporary DMG ${attachedDevice}: ${error.message}`);
    }
  }
  rmSync(temporaryDirectory, { recursive: true, force: true });
}
