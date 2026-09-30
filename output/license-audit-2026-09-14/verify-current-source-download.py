"""Verify anonymous public source delivery against the current local index."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[2]
TAG = "staging-sources-9b13340292f535bc"
NAME = "Yougori-corresponding-source-9b13340292f535bc.zip"
EXPECTED = "3d47cccfa14bf7b17a0b5d649f82aa4ac9ab94d45c44ee1e59352089fcbcc3a2"
BASE = "https://github.com/Yougori/yougori/releases/download/" + TAG + "/"


def get(url):
    # No GitHub token, cookies, credential helper or Authorization header.
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0", "Cache-Control": "no-cache"})
    return urllib.request.urlopen(request, timeout=90)


def main():
    local = ROOT / "build/compliance/bundle"
    with get(BASE + "SOURCE_INDEX.json") as response:
        index = response.read()
    if index != (local / "SOURCE_INDEX.json").read_bytes():
        raise RuntimeError("Public source index differs from the current reviewed index")
    with get(BASE + "SHA256SUMS") as response:
        if response.read() != (local / "SHA256SUMS").read_bytes():
            raise RuntimeError("Public archive checksum list differs")
    downloaded = ROOT / "build/angle-runtime/public-source.zip"
    checksum, count = hashlib.sha256(), 0
    with get(BASE + NAME) as response, downloaded.open("xb") as output:
        while chunk := response.read(4 * 1024 * 1024):
            output.write(chunk)
            checksum.update(chunk)
            count += len(chunk)
    if checksum.hexdigest() != EXPECTED:
        raise RuntimeError("Anonymous ZIP download checksum mismatch")
    if count != (ROOT / "build/compliance/dist" / NAME).stat().st_size:
        raise RuntimeError("Anonymous ZIP download length mismatch")
    with zipfile.ZipFile(downloaded) as archive:
        if archive.read("bundle/SOURCE_INDEX.json") != index or archive.testzip() is not None:
            raise RuntimeError("Published ZIP/index integrity failure")
    publication = {
        "status": "verified", "manifestUrl": BASE + "SOURCE_INDEX.json",
        "manifestSha256": hashlib.sha256(index).hexdigest(), "verifiedAt": datetime.now(timezone.utc).isoformat(),
        "evidence": "Anonymous HTTPS download of the complete ZIP matched its recorded SHA-256 and byte length; public and embedded source indexes, archive checksum list and ZIP CRCs matched. Reproducible check: output/license-audit-2026-09-14/verify-current-source-download.py.",
        "releaseUrl": "https://github.com/Yougori/yougori/releases/tag/" + TAG, "sourceTag": TAG,
        "sourceCommit": "46bc2677e0b4e5a85da3122750c760577d6bdc44", "bundleUrl": BASE + NAME,
        "bundleSha256": EXPECTED, "bundleBytes": count,
        "scope": "Matching current staging sources; no application installer publication or main promotion.",
    }
    report_path = ROOT / "compliance/release.json"
    report = json.loads(report_path.read_text(encoding="utf-8"))
    report["publication"] = publication
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    Path(__file__).with_name("source-publication.json").write_text(json.dumps(publication, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"Anonymous source delivery verified: {count} bytes, SHA-256 {EXPECTED}.")


if __name__ == "__main__":
    main()
