import base64
import hashlib
import io
import json
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
import uuid


SCRIPT = pathlib.Path(__file__).with_name('import_files.py')


def make_archive(entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w') as archive:
        for name, contents, kind in entries:
            info = tarfile.TarInfo(name)
            info.type = kind
            info.size = len(contents)
            archive.addfile(info, io.BytesIO(contents))
    return stream.getvalue()


class CloudImportTests(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)

    def upload(self, payload, count, size, *, checksum=None):
        transfer = uuid.uuid4().hex
        config = {'transferId': transfer, 'archiveBytes': len(payload),
                  'bytes': size, 'files': count,
                  'checksum': checksum or hashlib.sha256(payload).hexdigest()}
        env = dict(os.environ, HOME=self.home.name)
        result = subprocess.run([sys.executable, str(SCRIPT), base64.b64encode(json.dumps(config).encode()).decode()],
                                input=payload, capture_output=True, env=env, timeout=15)
        return result, pathlib.Path(self.home.name) / ('yougori-import-' + transfer)

    def test_nested_binary_hidden_and_empty_folders_are_copied_without_source_mutation(self):
        content = bytes(range(256)) * 600
        archive = make_archive([
            ('Project', b'', tarfile.DIRTYPE),
            ('Project/nested', b'', tarfile.DIRTYPE),
            ('Project/nested/empty', b'', tarfile.DIRTYPE),
            ('Project/nested/data.bin', content, tarfile.REGTYPE),
            ('Project/.hidden', b'keep', tarfile.REGTYPE),
        ])
        result, destination = self.upload(archive, 2, len(content) + 4)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads(result.stdout.decode().split('yougori-import-ready ', 1)[1])
        self.assertEqual(receipt['destination'], str(destination))
        self.assertEqual((destination / 'Project/nested/data.bin').read_bytes(), content)
        self.assertEqual((destination / 'Project/.hidden').read_bytes(), b'keep')
        self.assertTrue((destination / 'Project/nested/empty').is_dir())

    def test_links_traversal_and_corrupt_streams_never_publish_a_folder(self):
        unsafe = [
            make_archive([('link', b'outside', tarfile.SYMTYPE)]),
            make_archive([('../escape', b'bad', tarfile.REGTYPE)]),
            make_archive([('same', b'a', tarfile.REGTYPE), ('same', b'b', tarfile.REGTYPE)]),
        ]
        for archive in unsafe:
            with self.subTest(archive=archive[:32]):
                result, destination = self.upload(archive, 1, 3)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(destination.exists())
        valid = make_archive([('safe', b'hello', tarfile.REGTYPE)])
        result, destination = self.upload(valid, 1, 5, checksum='0' * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(destination.exists())


if __name__ == '__main__':
    unittest.main()
