#!/usr/bin/env bash
set -euo pipefail
# Build only; never opens a VM disk or changes the host's firmware settings.
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo/build/runtime-cache/edk2-secure-src"
make -C BaseTools -j8
export WORKSPACE="$PWD" EDK_TOOLS_PATH="$PWD/BaseTools" PYTHON_COMMAND=python3
# OpenSSL is one large module; parallelize its compilation too.
export MAKEFLAGS=-j8
set +u
source edksetup.sh
set -u
build -a X64 -b RELEASE -t GCC -p OvmfPkg/OvmfPkgX64.dsc -n 8 \
  -D QEMU_PV_VARS=TRUE -D SECURE_BOOT_ENABLE=TRUE -D TPM2_ENABLE=TRUE \
  -D TPM2_CONFIG_ENABLE=TRUE -D BUILD_SHELL=FALSE -D NETWORK_PXE_BOOT_ENABLE=FALSE \
  -D NETWORK_ISCSI_ENABLE=FALSE -D FD_SIZE_4MB=TRUE
mkdir -p "$repo/build/secure-runtime/firmware"
cp Build/OvmfX64/RELEASE_GCC/FV/OVMF.fd "$repo/build/secure-runtime/firmware/OVMF.qemuvars.fd"
