"""Read-only license audit checks; writes evidence beside this script only."""
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


evidence = read_json(ROOT / "compliance/evidence/application-dependencies.json")
indexed = {(x["ecosystem"], x["name"], x["version"]): x for x in evidence}
lock = read_json(ROOT / "package-lock.json")
npm_missing = []
npm_metadata_mismatch = []
dev_licenses = collections.Counter()
for relative, entry in lock["packages"].items():
    if not relative:
        continue
    if entry.get("dev"):
        dev_licenses[entry.get("license", "UNKNOWN")] += 1
        continue
    pkg = read_json(ROOT / relative / "package.json")
    key = ("npm", pkg["name"], entry["version"])
    if key not in indexed:
        npm_missing.append(key)
    elif (pkg["version"] != entry["version"] or
          indexed[key].get("integrity") != entry.get("integrity") or
          indexed[key]["license"] != pkg.get("license", entry.get("license"))):
        npm_metadata_mismatch.append(key)

cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
crate_map = {}
nonregistry = []
for relative in ["src-tauri/Cargo.lock", "cli/Cargo.lock", "runtime/cuda/host/Cargo.lock"]:
    for entry in tomllib.loads((ROOT / relative).read_text(encoding="utf-8"))["package"]:
        if entry.get("source", "").startswith("registry+"):
            crate_map[(entry["name"], entry["version"])] = entry
        else:
            nonregistry.append({"lock": relative, **entry})
cargo_missing, cargo_mismatch, crates_unavailable = [], [], []
deep_notices = []
crate_hashes = 0
for (name, version), entry in sorted(crate_map.items()):
    key = ("cargo", name, version)
    if key not in indexed:
        cargo_missing.append(key)
        continue
    rec = indexed[key]
    if rec.get("checksum") != entry.get("checksum"):
        cargo_mismatch.append({"crate": key, "reason": "checksum metadata"})
    cached = list((cargo_home / "registry/cache").glob(f"*/{name}-{version}.crate"))
    downloaded = ROOT / "build/compliance/crates" / f"{name}-{version}.crate"
    if not cached and downloaded.exists():
        cached = [downloaded]
    if cached:
        if sha(cached[0]) != entry["checksum"]:
            cargo_mismatch.append({"crate": key, "reason": "archive checksum"})
        crate_hashes += 1
        with tarfile.open(cached[0]) as archive:
            for member in archive:
                p = Path(member.name)
                relative = Path(*p.parts[1:])
                if member.isfile() and re.match(r"^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)(?:$|[._-])", p.name, re.I):
                    if len(relative.parts) > 3:
                        deep_notices.append({"crate": name, "version": version, "path": relative.as_posix()})
    else:
        crates_unavailable.append(key)

novnc_changes = []
with tarfile.open(ROOT / "build/compliance/bundle/novnc-1.7.0-used-source.tar.gz") as archive:
    archived = {}
    for entry in archive:
        if entry.isfile():
            archived[entry.name] = hashlib.sha256(archive.extractfile(entry).read()).hexdigest()
            file = ROOT / "node_modules/@novnc/novnc" / entry.name
            if not file.exists() or sha(file) != archived[entry.name]:
                novnc_changes.append(entry.name)
    current = {p.relative_to(ROOT / "node_modules/@novnc/novnc").as_posix()
               for p in (ROOT / "node_modules/@novnc/novnc").rglob("*") if p.is_file()}
    novnc_changes += sorted(current - archived.keys())

pe = read_json(ROOT / "compliance/evidence/windows-pe-imports.json")
new_graph = {Path(x["file"]).name.lower(): [n.lower() for n in x["imports"]] for x in pe}
def closure(name):
    visited = set()
    todo = [name.lower()]
    while todo:
        current = todo.pop()
        if current in visited:
            continue
        visited.add(current)
        todo.extend(new_graph.get(current, []))
    return sorted(visited)

report = {
    "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
    "trackedFiles": len(subprocess.check_output(["git", "ls-files"], cwd=ROOT).decode().splitlines()),
    "applicationEntries": dict(collections.Counter(x["ecosystem"] for x in evidence)),
    "npmProductionMissing": npm_missing,
    "npmMetadataMismatch": npm_metadata_mismatch,
    "npmDevelopmentLicenses": dict(dev_licenses),
    "cargoRegistryEntries": len(crate_map),
    "cargoMissing": cargo_missing,
    "cargoMismatch": cargo_mismatch,
    "cargoArchivesHashed": crate_hashes,
    "cargoArchivesUnavailable": crates_unavailable,
    "cargoNonregistry": nonregistry,
    "nestedCrateNoticesBeyondCollectorDepth": deep_notices,
    "novncArchivedFileCount": len(archived),
    "novncDifferencesFromArchivedSources": novnc_changes,
    "qemuTransitiveImports": closure("qemu-system-x86_64.exe"),
    "tpmWorkerTransitiveImports": closure("opendock-tpm-worker.exe"),
}
(OUT / "inventory-check.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps({k:v for k,v in report.items() if k not in ["cargoNonregistry", "nestedCrateNoticesBeyondCollectorDepth", "qemuTransitiveImports", "tpmWorkerTransitiveImports"]}, indent=2))
print("Nested notice candidates:", len(deep_notices))
