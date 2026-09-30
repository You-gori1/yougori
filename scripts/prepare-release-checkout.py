"""Prepare a private release checkout without changing the developer's Git index.

Run with a NEW destination directory. Runtime payloads and dependencies are copied,
never linked to live backing images. Build/source caches are shared separately.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE_ROOTS = ('src/', 'src-tauri/', 'assets/', 'public/', 'cli/', 'engine/', 'crates/', '.cargo/', 'vault/', 'vendor/', 'appliance/',
                'runtime/', 'scripts/', 'skills/', 'docs/', 'compliance/', 'e2e/', '.github/')
ROOT_ASSET_SUFFIXES = {'.svg', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.ico', '.woff', '.woff2', '.ttf', '.otf'}
ROOT_FILES = {'LICENSE', 'COPYING', 'NOTICE', 'COMMERCIAL_LICENSE.txt', '.gitignore',
              '.gitattributes', 'package.json', 'package-lock.json', 'index.html',
              'vite.config.ts', 'eslint.config.js', 'tsconfig.json', 'tsconfig.app.json',
              'tsconfig.node.json', 'playwright.config.ts', 'playwright.graph.config.ts',
              'components.json'}


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, **kwargs).decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    args = parser.parse_args()
    destination = args.destination.resolve()
    if destination.exists():
        raise ValueError('Use a new release checkout directory; existing files are never overwritten')
    tracked = set(run('git', 'ls-files', '-z').split('\0'))
    untracked = set(run('git', 'ls-files', '--others', '--exclude-standard', '-z').split('\0'))
    names = sorted(n for n in tracked | untracked if n and (n in tracked or n in ROOT_FILES or n.startswith(SOURCE_ROOTS)
                   or ('/' not in n and Path(n).suffix.lower() in ROOT_ASSET_SUFFIXES)))
    selected = []
    for name in names:
        path = ROOT / name
        if not path.exists():  # honor intentional source deletions
            continue
        if path.is_symlink() or not path.is_file():
            raise ValueError('Linked/non-file source requires review: ' + name)
        path.resolve().relative_to(ROOT)
        if path.suffix.lower() in ('.pfx', '.p12', '.pem', '.key') or path.name.startswith('.env'):
            raise ValueError('Potential credential must not enter release sources: ' + name)
        selected.append(name)
    destination.mkdir(parents=True)
    for name in selected:
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, target)
    for directory in ('node_modules', 'src-tauri/resources/cli', 'src-tauri/resources/vault'):
        source = ROOT / directory
        target = destination / directory
        if source.exists():
            shutil.copytree(source, target, dirs_exist_ok=True)
        else:
            # A clean native checkout has no bundled executables yet. The
            # release command installs dependencies and builds these natively.
            target.mkdir(parents=True, exist_ok=True)
    staged = args.runtime.resolve()
    # Verify all staged appliance members before copying any over the release copy.
    import hashlib
    appliance = staged / 'appliance'
    for line in (appliance / 'SHA256SUMS').read_text().splitlines():
        checksum, name = line.split()
        if Path(name).name != name or hashlib.sha256((appliance / name).read_bytes()).hexdigest() != checksum:
            raise ValueError('Invalid staged appliance checksum: ' + name)
    for name in ('appliance-base.qcow2', 'initramfs-virt', 'vmlinuz-virt', 'SHA256SUMS'):
        shutil.copy2(appliance / name, destination / 'src-tauri/resources/runtime/appliance' / name)
    cuda = destination / 'src-tauri/resources/runtime/cuda'
    shutil.copy2(staged / 'cuda/opendock-agent', cuda / 'opendock-agent')
    manifest = cuda / 'SHA256SUMS'
    lines = []
    for line in manifest.read_text().splitlines():
        _, name = line.split()
        if Path(name).name != name:
            raise ValueError('Invalid CUDA manifest member')
        lines.append(hashlib.sha256((cuda / name).read_bytes()).hexdigest() + '  ' + name)
    manifest.write_bytes(('\n'.join(lines) + '\n').encode())
    gitdir = run('git', 'rev-parse', '--absolute-git-dir')
    (destination / '.git').write_text('gitdir: ' + gitdir + '\n')
    index = destination.parent / (destination.name + '.index')
    if index.exists():
        raise ValueError('Release index already exists')
    env = dict(os.environ, GIT_DIR=gitdir, GIT_WORK_TREE=str(destination), GIT_INDEX_FILE=str(index))
    subprocess.run(['git', 'read-tree', '--empty'], env=env, check=True)
    # Explicitly selected previously tracked files may now match an ignore rule.
    subprocess.run(['git', 'add', '--force', '--pathspec-from-file=-', '--pathspec-file-nul'],
                   input='\0'.join(selected).encode(), cwd=destination, env=env, check=True)
    (destination.parent / (destination.name + '.environment.json')).write_text(json.dumps(
        {key: env[key] for key in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE')}, indent=2))
    print(f'Prepared {len(selected)} source files in {destination}. Developer index and runtime unchanged.')


if __name__ == '__main__':
    main()
