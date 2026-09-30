"""Read selected Linux trees into a stream; never modify the source."""
import base64
import io
import json
import os
import stat
import sys
import tarfile


def export(config, output):
    total = files = skipped = entries = 0
    roots = []
    names = set()
    for value in config["paths"]:
        path = os.path.abspath(os.path.expanduser(value))
        resolved = os.path.realpath(path)
        if resolved == "/" or any(resolved == p or resolved.startswith(p + "/") for p in ("/proc", "/sys", "/dev", "/run")):
            raise ValueError("Select data folders, not / or a virtual system directory")
        name = os.path.basename(path)
        if name in names or not name:
            raise ValueError("Selected files/folders must have different final names")
        names.add(name)
        roots.append((path, name))

    with tarfile.open(fileobj=output, mode="w|", format=tarfile.PAX_FORMAT) as archive:
        def visit(parent, name, relative, device, depth=0):
            nonlocal total, files, skipped, entries
            if depth > 128 or len(relative.encode("utf-8")) > 4096:
                raise ValueError("A selected path is too deeply nested")
            before = os.stat(name, dir_fd=parent, follow_symlinks=False)
            if not (stat.S_ISREG(before.st_mode) or stat.S_ISDIR(before.st_mode)) or (device is not None and before.st_dev != device):
                skipped += 1
                return
            fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            try:
                actual = os.fstat(fd)
                if (actual.st_dev, actual.st_ino) != (before.st_dev, before.st_ino):
                    raise ValueError("A source changed during the copy; pause writers and retry")
                entries += 1
                if entries > 1000000:
                    raise ValueError("Select fewer than one million entries per copy")
                header = tarfile.TarInfo(relative)
                # The importing user owns the copy. Preserve confidentiality:
                # never grant group/other access that the source did not have.
                # Special bits and group/other write permissions are not copied.
                header.mode = (0o700 | (actual.st_mode & 0o055)) if stat.S_ISDIR(actual.st_mode) else (0o600 | (actual.st_mode & 0o155))
                header.mtime = int(actual.st_mtime)
                if stat.S_ISDIR(actual.st_mode):
                    header.type = tarfile.DIRTYPE
                    archive.addfile(header)
                    with os.scandir(fd) as children:
                        for child in children:
                            visit(fd, child.name, relative + "/" + child.name, actual.st_dev, depth + 1)
                else:
                    header.size = actual.st_size
                    total += header.size
                    if total > config["maximumBytes"]:
                        raise ValueError("Selected files exceed the destination storage allowance")
                    with os.fdopen(os.dup(fd), "rb") as source:
                        archive.addfile(header, source)
                    files += 1
                after = os.fstat(fd)
                if (actual.st_size, actual.st_mtime_ns, actual.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                    raise ValueError("A source changed during the copy; pause writers and retry")
            finally:
                os.close(fd)

        for path, name in roots:
            visit(None, path, name, None)
        if not entries:
            raise ValueError("No ordinary files or folders could be copied")
        receipt = {"operationId": config["operationId"], "bytes": total, "files": files, "skipped": skipped}
        data = json.dumps(receipt, sort_keys=True).encode("utf-8")
        marker = tarfile.TarInfo(".yougori-copy-complete.json")
        marker.mode = 0o600
        marker.size = len(data)
        archive.addfile(marker, io.BytesIO(data))
    return receipt


if __name__ == "__main__":
    try:
        settings = json.loads(base64.b64decode(sys.argv[1]))
        summary = export(settings, sys.stdout.buffer)
        print("YOUGORI_FILE_COPY=" + json.dumps(summary), file=sys.stderr)
    except Exception as error:
        print("File copy failed: " + str(error), file=sys.stderr)
        sys.exit(1)
