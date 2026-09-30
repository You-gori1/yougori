"""Read COFF symbols from distributed binaries; correlate exact upstream license headers."""
from pathlib import Path
import hashlib
import json
import re
import struct
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent


def coff_symbols(path):
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    assert data[pe:pe + 4] == b"PE\0\0"
    pointer, count = struct.unpack_from("<II", data, pe + 12)
    strings = pointer + count * 18
    result, index = {}, 0
    if not pointer:
        return result
    while index < count:
        offset = pointer + index * 18
        name = data[offset:offset + 8]
        if name[:4] == b"\0" * 4:
            start = strings + struct.unpack_from("<I", name, 4)[0]
            end = data.find(b"\0", start)
            name = data[start:end]
        else:
            name = name.rstrip(b"\0")
        value, section, symbol_type, storage, aux = struct.unpack_from("<IhHBB", data, offset + 8)
        if section > 0 and storage == 2 and symbol_type == 0x20:
            result[name.decode("ascii", errors="replace")] = {"section": section, "value": value}
        index += 1 + aux
    return result


release = json.loads((ROOT / "compliance/release.json").read_text(encoding="utf-8"))
source = next(x for x in release["components"] if x["id"] == "qemu-secure-src")
files = []
with tarfile.open(ROOT / "build/compliance/bundle" / source["file"]) as archive:
    for member in archive:
        if not member.isfile() or not member.name.endswith(".c") or member.size > 1000000:
            continue
        contents = archive.extractfile(member).read().decode("utf-8", errors="replace")
        if not re.search(r"SPDX-License-Identifier:\s*GPL-2\.0-only(?:\s|\*/)", contents[:4000]):
            continue
        functions = sorted(set(re.findall(r"^\w[\w *]+\s+(\w+)\s*\([^;]*?\)\s*\{", contents, re.M)))
        literals = [{"text": m.group(1), "line": contents.count("\n", 0, m.start()) + 1}
                    for m in re.finditer(r'"([^"\n]{30,200})"', contents)
                    if "\\" not in m.group(1) and m.group(1).isascii()]
        files.append({"file": member.name, "header": contents[:1500], "functions": functions, "literals": literals,
                      "sourceSha256": hashlib.sha256(contents.encode()).hexdigest()})
records = []
for part in ("qemu", "qemu-secure"):
    relative = "src-tauri/resources/runtime/" + part + "/qemu-system-x86_64.exe"
    path = ROOT / relative
    symbols = coff_symbols(path)
    binary = path.read_bytes()
    records.append({"binary": relative, "binarySha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                    "definedExternalFunctionSymbols": len(symbols),
                    "gpl2OnlySourceMatches": [{**f, "matchedFunctions": [n for n in f["functions"] if n in symbols]}
                       for f in files if any(n in symbols for n in f["functions"])],
                    "gpl2OnlyLiteralMatches": [{"file": f["file"], "header": f["header"], "sourceSha256": f["sourceSha256"],
                        "matchedLiterals": [m for m in f["literals"] if m["text"].encode() in binary]}
                        for f in files if any(m["text"].encode() in binary for m in f["literals"])]})
result = {"sourceRevision": source["revision"], "sourceArchive": source["file"],
          "method": "Checked defined external COFF function symbols (none available in these executable payloads), then matched exact ASCII implementation literals to archived QEMU C files explicitly marked SPDX GPL-2.0-only. Literal matches corroborate incorporation and are not a linker map. Source headers preserved for review.",
          "binaries": records}
(OUT / "qemu-gpl2-only-symbols.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"binaries": [{"binary": r["binary"], "definedFunctions": r["definedExternalFunctionSymbols"],
                  "matches": [{"source": f["file"], "functions": f["matchedFunctions"]} for f in r["gpl2OnlySourceMatches"]],
                  "literalMatches": r["gpl2OnlyLiteralMatches"]} for r in records]}, indent=2))
