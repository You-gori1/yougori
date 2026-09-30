#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
tools="$repo/build/secure-runtime/firmware-tools/bin"
if [[ ! -x "$tools/virt-fw-vars" ]]; then
  python3 -m venv "$repo/build/secure-runtime/firmware-tools"
  "$tools/pip" install virt-firmware==25.7 pefile==2024.8.26 cryptography==50.0.1 cffi==2.1.1 typing-extensions==4.16.0 pycparser==3.0
fi
out="${2:-$repo/build/secure-runtime/firmware}"
boot_helper="${1:-$repo/src-tauri/boot-helper/bootx64.efi}"
mkdir -p "$out"
# Authenticode PE digest, NOT the ordinary file hash. Only this exact read-only
# bootstrap is trusted. No private signing key is shipped or saved to disk.
digest=$(pesign -h -i "$boot_helper" | sed -n 's/^hash: //p')
[[ "$digest" =~ ^[0-9a-f]{64}$ ]]
"$tools/python3" "$tools/virt-fw-vars" --enroll-generate 'Yougori virtual platform' --secure-boot \
  --microsoft-kek all \
  --set-dbx "$repo/build/runtime-cache/secureboot-objects/PostSignedObjects/SignedByKEK2023/dbx_x64.efiauth2" \
  --add-db-hash 41c2d866-a940-4fc2-aab3-203ace9d57a1 "$digest" \
  --output-json "$out/secure-vars.json"
