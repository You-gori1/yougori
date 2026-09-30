"""Independently require notices for every locked Rust and production npm dependency."""
import argparse
import json
import hashlib
import re
from pathlib import Path
import runpy

M = runpy.run_path(str(Path(__file__).with_name('collect-compliance.py')))


def check(root):
    records = json.loads((root / 'compliance/evidence/application-dependencies.json').read_text(encoding='utf-8'))
    locked_records = [p for p in records if p['ecosystem'] in ('cargo', 'npm')]
    indexed = {(p['ecosystem'], p['name'], p['version']): p for p in locked_records}
    if len(indexed) != len(locked_records):
        raise ValueError('Duplicate application dependency records')
    failures = []
    notices = (root / 'src-tauri/resources/APPLICATION_LICENSES.txt').read_text(encoding='utf-8')
    # Hash complete rendered sections, not just package names or declarations.
    sections = re.split(r'(?m)(?=^={72}\n(?:cargo|cargo-vendored|npm|vendored): )', notices)
    section_hashes = set()
    for section in sections:
        if section.startswith('=' * 72 + '\ncargo: '):
            # Ignore joining blank lines while retaining the full license text,
            # including any separator lines inside an upstream license.
            rendered = section.rstrip('\n') + '\n'
            section_hashes.add(hashlib.sha256(rendered.encode()).hexdigest())
    for (name, version), package in M['cargo_packages'](root).items():
        record = indexed.get(('cargo', name, version))
        if not record or record.get('checksum') != package['checksum'] or not record.get('texts'):
            failures.append(f'cargo {name} {version}: missing notices or wrong locked checksum')
        elif record.get('license', 'UNKNOWN') in ('UNKNOWN', '', 'UNLICENSED'):
            failures.append(f'cargo {name} {version}: unreviewed license')
        elif not record.get('noticeSectionSha256') or any(h not in section_hashes for h in record['noticeSectionSha256']):
            failures.append(f'cargo {name} {version}: full notice text missing from shipped notices')
    lock = json.loads((root / 'package-lock.json').read_text(encoding='utf-8'))
    for path, package in lock.get('packages', {}).items():
        if not path or package.get('dev'):
            continue
        name = package.get('name', path.split('node_modules/')[-1])
        record = indexed.get(('npm', name, package['version']))
        if not record or record.get('integrity') != package.get('integrity') or not record.get('texts'):
            failures.append(f'npm {name} {package["version"]}: missing notices or wrong locked integrity')
    report = json.loads((root / 'compliance/release.json').read_text(encoding='utf-8'))
    inputs = {p['path'] for p in report['inputs']}
    for name in M['cargo_lockfiles'](root):
        if name not in inputs:
            failures.append(f'{name}: missing from compliance inputs')
    if failures:
        raise ValueError('Dependency coverage incomplete:\n' + '\n'.join(failures))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    check(args.root.resolve())
    print('All locked Rust and production npm dependencies have matching notice records.')
