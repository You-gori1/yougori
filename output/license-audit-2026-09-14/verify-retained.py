"""Verify retained firmware and the dated QEMU patch, writing only audit evidence."""
from pathlib import Path
import bz2
import hashlib
import json
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
release = json.loads((ROOT / "compliance/release.json").read_text(encoding="utf-8"))
source = next(x for x in release["components"] if x["id"] == "qemu-secure-src")
firmware = json.loads((ROOT / "compliance/evidence/firmware-provenance.json").read_text(encoding="utf-8"))
results = []
with tarfile.open(ROOT / "build/compliance/bundle" / source["file"]) as archive:
    names = set(archive.getnames())
    for item in firmware["files"]:
        name = "pc-bios/" + item["file"]
        compressed = name not in names and name + ".bz2" in names
        expected = archive.extractfile(name + ".bz2" if compressed else name).read()
        if compressed:
            expected = bz2.decompress(expected)
        actual = (ROOT / "src-tauri/resources/runtime/qemu/share" / item["file"]).read_bytes()
        results.append({"file": item["file"], "matchesArchivedQemuBytes": actual == expected,
                        "matchesRecordedHash": hashlib.sha256(actual).hexdigest() == item["sha256"]})
(OUT / "firmware-check.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
assert all(x["matchesArchivedQemuBytes"] and x["matchesRecordedHash"] for x in results)
print("Firmware and keymaps matched:", len(results))

script = ROOT / "scripts/compliance-qemu-notices.py"
original = script.read_text(encoding="utf-8")
old = '(ROOT / "compliance/evidence/qemu-modification-notices.json").write_text'
new = '(ROOT / "output/license-audit-2026-09-14/qemu-modification-notices.json").write_text'
assert original.count(old) == 1
audit_script = original.replace(old, new)
exec(compile(audit_script, str(script), "exec"), {"__file__": str(script), "__name__": "__main__"})
