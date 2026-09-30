#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
source_dir="$repo/build/runtime-cache/qemu-secure-src"
output="${YOUGORI_QEMU_BUILD_DIRECTORY:-$repo/build/secure-runtime/qemu-build}"
mkdir -p "$output"
cd "$output"
export PYTHON=/ucrt64/bin/python
# Normalize both MSYS and native Windows forms before __FILE__ and debug
# strings are embedded in a distributable executable.
native_repo=$(cygpath -m "$repo")
if [[ "$repo" =~ [[:space:]] ]]; then
  echo 'Build secure QEMU from a checkout path without whitespace.' >&2
  exit 1
fi
privacy_cflags="-ffile-prefix-map=$repo=/yougori -ffile-prefix-map=$native_repo=/yougori"
tool_option=--disable-tools
targets=(qemu-system-x86_64.exe)
if [[ "${YOUGORI_QEMU_BUILD_TOOLS:-0}" == 1 ]]; then
  tool_option=--enable-tools
  targets+=(qemu-img.exe)
fi
"$source_dir/configure" --target-list=x86_64-softmmu --enable-whpx --enable-tpm \
  --prefix='C:/yougori' --sysconfdir='C:/yougori/etc' \
  --extra-cflags="$privacy_cflags" \
  --enable-virglrenderer --enable-opengl --enable-gnutls --enable-slirp \
  --disable-docs --disable-gtk --disable-sdl --disable-curses "$tool_option" \
  --audio-drv-list=dsound --disable-jack --disable-brlapi --disable-spice \
  --disable-curl --disable-libssh --disable-xen --disable-xen-pci-passthrough \
  --disable-guest-agent --disable-werror --disable-debug-info --disable-strip \
  --disable-user --enable-vnc --disable-install-blobs
ninja -j "${YOUGORI_QEMU_BUILD_JOBS:-4}" "${targets[@]}"
