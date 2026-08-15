import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const consoleDirectory = resolve(scriptDirectory, '..');
const desktopDirectory = resolve(scriptDirectory, '../../desktop');
const cli = resolve(consoleDirectory, 'node_modules/@tauri-apps/cli/tauri.js');

if (!existsSync(cli)) {
  console.error('Tauri CLI is not installed. Run `npm ci` in apps/console first.');
  process.exit(1);
}

const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], {
  cwd: desktopDirectory,
  env: process.env,
  stdio: 'inherit'
});

if (result.error) {
  console.error(`Could not start the Tauri CLI: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
