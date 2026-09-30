"""Inspect exact retained ANGLE source inputs and shipped DLLs without execution."""
from pathlib import Path
import hashlib
import json
import re
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
BASE = ROOT / "build/compliance/msys/mingw-w64-angleproject/2.1.r25748.890b5d8f-6"
RAW = ROOT / "build/compliance/audit-2026-09-14/angle"
RAW.mkdir(parents=True, exist_ok=True)
resource_texts = []
for path in (ROOT / "src-tauri/resources").rglob("*"):
    if path.is_file() and (path.suffix.lower() in (".txt", ".md", ".rst") or
                         re.search(r"licen[cs]e|copying|copyright|notice", path.name, re.I)):
        resource_texts.append((path.relative_to(ROOT).as_posix(), path.read_text(encoding="utf-8", errors="replace")))

def normalized(text):
    return re.sub(r"\s+", " ", text).strip()

retained = "\n".join(text for _, text in resource_texts)
normalized_retained = normalized(retained)
records, source_matches = [], []
inputs = {
    "angleproject.tar": ["src/third_party/volk/LICENSE.md", "src/common/third_party/xxhash/LICENSE", "src/libANGLE/overlay/LICENSE.txt"],
    "bare-clones_vulkan_memory_allocator.tar": ["LICENSE.txt"],
    "bare-clones_spirv-headers.tar": ["LICENSE"],
    "bare-clones_spirv-tools.tar": ["LICENSE"],
    "bare-clones_vulkan-loader.tar": ["LICENSE.txt", "LICENSES/Apache-2.0.txt"],
}
for filename, paths in inputs.items():
    with tarfile.open(BASE / "distfiles" / filename) as archive:
        names = set(archive.getnames())
        for path in paths:
            if path not in names:
                continue
            data = archive.extractfile(path).read()
            contents = data.decode("utf-8", errors="replace")
            destination = filename.removesuffix(".tar") + "--" + path.replace("/", "_")
            (RAW / destination).write_bytes(data)
            records.append({"inputArchive": filename, "file": path, "sha256": hashlib.sha256(data).hexdigest(),
                            "textPresentInResourcesIgnoringWhitespace": normalized(contents) in normalized_retained,
                            "textExcerpt": contents[:1800]})
        if filename == "angleproject.tar":
            for member in archive:
                if member.isfile() and member.name.endswith((".gn", ".gni", ".cpp", ".h")) and member.size < 300000:
                    content = archive.extractfile(member).read().decode("utf-8", errors="replace")
                    matches = [{"line": i + 1, "text": line} for i, line in enumerate(content.splitlines())
                               if re.search(r"VMA_IMPLEMENTATION|vulkan_memory_allocator|volkInitialize|XXH64\(|angle_enable_vulkan\s*=|spirv-tools.*(opt|src)", line)]
                    if matches:
                        source_matches.append({"file": member.name, "matches": matches[:15]})
                        if member.name in ("BUILD.gn", "gni/angle.gni"):
                            (RAW / member.name.replace("/", "_")).write_text(content, encoding="utf-8")
binary_records = []
for name in ("qemu/libGLESv2.dll", "qemu-secure/libGLESv2.dll"):
    path = ROOT / "src-tauri/resources/runtime" / name
    data = path.read_bytes()
    strings = re.findall(rb"[ -~]{8,}", data)
    hits = [s.decode() for s in strings if re.search(rb"Vma[A-Z]|vma[A-Z]|vk_mem_alloc|spvOptimizer|SpirvTools|volkInitialize|XXH64|SpirvValidate", s)]
    binary_records.append({"file": path.relative_to(ROOT).as_posix(), "sha256": hashlib.sha256(data).hexdigest(),
                           "matchedStrings": hits[:160], "matchCount": len(hits)})
result = {"noticeComparisons": records, "sourceReferences": source_matches, "binaryReferences": binary_records,
          "attributionSearch": {term: [name for name, text in resource_texts if term.lower() in text.lower()]
             for term in ["VulkanMemoryAllocator", "Advanced Micro Devices", "2017-2025 Advanced", "Arseny Kapoulkine", "Khronos Group Inc."]}}
(OUT / "angle-review.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"notices": records, "binaries": binary_records, "attributionSearch": result["attributionSearch"]}, indent=2))
