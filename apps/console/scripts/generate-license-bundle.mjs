import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync
} from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const consoleDirectory = resolve(scriptDirectory, '..');
const repositoryRoot = resolve(scriptDirectory, '../../..');
const desktopManifest = resolve(repositoryRoot, 'apps/desktop/src-tauri/Cargo.toml');
const outputDirectory = resolve(
  repositoryRoot,
  'apps/desktop/src-tauri/resources/licenses'
);
const cargoTargets = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc'
];
const candidateName = /^(licen[cs]e|copying|notice|copyright|patents|ofl)([._-].*)?$/i;
const maxLicenseBytes = 2 * 1024 * 1024;

function runJson(command, args, cwd = repositoryRoot) {
  const result = spawnSync(command, args, {
    cwd,
    env: process.env,
    encoding: 'utf8',
    maxBuffer: 128 * 1024 * 1024,
    // Node refuses to spawn a .cmd shim directly on Windows and fails with
    // EINVAL. Every argument here is a literal, so the shell cannot be fed
    // anything it should not receive.
    shell: process.platform === 'win32' && command.endsWith('.cmd')
  });
  if (result.error || result.status !== 0) {
    const detail = result.stderr?.trim() || result.error?.message || 'unknown failure';
    throw new Error(`${command} ${args.join(' ')} failed: ${detail}`);
  }
  return JSON.parse(result.stdout);
}

function normalizedText(path) {
  const size = statSync(path).size;
  if (size > maxLicenseBytes) {
    throw new Error(`License candidate is unexpectedly large: ${path} (${size} bytes)`);
  }
  const text = readFileSync(path, 'utf8').replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').trim();
  if (!text || text.includes('\0')) {
    throw new Error(`License candidate is not usable text: ${path}`);
  }
  return `${text}\n`;
}

function licenseFiles(directory, explicitPath) {
  const paths = new Set();
  if (explicitPath) {
    paths.add(resolve(directory, explicitPath));
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isFile() && candidateName.test(entry.name)) {
      paths.add(join(directory, entry.name));
    }
    if (entry.isDirectory() && /^(licenses?|notices?)$/i.test(entry.name)) {
      const nested = join(directory, entry.name);
      for (const child of readdirSync(nested, { withFileTypes: true })) {
        if (child.isFile()) paths.add(join(nested, child.name));
      }
    }
  }
  return [...paths].sort();
}

function cargoPackages() {
  const selected = new Map();
  for (const target of cargoTargets) {
    const metadata = runJson('cargo', [
      'metadata',
      '--format-version',
      '1',
      '--locked',
      '--filter-platform',
      target,
      '--manifest-path',
      desktopManifest
    ]);
    const root = metadata.packages.find(
      (entry) => resolve(entry.manifest_path) === desktopManifest
    );
    if (!root) throw new Error(`Desktop package is missing from Cargo metadata for ${target}.`);
    const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
    const packages = new Map(metadata.packages.map((entry) => [entry.id, entry]));
    const pending = [root.id];
    const reachable = new Set();
    while (pending.length > 0) {
      const id = pending.pop();
      if (reachable.has(id)) continue;
      reachable.add(id);
      for (const dependency of nodes.get(id)?.deps ?? []) pending.push(dependency.pkg);
    }
    for (const id of reachable) {
      const entry = packages.get(id);
      if (entry?.source) selected.set(entry.id, entry);
    }
  }
  return [...selected.values()]
    .sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`))
    .map((entry) => ({
      ecosystem: 'Rust',
      name: entry.name,
      version: entry.version,
      license: entry.license || 'NOASSERTION',
      authors: entry.authors ?? [],
      repository: entry.repository || entry.homepage || entry.source,
      directory: dirname(entry.manifest_path),
      explicitLicense: entry.license_file
    }));
}

function npmPackages() {
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const tree = runJson(npm, ['ls', '--all', '--json', '--long'], consoleDirectory);
  const selected = new Map();
  function visit(node) {
    for (const dependency of Object.values(node.dependencies ?? {})) {
      if (dependency.path && dependency.name && dependency.version) {
        selected.set(`${dependency.name}@${dependency.version}`, dependency);
      }
      visit(dependency);
    }
  }
  visit(tree);
  return [...selected.values()]
    .sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`))
    .map((entry) => ({
      ecosystem: 'npm',
      name: entry.name,
      version: entry.version,
      license: entry.license || 'NOASSERTION',
      authors: [
        formatPerson(entry.author),
        ...asPeople(entry.contributors).map(formatPerson)
      ].filter(Boolean),
      repository: formatRepository(entry.repository) || entry.homepage || entry.resolved,
      directory: entry.path,
      explicitLicense: null
    }));
}

function asPeople(value) {
  if (!value) return [];
  return Array.isArray(value) ? value : [value];
}

function formatPerson(person) {
  if (!person) return '';
  if (typeof person === 'string') return person;
  return [person.name, person.email && `<${person.email}>`, person.url && `(${person.url})`]
    .filter(Boolean)
    .join(' ');
}

function formatRepository(repository) {
  if (!repository) return '';
  if (typeof repository === 'string') return repository;
  return repository.url || '';
}

function fingerprint(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function contentHash(content) {
  return createHash('sha256').update(content).digest('hex');
}

function permittedFallback(expression) {
  const candidates = ['MIT', 'Apache-2.0', 'BSD-3-Clause', 'MPL-2.0', 'ISC'];
  return candidates.find((identifier) => expression.includes(identifier));
}

function fallbackText(record, identifier, standardText) {
  const holders =
    record.authors.length > 0
      ? record.authors.join('; ')
      : `contributors to ${record.name}; see ${record.repository || 'the package repository'}`;
  if (identifier === 'MIT') {
    const terms = standardText.slice(standardText.indexOf('Permission is hereby granted'));
    return `Copyright (c) ${holders}\n\n${terms}`;
  }
  if (identifier === 'ISC') {
    const terms = standardText.slice(standardText.indexOf('Permission to use, copy, modify'));
    return `ISC License\n\nCopyright (c) ${holders}\n\n${terms}`;
  }
  return standardText;
}

const records = [...cargoPackages(), ...npmPackages()];
const blocks = new Map();
const missing = [];
const canonical = new Map([
  ['MIT', normalizedText(resolve(repositoryRoot, 'LICENSE-MIT'))],
  ['Apache-2.0', normalizedText(resolve(repositoryRoot, 'LICENSE-APACHE'))]
]);

function addBlock(content, reference) {
  const hash = contentHash(content);
  const block = blocks.get(hash) ?? { content, references: [] };
  block.references.push(reference);
  blocks.set(hash, block);
}

for (const record of records) {
  const files = licenseFiles(record.directory, record.explicitLicense);
  record.files = files;
  if (files.length === 0) {
    missing.push(record);
    continue;
  }
  for (const file of files) {
    const content = normalizedText(file);
    if (!canonical.has(record.license) && ['BSD-3-Clause', 'MPL-2.0', 'ISC'].includes(record.license)) {
      canonical.set(record.license, content);
    }
    addBlock(content, {
      package: `${record.ecosystem}:${record.name}@${record.version}`,
      declared: record.license,
      source: basename(file)
    });
  }
}

for (const record of missing) {
  const fallback = permittedFallback(record.license);
  const content = fallback && canonical.get(fallback);
  if (!fallback || !content) {
    throw new Error(
      `No shipped license file or vetted fallback for ${record.ecosystem}:${record.name}@${record.version} (${record.license}).`
    );
  }
  addBlock(fallbackText(record, fallback, content), {
    package: `${record.ecosystem}:${record.name}@${record.version}`,
    declared: record.license,
    source: `standard ${fallback} text; package archive contained no license file`
  });
}

const inventory = records
  .map((record) => {
    const author = record.authors.length > 0 ? record.authors.join('; ') : 'see repository metadata';
    return `- ${record.ecosystem}:${record.name}@${record.version} — ${record.license} — ${author} — ${record.repository || 'repository not declared'}`;
  })
  .join('\n');

const legalBlocks = [...blocks.entries()]
  .sort((left, right) => {
    const leftName = left[1].references.map((entry) => entry.package).sort()[0];
    const rightName = right[1].references.map((entry) => entry.package).sort()[0];
    return leftName.localeCompare(rightName);
  })
  .map(([hash, block], index) => {
    const references = block.references
      .sort((left, right) => left.package.localeCompare(right.package))
      .map(
        (entry) =>
          `- ${entry.package} — declared ${entry.declared} — ${entry.source}`
      )
      .join('\n');
    return [
      `## License text ${String(index + 1).padStart(3, '0')} · SHA-256 ${hash}`,
      '',
      'Applies to:',
      '',
      references,
      '',
      '----- BEGIN VERBATIM LICENSE OR NOTICE TEXT -----',
      block.content.trimEnd(),
      '----- END VERBATIM LICENSE OR NOTICE TEXT -----'
    ].join('\n');
  })
  .join('\n\n');

const report = [
  '# Bundled dependency license texts',
  '',
  'Generated from the locked dependency graphs for the Tender’s Log desktop app.',
  'Identical legal texts are stored once and every covered package is listed.',
  'Build-only npm packages are intentionally included so the report errs toward notice retention.',
  '',
  `- Cargo.lock SHA-256: ${fingerprint(resolve(repositoryRoot, 'Cargo.lock'))}`,
  `- package-lock.json SHA-256: ${fingerprint(resolve(consoleDirectory, 'package-lock.json'))}`,
  `- Rust packages: ${records.filter((record) => record.ecosystem === 'Rust').length}`,
  `- npm packages: ${records.filter((record) => record.ecosystem === 'npm').length}`,
  `- Unique legal texts: ${blocks.size}`,
  `- Packages using a declared-license fallback because their archive omitted a license file: ${missing.length}`,
  '',
  '## Package inventory',
  '',
  inventory,
  '',
  legalBlocks,
  ''
].join('\n');

rmSync(outputDirectory, { recursive: true, force: true });
mkdirSync(outputDirectory, { recursive: true });
writeFileSync(resolve(outputDirectory, 'DEPENDENCY_LICENSES.txt'), report);
writeFileSync(
  resolve(outputDirectory, 'THIRD_PARTY_NOTICES.md'),
  readFileSync(resolve(repositoryRoot, 'THIRD_PARTY_NOTICES.md'))
);
writeFileSync(resolve(outputDirectory, 'LICENSE-MIT'), readFileSync(resolve(repositoryRoot, 'LICENSE-MIT')));
writeFileSync(
  resolve(outputDirectory, 'LICENSE-APACHE'),
  readFileSync(resolve(repositoryRoot, 'LICENSE-APACHE'))
);

console.log(
  `Wrote ${records.length} package records and ${blocks.size} unique license texts to ${outputDirectory}`
);
