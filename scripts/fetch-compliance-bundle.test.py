import hashlib
import io
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

fetch = runpy.run_path(str(Path(__file__).with_name('fetch-compliance-bundle.py')))['fetch']
PAYLOAD = b'verified source archive'
PUBLICATION = {'status': 'verified', 'bundleUrl': 'https://example.org/sources.zip',
               'bundleSha256': hashlib.sha256(PAYLOAD).hexdigest(), 'bundleBytes': len(PAYLOAD)}


class DownloadTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory()
        self.addCleanup(self.work.cleanup)
        self.destination = Path(self.work.name) / 'sources.zip'

    @patch('time.sleep')
    @patch('urllib.request.urlopen')
    def test_cached_504_retries_with_new_url_and_verifies(self, opening, sleeping):
        opening.side_effect = [urllib.error.HTTPError(PUBLICATION['bundleUrl'], 504, 'timeout', {}, None),
                               io.BytesIO(PAYLOAD)]
        fetch(PUBLICATION, self.destination)
        urls = [call.args[0].full_url for call in opening.call_args_list]
        self.assertNotEqual(urls[0], urls[1])
        self.assertEqual(self.destination.read_bytes(), PAYLOAD)
        self.assertEqual(list(Path(self.work.name).glob('*.part')), [])

    @patch('time.sleep')
    @patch('urllib.request.urlopen')
    def test_truncated_transfer_restarts_without_partial_bytes(self, opening, sleeping):
        opening.side_effect = [io.BytesIO(PAYLOAD[:4]), io.BytesIO(PAYLOAD)]
        fetch(PUBLICATION, self.destination)
        self.assertEqual(self.destination.read_bytes(), PAYLOAD)

    @patch('urllib.request.urlopen')
    def test_checksum_failure_preserves_existing_file(self, opening):
        self.destination.write_bytes(b'previous download')
        opening.return_value = io.BytesIO(b'x' * len(PAYLOAD))
        with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
            fetch(PUBLICATION, self.destination)
        self.assertEqual(self.destination.read_bytes(), b'previous download')
        self.assertEqual(list(Path(self.work.name).glob('*.part')), [])
        self.assertEqual(opening.call_count, 1)

    @patch('time.sleep')
    @patch('urllib.request.urlopen')
    def test_retry_limit_and_nonretryable_errors(self, opening, sleeping):
        for status, count in ((504, 3), (404, 1)):
            with self.subTest(status=status):
                opening.reset_mock()
                opening.side_effect = urllib.error.HTTPError(PUBLICATION['bundleUrl'], status, 'failed', {}, None)
                with self.assertRaises(urllib.error.HTTPError):
                    fetch(PUBLICATION, self.destination, attempts=3)
                self.assertEqual(opening.call_count, count)
                self.assertFalse(self.destination.exists())
                self.assertEqual(list(Path(self.work.name).glob('*.part')), [])

    @patch('urllib.request.urlopen')
    def test_unverified_publication_rejected_before_network(self, opening):
        for field, value in (('status', 'not-published'), ('bundleUrl', 'http://example.org/source.zip'),
                             ('bundleSha256', 'invalid'), ('bundleBytes', 0)):
            with self.subTest(field=field), self.assertRaises(ValueError):
                fetch({**PUBLICATION, field: value}, self.destination)
        opening.assert_not_called()


if __name__ == '__main__':
    unittest.main()
