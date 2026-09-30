'use strict';
// Yougori launcher repairs for projects copied from another system, run inside the container.
// Never fails a launch: every repair is best effort.
//   node prepare.cjs workspace TWO_WAY   before each start, in /workspace
//   node prepare.cjs optional            after npm installs a folder, in that folder
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const SKIP = new Set(['node_modules', '.git']);

// Shell scripts copied from Windows have no executable bit, and editors or Git's autocrlf give
// them CRLF line endings that Linux shells reject. Scripts are *.sh/*.bash files and files with
// a #! line in bin/ or scripts/ folders. Line endings change only with one-way sync, where the
// container's copy never syncs back to the PC; with two-way sync only the mode changes.
function workspace(root, twoWay) {
  const stack = [root];
  let budget = 200000;
  while (stack.length && budget > 0) {
    const dir = stack.pop();
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { continue; }
    for (const entry of entries) {
      budget -= 1;
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!SKIP.has(entry.name)) stack.push(file);
      } else if (entry.isFile()) {
        const shell = /\.(sh|bash)$/.test(entry.name);
        if (shell || ['bin', 'scripts'].includes(path.basename(dir))) repair(file, shell, twoWay);
      }
    }
  }
}

function repair(file, shell, twoWay) {
  let data;
  try {
    if (fs.statSync(file).size > 4 * 1024 * 1024) return;
    data = fs.readFileSync(file);
  } catch { return; }
  const bang = data[0] === 0x23 && data[1] === 0x21;
  if (!shell && !bang) return;
  if (!twoWay && data.includes(13)) {
    const text = data.toString('latin1');
    const first = text.indexOf('\n');
    const interpreter = bang ? text.slice(0, first < 0 ? text.length : first) : '';
    // A shell reads every line; other interpreters only need a clean #! line.
    const fixed = shell || /\b(sh|bash|dash|zsh|ksh)\b/.test(interpreter)
      ? text.replace(/\r\n/g, '\n')
      : text.replace(/^([^\n]*?)\r\n/, '$1\n');
    if (fixed !== text) {
      try { fs.writeFileSync(file, Buffer.from(fixed, 'latin1')); } catch { /* read-only: leave it */ }
    }
  }
  try {
    const { mode } = fs.statSync(file);
    if (!(mode & 0o100)) fs.chmodSync(file, mode | 0o111);
  } catch { /* not ours to change */ }
}

// Debian provides python3 only; scripts written on Windows and macOS often call `python`.
function alias() {
  const dirs = String(process.env.PATH || '').split(':').filter(Boolean);
  const find = (name) => dirs.map((dir) => path.join(dir, name)).find((file) => {
    try { fs.accessSync(file, fs.constants.X_OK); return true; } catch { return false; }
  });
  const python3 = !find('python') && find('python3');
  if (python3) {
    try { fs.symlinkSync(python3, '/usr/local/bin/python'); } catch { /* not root, or already there */ }
  }
}

// Package folders directly in node_modules, including scoped ones.
function packages(modules) {
  const found = [];
  let names = [];
  try { names = fs.readdirSync(modules); } catch { return found; }
  for (const name of names) {
    if (name.startsWith('.')) continue;
    if (!name.startsWith('@')) { found.push(path.join(modules, name)); continue; }
    try { for (const scoped of fs.readdirSync(path.join(modules, name))) found.push(path.join(modules, name, scoped)); } catch { /* not a folder */ }
  }
  return found;
}

// npm skips the Linux builds of native packages (Rollup, esbuild, SWC, Lightning CSS, Tailwind,
// Sharp...) when package-lock.json was written on another system (npm/cli#4828). They are the
// optional dependencies named for this OS, CPU and C library that are not installed.
function missingOptional(folder, arch = process.arch, musl = isMusl()) {
  const cpu = { x64: ['x64', 'x86_64', 'amd64'], arm64: ['arm64', 'aarch64'] }[arch];
  if (!cpu) return [];
  const fits = (name) => {
    const n = name.toLowerCase();
    return n.includes('linux') && n.includes('musl') === musl && cpu.some((c) => n.includes(c));
  };
  const installed = (base, name) => fs.existsSync(path.join(base, 'node_modules', name, 'package.json'));
  const missing = new Map();
  for (const dir of packages(path.join(folder, 'node_modules'))) {
    let manifest;
    try { manifest = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')); } catch { continue; }
    for (const [name, version] of Object.entries(manifest.optionalDependencies || {})) {
      if (fits(name) && typeof version === 'string' && !installed(folder, name) && !installed(dir, name)) missing.set(name, version);
    }
  }
  return [...missing].map(([name, version]) => `${name}@${version}`);
}

function isMusl() {
  try { return !process.report.getReport().header.glibcVersionRuntime; } catch { return false; }
}

function optional(folder) {
  if (!['package-lock.json', 'npm-shrinkwrap.json'].some((name) => fs.existsSync(path.join(folder, name)))) return;
  const specs = missingOptional(folder);
  if (!specs.length) return;
  process.stdout.write(`Installing Linux builds npm skipped: ${specs.join(' ')}\n`);
  spawnSync('npm', ['install', '--no-save', '--no-audit', '--no-fund', ...specs], { cwd: folder, stdio: 'inherit' });
}

if (require.main === module) {
  try {
    if (process.argv[2] === 'workspace') {
      workspace(process.cwd(), process.argv[3] === '1');
      alias();
    } else if (process.argv[2] === 'optional') {
      optional(process.cwd());
    }
  } catch { /* repairs are best effort */ }
} else {
  module.exports = { workspace, missingOptional };
}
