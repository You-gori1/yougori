"""Real POSIX receiver tests; no SSH account or user's home is accessed."""
import hashlib
import io
import json
import pathlib
import sys
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE = pathlib.Path(__file__).resolve().parents[1] / 'src-tauri/src/runtime/cloud/import_files.py'
receiver = types.ModuleType('isolated_cloud_receiver')
exec(compile(SOURCE.read_text(), str(SOURCE), 'exec'), receiver.__dict__)

@unittest.skipUnless(sys.platform.startswith('linux'), 'receiver requires POSIX directory descriptors')
class ReceiverTests(unittest.TestCase):
    def archive(self, data=b'copied', symlink=False):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w') as archive:
            entry = tarfile.TarInfo('code.py')
            if symlink:
                entry.type = tarfile.SYMTYPE
                entry.linkname = '../keep'
                archive.addfile(entry)
            else:
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        return output.getvalue()

    def receive(self, home, archive, data_size=6, checksum=None):
        config = {'transferId': 'a'*32, 'bytes': data_size, 'files': 1,
                  'archiveBytes': len(archive), 'checksum': checksum or hashlib.sha256(archive).hexdigest()}
        output = io.StringIO()
        clock = iter(range(100000))
        with patch.object(receiver.pathlib.Path, 'home', return_value=home), \
             patch.object(receiver.sys, 'stdin', types.SimpleNamespace(buffer=io.BytesIO(archive))), \
             patch.object(receiver.sys, 'stdout', output), \
             patch.object(receiver.time, 'monotonic', side_effect=lambda: next(clock)):
            result = receiver.receive(config)
        return result, output.getvalue()

    def test_confirms_bytes_before_publishing_without_changing_existing_files(self):
        with tempfile.TemporaryDirectory() as directory:
            home = pathlib.Path(directory)
            (home/'keep').write_bytes(b'original')
            result, progress = self.receive(home, self.archive())
            self.assertEqual((pathlib.Path(result['destination'])/'code.py').read_bytes(), b'copied')
            self.assertEqual((home/'keep').read_bytes(), b'original')
            records = [json.loads(line.split(' ', 1)[1]) for line in progress.splitlines()]
            self.assertEqual(records[-1]['confirmedBytes'], 6)
            self.assertFalse(list(home.glob('.yougori-import-*')))

    def test_checksum_failure_removes_unpublished_staging_and_preserves_originals(self):
        with tempfile.TemporaryDirectory() as directory:
            home = pathlib.Path(directory)
            (home/'keep').write_bytes(b'original')
            with self.assertRaisesRegex(ValueError, 'incomplete or changed'):
                self.receive(home, self.archive(), checksum='0'*64)
            self.assertEqual(sorted(p.name for p in home.iterdir()), ['keep'])
            self.assertEqual((home/'keep').read_bytes(), b'original')

    def test_links_cannot_overwrite_existing_data(self):
        with tempfile.TemporaryDirectory() as directory:
            home = pathlib.Path(directory)
            (home/'keep').write_bytes(b'original')
            with self.assertRaisesRegex(ValueError, 'ordinary files'):
                self.receive(home, self.archive(symlink=True))
            self.assertEqual(sorted(p.name for p in home.iterdir()), ['keep'])

if __name__ == '__main__':
    unittest.main()
