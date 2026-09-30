#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
required="$(awk '$1 == "go" { print $2; exit }' "$repo_root/appliance/agent/go.mod")"
message="Building the guest agent requires Go $required or newer on PATH. Install it from https://go.dev/dl/; Ubuntu 22.04's golang-go package is too old."
if ! command -v go >/dev/null 2>&1; then
  echo "$message" >&2
  exit 1
fi
# Run inside the module so recent Go versions can select its required toolchain.
version="$(cd "$repo_root/appliance/agent" && go version)"
version="$(awk '{sub(/^go/, "", $3); print $3}' <<< "$version")"
if [[ "$(printf '%s\n' "$required" "$version" | sort -V | head -n1)" != "$required" ]]; then
  echo "$message Found Go $version." >&2
  exit 1
fi
