#!/bin/bash
# Build only the separate CUDA payload; never replace a running QEMU appliance.
set -euo pipefail
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output="${1:-$repo_root/src-tauri/resources/runtime/cuda}"
bash "$repo_root/scripts/check-agent-go.sh"
command -v musl-gcc >/dev/null
command -v gcc >/dev/null
mkdir -p "$output"
(cd "$repo_root/appliance/agent" && CGO_ENABLED=0 go build -buildvcs=false -trimpath -ldflags='-s -w -buildid=' -o "$output/opendock-agent" .)
musl-gcc -static -Os -s -Wall -Wextra -Werror -o "$output/opendock-mount-helper" "$repo_root/appliance/mount-helper.c"
gcc -Os -s -Wall -Wextra -Werror -o "$output/opendock-cuda-probe" "$repo_root/runtime/cuda/kernel-probe.c" -ldl
(cd "$output" && sha256sum opendock-agent opendock-mount-helper opendock-cuda-probe > SHA256SUMS)
echo 'CUDA payload built and checksummed. Rebuild the desktop to embed its checksums.'
