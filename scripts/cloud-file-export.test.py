import importlib.util
import io
import json
from pathlib import Path
import os
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("export_files", Path(__file__).resolve().parents[1] / "src-tauri/src/runtime/cloud/export_files.py")
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)


class CloudFiles(unittest.TestCase):
    def settings(self, paths, maximum=10_000_000):
        return {"paths": [str(p) for p in paths], "operationId": "test-copy", "maximumBytes": maximum}

    def test_nested_hidden_binary_and_empty_folders_are_copied_without_following_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "project ü ' $(literal)"
            (source / "nested/empty").mkdir(parents=True)
            (source / ".private").write_bytes(b"private data")
            original = bytes(range(256)) * 2048
            (source / "nested/file.bin").write_bytes(original)
            (source / "linked").symlink_to("/etc/passwd")
            os.mkfifo(source / "pipe")
            stream = io.BytesIO()
            result = exporter.export(self.settings([source]), stream)
            self.assertEqual(result["files"], 2)
            self.assertEqual(result["skipped"], 2)
            self.assertEqual((source / "nested/file.bin").read_bytes(), original)
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                self.assertEqual(archive.extractfile(source.name + "/nested/file.bin").read(), original)
                self.assertTrue(archive.getmember(source.name + "/nested/empty").isdir())
                self.assertNotIn(source.name + "/linked", archive.getnames())
                self.assertEqual(json.load(archive.extractfile(".yougori-copy-complete.json")), result)

    def test_private_sources_do_not_become_readable_by_other_users(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "private"
            source.mkdir(mode=0o700)
            private = source / "private-key"
            private.write_bytes(b"fixture-private-content")
            private.chmod(0o600)
            public = source / "public-data"
            public.write_bytes(b"public")
            public.chmod(0o644)
            executable = source / "private-tool"
            executable.write_bytes(b"#!/bin/sh\n")
            executable.chmod(0o700)
            unsafe = source / "untrusted-bits"
            unsafe.write_bytes(b"untrusted")
            unsafe.chmod(0o6777)
            stream = io.BytesIO()
            exporter.export(self.settings([source]), stream)
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                for name, mode in [("private", 0o700), ("private/private-key", 0o600),
                                   ("private/public-data", 0o644), ("private/private-tool", 0o700),
                                   ("private/untrusted-bits", 0o755)]:
                    self.assertEqual(archive.getmember(name).mode, mode, name)
            self.assertEqual(private.stat().st_mode & 0o7777, 0o600)
            self.assertEqual(private.read_bytes(), b"fixture-private-content")

    def test_system_roots_size_overflow_and_colliding_roots_fail_without_receipt(self):
        for path in ["/", "/proc", "/sys", "/dev", "/run"]:
            with self.assertRaises(ValueError):
                exporter.export(self.settings([path]), io.BytesIO())
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "file"
            source.write_bytes(b"too much data")
            with self.assertRaises(ValueError):
                exporter.export(self.settings([source], maximum=2), io.BytesIO())
            with self.assertRaises(ValueError):
                exporter.export(self.settings([source, source]), io.BytesIO())

    def test_live_changes_abort_the_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "data"
            source.write_bytes(b"a" * 1_000_000)
            class ChangingStream(io.BytesIO):
                changed = False
                def write(self, data):
                    if not self.changed:
                        self.changed = True
                        with source.open("ab") as writer:
                            writer.write(b"changed")
                    return super().write(data)
            with self.assertRaisesRegex(ValueError, "source changed"):
                exporter.export(self.settings([source]), ChangingStream())


if __name__ == "__main__":
    unittest.main()
