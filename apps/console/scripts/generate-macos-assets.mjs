import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') {
  console.error('macOS app assets require the built-in `sips` and `iconutil` tools.');
  process.exit(1);
}

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const consoleDirectory = resolve(scriptDirectory, '..');
const desktopDirectory = resolve(scriptDirectory, '../../desktop');
const cli = resolve(consoleDirectory, 'node_modules/@tauri-apps/cli/tauri.js');
const icons = resolve(desktopDirectory, 'src-tauri/icons');
const mark = resolve(icons, 'icon.svg');
const background = resolve(icons, 'dmg-background.svg');
const trayMark = resolve(icons, 'tray-icon.svg');

if (!existsSync(cli)) {
  console.error('Tauri CLI is not installed. Run `npm ci` in apps/console first.');
  process.exit(1);
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: desktopDirectory,
    env: process.env,
    stdio: 'inherit'
  });
  if (result.error || result.status !== 0) {
    console.error(`Asset command failed: ${command}`);
    process.exit(result.status ?? 1);
  }
}

run(process.execPath, [cli, 'icon', mark, '--output', icons]);
run('/usr/bin/sips', [
  '-s',
  'format',
  'png',
  background,
  '--out',
  resolve(icons, 'dmg-background.png')
]);
run('/usr/bin/sips', [
  '-s',
  'format',
  'png',
  trayMark,
  '--out',
  resolve(icons, 'tray-icon.png')
]);
