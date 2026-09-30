"""Cross-check native DLL strings against exact retained source code, without execution."""
from pathlib import Path
import hashlib
import json
import re
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
BASE = ROOT / "build/compliance/msys/mingw-w64-angleproject/2.1.r25748.890b5d8f-6/distfiles"
binary = (ROOT / "src-tauri/resources/runtime/qemu/libGLESv2.dll").read_bytes()
matches = []
for filename, roots in [
    ("bare-clones_spirv-tools.tar", ("source/",)),
    ("bare-clones_vulkan_memory_allocator.tar", ("include/", "src/")),
    ("bare-clones_vulkan-loader.tar", ("loader/",)),
    ("angleproject.tar", ("src/third_party/volk/", "src/common/third_party/xxhash/", "src/libANGLE/renderer/vulkan/", "src/compiler/translator/spirv/")),
]:
    hits = []
    with tarfile.open(BASE / filename) as archive:
        for member in archive:
            if not member.isfile() or not member.name.startswith(roots) or not member.name.endswith((".h", ".hpp", ".c", ".cpp", ".cc")) or member.size > 2000000:
                continue
            content = archive.extractfile(member).read().decode("utf-8", errors="replace")
            for match in re.finditer(r'"([^"\n]{35,250})"', content):
                literal = match.group(1)
                if "\\" in literal or not literal.isascii() or literal.encode() not in binary:
                    continue
                if literal.startswith(("//", "http", "../")):
                    continue
                hits.append({"file": member.name, "line": content.count("\n", 0, match.start()) + 1,
                             "literal": literal, "binaryOffset": binary.index(literal.encode())})
    unique = {(x["file"], x["literal"]): x for x in hits}
    matches.append({"sourceArchive": filename, "matchedLiteralCount": len(unique), "matches": list(unique.values())[:80]})
result = {"binary": "src-tauri/resources/runtime/qemu/libGLESv2.dll", "sha256": hashlib.sha256(binary).hexdigest(),
          "method": "Exact ASCII literals from non-test implementation files in retained source archives found in shipped DLL; corroborative incorporation evidence, not a linker map", "sourceMatches": matches}
(OUT / "native-code-traces.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"sources": [{**x, "matches": x["matches"][:8]} for x in matches]}, indent=2))
