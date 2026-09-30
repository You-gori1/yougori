#!/usr/bin/env python3
"""Restore the pinned MSYS2 ANGLE inputs and build only Yougori's D3D11 DLLs.

Requires a Windows MSYS2 UCRT64 GCC, GN, Ninja, Python, pkgconf and patch toolchain.
Sources come from the corresponding-source bundle; this script downloads nothing.
The output is isolated from installed or tracked runtimes. Reuse --work to resume.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parent.parent
REVISION = "890b5d8fa2988e3719e0d80421bf3e927db9cd5c"
COMPONENT = "msys/mingw-w64-ucrt-x86_64-angleproject/2.1.r25748.890b5d8f-6"
PREPARATION_VERSION = 1


def sha(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run(args, cwd, env, output=None):
    print("+ " + subprocess.list2cmdline([str(a) for a in args]), flush=True)
    if output:
        with output.open("w", encoding="utf-8", newline="\n") as stream:
            subprocess.run(args, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT, check=True)
    else:
        subprocess.run(args, cwd=cwd, env=env, check=True)


def source_tree(angle):
    return {path.relative_to(angle).as_posix(): sha(path) for path in sorted(angle.rglob("*"))
            if path.is_file() and path.relative_to(angle).parts[0] != "out" and "__pycache__" not in path.parts}


def restore(inputs, work, env, msys):
    # GN parses some unused targets too; presence in the tree is not build inclusion.
    trees = {
        "angleproject.tar": "angle",
        "bare-clones_build.tar": "angle/build",
        "bare-clones_clang.tar": "angle/tools/clang",
        "bare-clones_zlib.tar": "zlib",
        "bare-clones_spirv-headers.tar": "angle/third_party/spirv-headers/src",
        "bare-clones_spirv-tools.tar": "angle/third_party/spirv-tools/src",
    }
    for archive, target in trees.items():
        with tarfile.open(inputs / "distfiles" / archive) as source:
            source.extractall(work / target, filter="data")
    angle = work / "angle"
    for target, patch in [(angle / "build", "001-add-mingw-toolchain.patch"),
                          (angle, "002-buildflags-fixes.patch"), (angle, "003-angle-src-fixes.patch"),
                          (angle / "third_party/spirv-tools/src", "006-spirv-updates.patch")]:
        run([str(msys / "usr/bin/patch.exe"), "--batch", "--forward", "-p1", "-i",
             (inputs / "recipe" / patch).as_posix()], target, env)
    (angle / "build/config/gclient_args.gni").write_text("build_with_chromium = false\n", encoding="utf-8")
    # LASTCHANGE is build metadata, not an inferred Git revision from the containing checkout.
    (angle / "build/util/LASTCHANGE").write_text(
        "LASTCHANGE=70c150bc0ae989d00c15ab1ed67464198e4890d0\n", encoding="utf-8")
    with tarfile.open(inputs / "distfiles/bare-clones_build.tar") as source:
        timestamp = source.getmembers()[0].mtime
    (angle / "build/util/LASTCHANGE.committime").write_text(str(timestamp) + "\n", encoding="utf-8")
    for recipe, dest in {"zlib.gn": "zlib", "jpeg.gn": "libjpeg_turbo", "jsoncpp.gn": "jsoncpp",
                         "png.gn": "libpng", "rjson.gn": "rapidjson"}.items():
        shutil.copyfile(inputs / "recipe" / recipe, angle / "third_party" / dest / "BUILD.gn")
    shutil.copytree(work / "zlib/google", angle / "third_party/zlib/google", dirs_exist_ok=True)
    run([str(msys / "ucrt64/bin/python.exe"), "src/commit_id.py", "gen", "src/common/angle_commit.h"], angle, env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, default=ROOT / "build/compliance/msys/mingw-w64-angleproject/2.1.r25748.890b5d8f-6")
    parser.add_argument("--source-index", type=Path, default=ROOT / "build/compliance/bundle/SOURCE_INDEX.json")
    parser.add_argument("--msys", type=Path, default=ROOT / "build/secure-runtime/toolchain/msys64")
    parser.add_argument("--work", type=Path, default=ROOT / "build/angle-runtime")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    inputs, work, msys = args.inputs.resolve(), args.work.resolve(), args.msys.resolve()
    if not work.is_relative_to(ROOT / "build") or args.jobs < 1:
        raise SystemExit("Use an isolated work directory inside this checkout's build directory and positive --jobs.")
    component = next(c for c in json.loads(args.source_index.read_text(encoding="utf-8"))["components"] if c["id"] == COMPONENT)
    for item in component["inputs"]:
        if sha(inputs / item["file"]) != item["sha256"]:
            raise SystemExit("ANGLE source input checksum mismatch: " + item["file"])
    env = os.environ.copy()
    env.update({"MSYSTEM": "UCRT64", "MINGW_PREFIX": "/ucrt64", "MINGW_PACKAGE_PREFIX": "mingw-w64-ucrt-x86_64",
                "CC": "gcc", "CXX": "g++", "ANGLE_UPSTREAM_HASH": REVISION[:12],
                "PYTHONDONTWRITEBYTECODE": "1",
                # pkgconf's relocatable prefix otherwise puts the compiler's own
                # include directory before C++ headers, breaking include_next.
                "PKG_CONFIG_SYSTEM_INCLUDE_PATH": (msys / "ucrt64/bin").as_posix() + "/../include",
                "PATH": os.pathsep.join([str(msys / "ucrt64/bin"), str(msys / "usr/bin"), env["PATH"]])})
    for tool in ["ucrt64/bin/gn.exe", "ucrt64/bin/ninja.exe", "ucrt64/bin/g++.exe", "ucrt64/bin/python.exe", "usr/bin/patch.exe"]:
        if not (msys / tool).is_file():
            raise SystemExit("Missing build tool: " + str(msys / tool))
    marker = work / "prepared.json"
    recipe_record = {"component": COMPONENT, "inputs": component["inputs"], "preparationVersion": PREPARATION_VERSION}
    if marker.exists():
        previous = json.loads(marker.read_text())
        if (previous["component"] != COMPONENT or previous["inputs"] != component["inputs"]
                or previous.get("preparationVersion") != PREPARATION_VERSION
                or previous.get("preparedSources") != source_tree(work / "angle")):
            raise SystemExit("Prepared source identity changed; choose a new --work directory.")
    else:
        if work.exists():
            raise SystemExit("Unrecorded work directory exists; choose a new --work directory.")
        work.mkdir(parents=True)
        restore(inputs, work, env, msys)
        recipe_record["preparedSources"] = source_tree(work / "angle")
        marker.write_text(json.dumps(recipe_record, indent=2) + "\n", encoding="utf-8")
    angle = work / "angle"
    out = angle / "out/Yougori-D3D11"
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "runtime/gpu/angle-d3d11.gn", out / "args.gn")
    gn, ninja = str(msys / "ucrt64/bin/gn.exe"), str(msys / "ucrt64/bin/ninja.exe")
    run([gn, "gen", "out/Yougori-D3D11", "--fail-on-unused-args"], angle, env)
    run([gn, "desc", "out/Yougori-D3D11", "*", "--format=json"], angle, env, work / "gn-targets.json")
    run([ninja, "-C", str(out), "-t", "commands", "libEGL.dll", "libGLESv2.dll"], angle, env, work / "target-commands.txt")
    if args.prepare_only:
        return
    run([ninja, "-C", str(out), "-j", str(args.jobs), "libEGL.dll", "libGLESv2.dll"], angle, env, work / "build.log")
    run([ninja, "-C", str(out), "-t", "deps"], angle, env, work / "header-dependencies.txt")
    run([sys.executable, str(ROOT / "scripts/compliance-angle.py"), "--work", str(work), "--msys", str(msys)], ROOT, os.environ.copy())
    print("Built isolated ANGLE DLLs in " + str(out), flush=True)


if __name__ == "__main__":
    main()
