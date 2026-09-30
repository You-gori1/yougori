"""Verify QEMU's source notices against exported upstream and original build evidence.

Only restores the affected source files and applies patches in a temporary tree.
Does not compile or execute QEMU, or modify the original build cache.
"""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FUNCTIONAL = ("qemu-windows-tpm.patch", "qemu-whpx-tpm-ppi.patch", "qemu-whpx-reboot.patch")
NOTICE = "runtime/security/qemu-license-notices.patch"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def verify():
    release = json.loads((ROOT / "compliance/release.json").read_text(encoding="utf-8"))
    upstream = next(item for item in release["components"] if item["id"] == "qemu-secure-src")
    archive = ROOT / "build/compliance/bundle" / upstream["file"]
    with archive.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != upstream["sha256"]:
            raise ValueError("QEMU upstream archive differs from recorded source")
    baseline = json.loads((ROOT / "compliance/evidence/source-restoration.json").read_text(encoding="utf-8"))["results"]
    affected = set()
    for name in FUNCTIONAL:
        affected.update(re.findall(r"^diff --git a/(\S+) b/", (ROOT / "runtime/security" / name).read_text(), re.M))
    annotated = set(re.findall(r"^diff --git a/(\S+) b/", (ROOT / NOTICE).read_text(), re.M))
    if not affected or affected != annotated:
        raise ValueError("Dated notice patch must cover every modified upstream QEMU file")
    with tempfile.TemporaryDirectory(prefix="qemu-notice-check-", dir=ROOT / "build/compliance") as directory:
        tree = Path(directory)
        subprocess.run(["git", "init", "--quiet", str(tree)], check=True)
        with tarfile.open(archive) as source:
            for name in sorted(affected):
                member = source.getmember(name)
                if not member.isfile():
                    raise ValueError("Expected regular QEMU source file: " + name)
                source.extract(member, tree, filter="data")
        for name in FUNCTIONAL:
            subprocess.run(["git", "apply", str(ROOT / "runtime/security" / name)], cwd=tree, check=True)
        for name in ("tpm-api.h", "tpm-qemu.c"):
            # The shipped binaries predate a display-string rename. Supply the
            # exact original alongside current development code, never rehash
            # newer source and call it the source used by an older binary.
            source = ROOT / "runtime/security" / ("shipped/tpm-qemu.c" if name == "tpm-qemu.c" else name)
            (tree / "backends/tpm" / name).write_bytes(source.read_bytes())
        for item in baseline:
            if sha((tree / item["path"]).read_bytes()) != item["builtTreeSha256"]:
                raise ValueError("Functional QEMU source no longer matches the recorded build: " + item["path"])
        subprocess.run(["git", "apply", str(ROOT / NOTICE)], cwd=tree, check=True)
        results = []
        for item in baseline:
            data = (tree / item["path"]).read_bytes()
            original = data
            if item["path"] in affected:
                pattern = (rb"\A/\*\n \* Yougori modifications first recorded 2026-09-09:\n \* [^\n]+\n \* This dated notice was added 2026-09-13\.\n \*/\n"
                           if item["path"].endswith((".c", ".h")) else
                           rb"\A# Yougori modifications first recorded 2026-09-09:\n# [^\n]+\n# This dated notice was added 2026-09-13\.\n")
                original, count = re.subn(pattern, b"", data, count=1)
                if count != 1:
                    raise ValueError("Missing dated QEMU source notice: " + item["path"])
            if sha(original) != item["builtTreeSha256"]:
                raise ValueError("Notice patch changed functional source: " + item["path"])
            results.append({"path": item["path"], "annotated": item["path"] in affected,
                            "annotatedSha256": sha(data), "withoutNoticeSha256": sha(original),
                            "builtTreeSha256": item["builtTreeSha256"], "matches": True})
    result = {"date": "2026-09-13", "upstreamRevision": upstream["revision"],
              "patch": {"path": NOTICE, "sha256": sha((ROOT / NOTICE).read_bytes())},
              "method": "Restored affected files from checksum-verified upstream archive; applied original patches and notice patch. Removing only the exact new comment headers reproduces all 12 recorded build-source hashes.",
              "originalBuildRecordsPreserved": True, "results": results}
    (ROOT / "compliance/evidence/qemu-modification-notices.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"Verified {len(affected)} dated QEMU notices; all {len(results)} functional source files match original build evidence.")


if __name__ == "__main__":
    verify()
