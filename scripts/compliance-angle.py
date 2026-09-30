#!/usr/bin/env python3
"""Record the actual ANGLE target closure, compiler inputs and embedded notices."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess

ROOT = Path(__file__).resolve().parent.parent
FORBIDDEN = ("third_party/spirv-", "third_party/vulkan", "third_party/SwiftShader/",
             "third_party/astc", "third_party/dawn/", "src/third_party/volk/",
             "src/libANGLE/renderer/vulkan/", "src/libANGLE/renderer/wgpu/",
             "src/compiler/translator/spirv/TranslatorSPIRV.cpp")
INTERFACE_HEADERS = {"include/EGL/egl.h", "include/EGL/eglext.h", "include/EGL/eglplatform.h",
                     "include/GLES/glplatform.h", "include/GLES2/gl2platform.h", "include/GLES3/gl3platform.h"}
FONT = "src/libANGLE/Overlay_font_autogen.cpp"
PARSERS = {"src/compiler/translator/glslang_tab_autogen.cpp", "src/compiler/translator/glslang_tab_autogen.h",
           "src/compiler/preprocessor/preprocessor_tab_autogen.cpp"}


def sha(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def text_sha(path):
    return hashlib.sha256(path.read_text(encoding="utf-8").encode()).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8", newline="\n")


def closure(graph, roots):
    seen = set()
    def visit(name):
        if name in seen:
            return
        seen.add(name)
        for child in graph[name].get("deps", []):
            visit(child)
    for name in roots:
        visit(name)
    return sorted(seen)


def component_for(name, contents):
    if name.startswith(FORBIDDEN):
        raise ValueError("Excluded ANGLE implementation entered the build: " + name)
    if name in INTERFACE_HEADERS:
        # Do not extend this exception to implementation-bearing headers.
        uncommented = re.sub(r"/\*.*?\*/|//[^\n]*", "", contents, flags=re.S)
        if re.search(r"\b(?:inline|static|class)\b", uncommented) or re.search(r"\)\s*\{", uncommented):
            raise ValueError("Interface header now contains implementation; review required: " + name)
        return "khronos-apache-interfaces"
    if name in PARSERS:
        if "As a special exception" not in contents[:3000] or "under terms of your choice" not in contents[:3000]:
            raise ValueError("Missing Bison output exception: " + name)
        return "bison-output"
    if name == FONT:
        return "angle-font-disabled-wrapper"
    if re.search(r"Apache License|Apache-2\.0|GNU (?:Lesser )?General Public License|SPDX-License-Identifier:\s*(?:GPL|AGPL|LGPL)", contents[:4000], re.I):
        raise ValueError("Unreviewed native license declaration: " + name)
    if name.startswith("src/common/third_party/xxhash/"):
        return "xxhash"
    if name.startswith("src/common/base/anglebase/"):
        return "chromium-base"
    if name.startswith("third_party/zlib/google/"):
        return "chromium-compression"
    if name.endswith(("preprocessor_lex_autogen.cpp", "glslang_lex_autogen.cpp")):
        return "flex-output"
    if name.startswith("include/") and "The Khronos Group" in contents[:4000]:
        return "khronos-mit-interfaces"
    if name.startswith(("src/", "include/")):
        return "angle"
    if name.startswith("out/Yougori-D3D11/gen/angle/"):
        return "angle-generated"
    raise ValueError("Unreviewed ANGLE input path: " + name)


def inspect(work, msys):
    angle = work / "angle"
    out = angle / "out/Yougori-D3D11"
    args = out / "args.gn"
    if text_sha(args) != text_sha(ROOT / "runtime/gpu/angle-d3d11.gn"):
        raise ValueError("ANGLE build arguments differ from the reviewed profile")
    graph = json.loads((work / "gn-targets.json").read_text(encoding="utf-8"))
    targets = closure(graph, ["//:libEGL", "//:libGLESv2"])
    commands = (work / "target-commands.txt").read_text(encoding="utf-8")
    if re.search(r"-DANGLE_ENABLE_(?:VULKAN|OVERLAY|WGPU|METAL|D3D9)(?:\s|=1\b)", commands):
        raise ValueError("Excluded backend enabled in compiler command")
    deps = (work / "header-dependencies.txt").read_text(encoding="utf-8")
    if "(STALE)" in deps or "(VALID)" not in deps:
        raise ValueError("Compiler dependency evidence is missing or stale")
    input_paths = {(out / line.strip()).resolve() for line in deps.splitlines() if line.startswith("    ")}
    # GN lists unused headers too. Compiler dependency files establish header
    # inclusion; add only compilation/resource/linker units from reachable targets.
    for target in targets:
        for name in graph[target].get("sources", []):
            path = angle / name.removeprefix("//")
            if path.suffix in (".cpp", ".cc", ".c", ".rc", ".def", ".s") and path.is_file():
                input_paths.add(path.resolve())
    inputs = []
    for path in sorted(input_paths):
        if path.is_relative_to(angle):
            name = path.relative_to(angle).as_posix()
            component = component_for(name, path.read_text(encoding="utf-8", errors="replace"))
            inputs.append({"path": name, "sha256": sha(path), "component": component})
        elif path.is_relative_to(msys):
            inputs.append({"path": "<MSYS>/" + path.relative_to(msys).as_posix(), "sha256": sha(path), "component": "toolchain-headers"})
        else:
            raise ValueError("Compiler input outside the recorded source/toolchain trees: " + str(path))
    font_command = next(line for line in commands.splitlines() if " -c ../../" + FONT + " " in line)
    command = shlex.split(font_command)
    command[0] = str(msys / "ucrt64/bin/g++.exe")
    filtered = []
    skip = False
    for item in command:
        if skip:
            skip = False
        elif item in ("-MF", "-o"):
            skip = True
        elif item == "-MD":
            continue
        else:
            filtered.append("-E" if item == "-c" else item)
    env = os.environ.copy()
    env["PATH"] = str(msys / "ucrt64/bin") + os.pathsep + env["PATH"]
    preprocessed = subprocess.check_output(filtered + ["-P"], cwd=out, env=env)
    if b"kFontData" in preprocessed or not re.search(rb"OverlayState::getFontData\(\) const\s*\{\s*return nullptr;\s*\}", preprocessed):
        raise ValueError("Apache Roboto glyph data was not excluded by preprocessing")
    (work / "font-preprocessed.cpp").write_bytes(preprocessed)
    normalized_commands = commands.replace(angle.as_posix(), "<ANGLE>").replace(msys.as_posix(), "<MSYS>")
    (work / "angle-commands.txt").write_text(normalized_commands, encoding="utf-8", newline="\n")
    closure_record = [{"target": target, "type": graph[target]["type"], "deps": graph[target].get("deps", []),
                       "sources": graph[target].get("sources", []), "libs": graph[target].get("libs", [])} for target in targets]
    write_json(work / "angle-targets.json", closure_record)
    notices = ROOT / "compliance/notices/angle-third-party.txt"
    package = subprocess.check_output([str(msys / "usr/bin/pacman.exe"), "-Q"], env=env).decode()
    package_names = ("gcc", "gcc-libs", "binutils", "crt", "headers", "winpthreads", "libwinpthread", "zlib", "gn", "ninja", "python", "pkgconf")
    record = {
        "schemaVersion": 1, "profile": "angle-d3d11-only", "revision": "890b5d8fa2988e3719e0d80421bf3e927db9cd5c",
        "sourceComponent": "msys/mingw-w64-ucrt-x86_64-angleproject/2.1.r25748.890b5d8f-6",
        "buildScript": {"path": "scripts/build-angle-runtime.py", "sha256": text_sha(ROOT / "scripts/build-angle-runtime.py")},
        "inspector": {"path": "scripts/compliance-angle.py", "sha256": text_sha(Path(__file__))},
        "arguments": {"path": "runtime/gpu/angle-d3d11.gn", "sha256": text_sha(args)},
        "toolchainPackages": [line for line in package.splitlines() if any(line.startswith("mingw-w64-ucrt-x86_64-" + name + " ") for name in package_names)],
        "binaries": [{"file": name, "sha256": sha(out / name), "bytes": (out / name).stat().st_size} for name in ("libEGL.dll", "libGLESv2.dll")],
        "evidence": [{"path": "compliance/evidence/" + name, "sha256": text_sha(work / name)} for name in ("angle-commands.txt", "angle-targets.json")],
        "notices": {"path": "compliance/notices/angle-third-party.txt", "sha256": text_sha(notices)},
        "fontPreprocessing": {"dataPresent": False, "disabledBody": "return nullptr;", "sha256": hashlib.sha256(preprocessed).hexdigest()},
        "excludedImplementations": list(FORBIDDEN), "inputs": inputs,
        "componentCounts": {name: sum(item["component"] == name for item in inputs) for name in sorted({item["component"] for item in inputs})},
    }
    write_json(out / "ANGLE_BUILD.json", record)
    (out / "ANGLE-NOTICES.txt").write_text(notices.read_text(encoding="utf-8"), encoding="utf-8", newline="\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--msys", type=Path, default=ROOT / "build/secure-runtime/toolchain/msys64")
    parser.add_argument("--write-evidence", action="store_true")
    args = parser.parse_args()
    work = args.work.resolve()
    record = inspect(work, args.msys.resolve())
    if args.write_evidence:
        write_json(ROOT / "compliance/evidence/angle-build.json", record)
        for name in ("angle-commands.txt", "angle-targets.json"):
            (ROOT / "compliance/evidence" / name).write_text((work / name).read_text(encoding="utf-8"), encoding="utf-8", newline="\n")
    print(f"Inspected {len(record['inputs'])} ANGLE compiler/source inputs; excluded implementation and font checks passed.")


if __name__ == "__main__":
    main()
