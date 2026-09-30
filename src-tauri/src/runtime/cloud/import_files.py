"""Receive a Yougori file drop over pinned SSH into this account's home folder.

The archive contains only ordinary files and directories. Extract through
directory descriptors into a private temporary folder, then publish it once
all bytes and counts have been verified. Never run an archive member as code.
"""
import base64
import hashlib
import json
import os
import pathlib
import shutil
import sys
import tarfile
import tempfile


class CheckedStream:
    def __init__(self, stream, maximum):
        self.stream = stream
        self.maximum = maximum
        self.count = 0
        self.digest = hashlib.sha256()

    def read(self, size=-1):
        block = self.stream.read(size)
        self.count += len(block)
        if self.count > self.maximum:
            raise ValueError('The archive exceeds its declared size')
        self.digest.update(block)
        return block


def receive(config):
    transfer = config['transferId']
    if len(transfer) != 32 or any(c not in '0123456789abcdef' for c in transfer):
        raise ValueError('Invalid transfer ID')
    expected_bytes = int(config['bytes'])
    expected_files = int(config['files'])
    archive_bytes = int(config['archiveBytes'])
    checksum = config['checksum']
    if not 0 <= expected_bytes <= 2**50 or not 0 <= expected_files <= 1000000 or not 0 < archive_bytes <= 2**50:
        raise ValueError('Invalid transfer size')
    if not isinstance(checksum, str) or len(checksum) != 64 or any(c not in '0123456789abcdef' for c in checksum):
        raise ValueError('Invalid archive checksum')
    home = pathlib.Path.home()
    if shutil.disk_usage(home).free < expected_bytes + 256 * 1024 * 1024:
        raise ValueError('Not enough free space on the cloud server for this copy while keeping 256 MB free')
    destination = home / ('yougori-import-' + transfer)
    if destination.exists() or destination.is_symlink():
        raise ValueError('This import destination already exists')
    staging = pathlib.Path(tempfile.mkdtemp(prefix='.yougori-import-', dir=home))
    os.chmod(staging, 0o700)
    root = os.open(staging, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    seen = set()
    files = 0
    copied = 0
    stream = CheckedStream(sys.stdin.buffer, archive_bytes)
    try:
        with tarfile.open(fileobj=stream, mode='r|') as archive:
            for member in archive:
                name = member.name.rstrip('/')
                parts = name.split('/')
                if (not name or len(name.encode('utf-8')) > 4096 or name.startswith('/')
                        or any(part in ('', '.', '..') or '\\' in part or ':' in part or '\x00' in part for part in parts)
                        or name in seen or len(seen) >= 1000000):
                    raise ValueError('Unsafe or repeated archive path')
                seen.add(name)
                if not (member.isfile() or member.isdir()):
                    raise ValueError('Only ordinary files and folders may be imported')
                parent = root
                opened = []
                try:
                    for part in parts[:-1]:
                        parent = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
                        opened.append(parent)
                    leaf = parts[-1]
                    if member.isdir():
                        if member.size != 0:
                            raise ValueError('Invalid directory in archive')
                        os.mkdir(leaf, 0o700, dir_fd=parent)
                    else:
                        if copied + member.size > expected_bytes or files >= expected_files:
                            raise ValueError('The archive exceeds its declared size')
                        source = archive.extractfile(member)
                        if source is None:
                            raise ValueError('Cannot read archive file')
                        output = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent)
                        try:
                            remaining = member.size
                            while remaining:
                                block = source.read(min(131072, remaining))
                                if not block:
                                    raise ValueError('The archive ended before its declared file size')
                                offset = 0
                                while offset < len(block):
                                    offset += os.write(output, block[offset:])
                                remaining -= len(block)
                            os.fchmod(output, 0o700 if member.mode & 0o100 else 0o600)
                            os.fsync(output)
                        finally:
                            os.close(output)
                        copied += member.size
                        files += 1
                finally:
                    for fd in reversed(opened):
                        os.close(fd)
        while stream.read(131072):
            pass
        if stream.count != archive_bytes or stream.digest.hexdigest() != checksum:
            raise ValueError('The archive was incomplete or changed during transfer')
        if files != expected_files or copied != expected_bytes:
            raise ValueError('The cloud server did not receive every selected file')
        if destination.exists() or destination.is_symlink():
            raise ValueError('This import destination appeared during upload')
        os.rename(staging, destination)
        return {'destination': str(destination), 'bytes': copied, 'files': files}
    finally:
        os.close(root)
        if staging.exists():
            shutil.rmtree(staging)


if __name__ == '__main__':
    try:
        configuration = json.loads(base64.b64decode(sys.argv[1], validate=True))
        result = receive(configuration)
        print('yougori-import-ready ' + json.dumps(result, separators=(',', ':')), flush=True)
    except Exception as error:
        print('Yougori cloud import failed: ' + str(error), file=sys.stderr, flush=True)
        sys.exit(1)
