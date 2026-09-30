"""Restore recorded runtime source trees into a NEW directory, without Git/network.

Python 3.12+ is required for safe tar extraction. Unix preserves upstream symlinks;
Windows may need Developer Mode for archives which contain symbolic links.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile


def safe_path(root, relative):
    if not isinstance(relative, str) or "\\" in relative or ":" in relative or "\0" in relative:
        raise ValueError("Unsafe source path")
    if relative.startswith("/") or any(part in ("", ".", "..") for part in relative.split("/")):
        raise ValueError("Unsafe source path")
    result = root / relative
    result.resolve().relative_to(root.resolve())
    return result


def restore(bundle, destination):
    index = json.loads((bundle / "SOURCE_INDEX.json").read_text(encoding="utf-8"))
    if destination.exists():
        raise ValueError("Use a new destination. Existing source files are never overwritten.")
    roots = {"qemu-secure-src", "ms-tpm-20-ref", "edk2-secure-src", "secureboot-objects"}
    selected = [item for item in index["components"] if item["id"] in roots | {"local-build-material"} or item.get("parent") in roots]
    if not roots.issubset({item["id"] for item in selected}):
        raise ValueError("Source index lacks the required pinned source trees")
    for item in selected:
        path = safe_path(bundle, item["file"])
        with path.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != item["sha256"]:
                raise ValueError("Source archive checksum mismatch: " + item["file"])
    destination.mkdir(parents=True)
    omitted_links = []
    # Restore parents before nested submodules. All archive members also pass
    # Python's data filter, rejecting traversal, device files and escaping links.
    for item in sorted(selected, key=lambda item: item.get("mountAt", "").count("/") + (1 if "parent" in item else 0)):
        if item["id"] == "local-build-material":
            target = destination
        else:
            target = destination / "build/runtime-cache" / item.get("parent", item["id"])
            if "mountAt" in item:
                target = safe_path(target, item["mountAt"])
        target.mkdir(parents=True, exist_ok=True)
        def source_filter(member, directory):
            # This upstream macOS emulator SDK link is unused by the x86 OVMF
            # build. Preserve it in the untouched source archive and receipt,
            # but never create an absolute host link during extraction.
            if (member.issym() and member.name == "EmulatorPkg/Unix/Host/X11IncludeHack"
                    and member.linkname == "/opt/X11/include"):
                omitted_links.append({"component": item["id"], "path": member.name, "target": member.linkname})
                return None
            return tarfile.data_filter(member, directory)
        with tarfile.open(safe_path(bundle, item["file"])) as archive:
            archive.extractall(target, filter=source_filter)
        print("Restored:", item["id"], flush=True)
    (destination / "SOURCE_RECEIPT.json").write_text(json.dumps({"components": selected, "omittedHostLinks": omitted_links}, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    restore(args.bundle.resolve(), args.destination.resolve())
