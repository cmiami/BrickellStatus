#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const mode = process.argv[2] ?? 'all';
if (!['console', 'core', 'all'].includes(mode) || process.argv.length > 3) {
  console.error('Usage: node scripts/verify.mjs [console|core|all]');
  process.exit(2);
}

const manifest = JSON.parse(readFileSync(resolve(root, 'apps/console/package.json'), 'utf8'));
const minimum = manifest.engines.node.match(/^>=(\d+)\.(\d+)\.(\d+)$/)?.slice(1).map(Number);
if (!minimum) throw new Error('Update verify.mjs to handle the configured Node engine range.');
const difference = process.versions.node.split('.').map((part, index) => Number(part) - minimum[index]);
if ((difference.find((part) => part !== 0) ?? 0) < 0) {
  console.error(`Node ${manifest.engines.node} is required; running ${process.versions.node}.`);
  process.exit(1);
}

function run(command, args) {
  console.log(`\n> ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    // Windows npm is a .cmd launcher. All arguments here are fixed strings.
    shell: process.platform === 'win32' && command === 'npm'
  });
  if (result.error) console.error(result.error.message);
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const npm = (script) => run('npm', ['--prefix', 'apps/console', 'run', script]);

if (mode !== 'console') {
  run('cargo', ['fmt', '--all', '--check']);
  run(process.platform === 'win32' ? 'python' : 'python3', [
    '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_*.py'
  ]);
}

if (mode !== 'core') {
  npm('check');
  npm('test');
  if (mode === 'all') {
    npm('licenses:bundle');
    run(process.platform === 'win32' ? 'python' : 'python3', [
      'firmware/panel/scripts/bundle_firmware.py', '--skip-build'
    ]);
  }
  npm('build');
}

if (mode !== 'console') {
  const workspace = ['--workspace', '--locked'];
  if (mode === 'core') workspace.push('--exclude', 'brickellstatus-desktop');
  run('cargo', ['test', ...workspace]);
  run('cargo', ['clippy', ...workspace, '--all-targets', '--', '-D', 'warnings']);
}

console.log(`\n${mode} verification passed.`);
