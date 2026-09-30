"""Regression tests for actual source omissions, including rehashed archives."""
import copy
import io
import json
from pathlib import Path
import runpy
import subprocess
import tarfile
import tempfile
import unittest

M = runpy.run_path(str(Path(__file__).with_name('compliance-application.py')))


class ApplicationSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='yougori-application-source-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / 'build/compliance/bundle'
        for name in M['REQUIRED']:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fixture source\n')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        self.git('add', '.')
        self.record = M['collect'](self.root, self.bundle)
        self.report = {'components': [self.record], 'archives': [
            {k: self.record[k] for k in ('file', 'sha256', 'bytes')}]}

    def git(self, *args):
        subprocess.run(['git', '-C', str(self.root), *args], check=True, capture_output=True)

    def check(self):
        M['check'](self.root, self.bundle, self.report)

    def test_complete_archive_and_cross_platform_newlines(self):
        self.check()
        before = copy.deepcopy(self.record)
        (self.root / 'cli/src/main.rs').write_bytes(b'fixture source\r\n')
        self.assertEqual(M['collect'](self.root, self.bundle), before)
        self.check()

    def test_backend_change_rejected(self):
        (self.root / 'src-tauri/src/lib.rs').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'stale or incomplete'):
            self.check()

    def test_missing_required_cli_even_after_git_removal(self):
        self.git('rm', '-f', 'cli/src/main.rs')
        with self.assertRaisesRegex(ValueError, 'Required application source missing'):
            self.check()

    def test_new_tracked_source_requires_collection(self):
        (self.root / 'cli/src/new.rs').write_text('new module')
        self.git('add', 'cli/src/new.rs')
        with self.assertRaisesRegex(ValueError, 'stale or incomplete'):
            self.check()

    def test_new_untracked_production_source_is_not_silently_omitted(self):
        (self.root / 'cli/src/new.rs').write_text('new module')
        with self.assertRaisesRegex(ValueError, 'Stage new source'):
            self.check()

    def test_untracked_brand_asset_is_not_omitted_from_source_delivery(self):
        (self.root / 'new-logo.png').write_bytes(b'brand asset')
        with self.assertRaisesRegex(ValueError, 'Stage new source'):
            self.check()

    def test_untracked_vault_and_skill_sources_are_not_silently_omitted(self):
        for name in ('vault/src/new.rs', 'engine/src/new.rs', 'skills/yougori/references/new.txt'):
            with self.subTest(name=name):
                path = self.root / name
                path.write_text('new production source', encoding='utf-8')
                try:
                    with self.assertRaisesRegex(ValueError, 'Stage new source'):
                        self.check()
                finally:
                    path.unlink()

    def test_required_vault_and_embedded_skills_survive_source_packaging(self):
        for name in ('vault/src/lib.rs', 'engine/src/main.rs', 'skills/yougori/SKILL.txt'):
            with self.subTest(name=name):
                self.git('rm', '--cached', name)
                try:
                    with self.assertRaisesRegex(ValueError, 'Required application source missing'):
                        self.check()
                finally:
                    self.git('add', name)

    def test_updated_upstream_archives_are_separate_from_application_source(self):
        cache = self.root / 'compliance/source-cache.json'
        update = self.root / 'compliance/source-updates/alpine-example.tar.gz'
        update.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text('{"status":"verified"}\n', encoding='utf-8')
        update.write_bytes(b'upstream source archive')
        self.git('add', 'compliance/source-cache.json', 'compliance/source-updates/alpine-example.tar.gz')
        paths = {item['path'] for item in M['inventory'](self.root)['files']}
        self.assertIn('compliance/source-cache.json', paths)
        self.assertNotIn('compliance/source-updates/alpine-example.tar.gz', paths)

    def test_omitted_backend_rejected_even_after_rehashing_archive(self):
        path = self.bundle / M['ARCHIVE']
        with tarfile.open(path) as source:
            entries = [(m, source.extractfile(m).read()) for m in source if m.name != 'src-tauri/src/lib.rs']
        with tarfile.open(path, 'w:gz') as archive:
            for member, data in entries:
                archive.addfile(member, io.BytesIO(data))
        for item in (self.record, self.report['archives'][0]):
            item.update(sha256=M['sha'](path.read_bytes()), bytes=path.stat().st_size)
        with self.assertRaisesRegex(ValueError, 'omits source files'):
            self.check()

    def test_missing_application_archive_record_rejected(self):
        self.report['components'] = []
        with self.assertRaisesRegex(ValueError, 'Missing reviewed Yougori'):
            self.check()


if __name__ == '__main__':
    unittest.main()
