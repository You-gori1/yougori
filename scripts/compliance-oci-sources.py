"""Collect the Debian C libraries embedded by nerdctl's recorded release build.

The release log identifies libseccomp and libbtrfs. The exact Docker build-base
image's digest-checked SPDX attestation identifies glibc, including its Debian
patch revision. These inputs are not inferred from a nearby upstream version.
"""
import json
from pathlib import Path
import re
import runpy
import shutil
import tarfile

M = runpy.run_path(str(Path(__file__).with_name('collect-compliance.py')))
ROOT, WORK, BUNDLE = M['ROOT'], M['WORK'], M['BUNDLE']


def main():
    components = [
        ('glibc', '2.41-12+deb13u3', 'g/glibc', 'LGPL-2.1-or-later and per-file notices'),
        ('libseccomp', '2.6.0-2', 'libs/libseccomp', 'LGPL-2.1-only'),
        ('btrfs-progs', '6.14-1', 'b/btrfs-progs', 'libbtrfs: LGPL-2.1-or-later; tools and other files retain their terms'),
    ]
    results, notice_sections = [], []
    for name, version, pool, license_name in components:
        directory = WORK / 'debian' / f'{name}-{version}'
        directory.mkdir(parents=True, exist_ok=True)
        base = f'https://deb.debian.org/debian/pool/main/{pool}/'
        dsc = directory / f'{name}_{version}.dsc'
        M['download'](base + dsc.name, dsc)
        text = dsc.read_text(encoding='utf-8')
        if not re.search(r'^Version: ' + re.escape(version) + '$', text, re.M):
            raise ValueError('Debian source version mismatch')
        section = re.search(r'^Checksums-Sha256:\n((?: .+\n)+)', text, re.M)
        if section is None:
            raise ValueError('Missing Debian source checksums')
        inputs = [{'file': dsc.name, 'sha256': M['digest'](dsc), 'origin': base + dsc.name}]
        for line in section[1].splitlines():
            checksum, size, filename = line.split()
            M['safe_name'](filename)
            target = directory / filename
            M['download'](base + filename, target, checksum)
            if target.stat().st_size != int(size):
                raise ValueError('Debian source length mismatch')
            inputs.append({'file': filename, 'sha256': checksum, 'origin': base + filename})
            if filename.endswith('.debian.tar.xz'):
                with tarfile.open(target) as archive:
                    notice_sections.append(f'{name} {version}\nDebian copyright file\n\n' + archive.extractfile('debian/copyright').read().decode())
        output = BUNDLE / f'debian-{name}-{version}.tar.gz'
        M['archive_directory'](directory, output)
        results.append(M['archive_record'](output, id=f'debian/{name}/{version}', version=version,
            license=license_name, inputs=inputs, provenance='compliance/evidence/oci-native-libraries.json', status='collected'))
        print('Collected exact Debian source:', name, version, flush=True)
    notice = ROOT / 'compliance/notices/oci-native-libraries.txt'
    notice.write_text('\n\n'.join(notice_sections), encoding='utf-8')
    # Retain the raw, public upstream evidence alongside the source inputs.
    evidence_dir = WORK / 'oci-build-evidence'
    evidence_dir.mkdir(exist_ok=True)
    for name in ('nerdctl-attestations.json', 'nerdctl-release-build.log', 'golang-image-index.json',
                 'golang-amd64-attestation-0.json', 'golang-amd64-attestation-1.json'):
        shutil.copyfile(WORK / name, evidence_dir / name)
    output = BUNDLE / 'oci-build-provenance.tar.gz'
    M['archive_directory'](evidence_dir, output)
    results.append(M['archive_record'](output, id='oci-build-provenance', status='collected'))
    M['write_json'](WORK / 'debian-sources.json', results)


if __name__ == '__main__':
    main()
