"""Download the published source bundle with bounded retries and exact verification."""
import hashlib
import http.client
import json
from pathlib import Path
import re
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def fetch(publication, destination, attempts=5):
    url = publication.get('bundleUrl', '')
    checksum = publication.get('bundleSha256', '')
    size = publication.get('bundleBytes')
    if (publication.get('status') != 'verified'
            or urllib.parse.urlsplit(url).scheme != 'https'
            or not urllib.parse.urlsplit(url).hostname
            or not re.fullmatch(r'[a-f0-9]{64}', checksum)
            or type(size) is not int or size <= 0):
        raise ValueError('A verified source publication with an HTTPS URL, SHA-256 and byte length is required')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(attempts):
        # A fresh query prevents an intermediary from replaying a cached 504
        # or an expired release-asset redirect on every retry.
        parts = urllib.parse.urlsplit(url)
        query = urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
        query.append(('source_download', uuid.uuid4().hex))
        fresh_url = urllib.parse.urlunsplit(parts._replace(query=urllib.parse.urlencode(query)))
        request = urllib.request.Request(fresh_url, headers={
            'User-Agent': 'Yougori-source-verification', 'Accept-Encoding': 'identity',
            'Cache-Control': 'no-cache',
        })
        temporary = None
        try:
            print(f'Downloading source bundle (attempt {attempt + 1}/{attempts})', flush=True)
            digest, received = hashlib.sha256(), 0
            with tempfile.NamedTemporaryFile(dir=destination.parent, suffix='.part', delete=False) as output:
                temporary = Path(output.name)
                with urllib.request.urlopen(request, timeout=45) as response:
                    while chunk := response.read(1024 * 1024):
                        received += len(chunk)
                        if received > size:
                            raise ValueError('Source bundle exceeds its recorded byte length')
                        digest.update(chunk)
                        output.write(chunk)
            if received != size:
                raise http.client.IncompleteRead(b'', size - received)
            if digest.hexdigest() != checksum:
                raise ValueError('Source bundle SHA-256 mismatch')
            temporary.replace(destination)
            print(f'Source bundle verified: {received} bytes, SHA-256 {checksum}', flush=True)
            return destination
        except (urllib.error.URLError, http.client.IncompleteRead, TimeoutError, ConnectionError) as error:
            if isinstance(error, urllib.error.HTTPError) and error.code not in (408, 429, 500, 502, 503, 504):
                raise
            if attempt + 1 == attempts:
                raise
            print(f'Temporary download failure: {error}; retrying with a fresh URL.', flush=True)
            time.sleep(2 ** attempt)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    # This published bundle is an immutable cache of previously reviewed
    # upstream sources. The current checkout's publication is tracked
    # separately and must not be represented as verified by this download.
    cache = json.loads((root / 'compliance/source-cache.json').read_text(encoding='utf-8'))
    fetch(cache, root / 'build/compliance/corresponding-source.zip')
