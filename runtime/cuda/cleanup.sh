#!/bin/bash
# Temporary maintenance runtime: no NVIDIA probe, workload start, or installation.
set -euo pipefail
test -f /etc/opendock-cuda-ready
test -f /etc/opendock-cuda-runtime
exec 9>/run/opendock-cuda.lock
flock -n 9 || { echo 'CUDA runtime is already owned by another Yougori process.' >&2; exit 1; }
read -r OPENDOCK_CUDA_TOKEN
read -r cuda_port
read -r agent_size
[[ "$OPENDOCK_CUDA_TOKEN" =~ ^[a-f0-9]{64}$ && "$cuda_port" =~ ^[0-9]{4,5}$ && "$agent_size" =~ ^[0-9]+$ ]]
export OPENDOCK_CUDA_MODE=1 OPENDOCK_CUDA_TOKEN
export OPENDOCK_CUDA_LISTEN="127.0.0.1:$cuda_port"
scratch=$(mktemp -d /run/yougori-cleanup.XXXXXXXX)
containerd_pid=''
agent_pid=''
cleanup() {
  trap - EXIT TERM INT
  if [[ -n "$agent_pid" ]]; then kill "$agent_pid" 2>/dev/null || true; wait "$agent_pid" 2>/dev/null || true; fi
  if [[ -n "$containerd_pid" ]]; then kill "$containerd_pid" 2>/dev/null || true; wait "$containerd_pid" 2>/dev/null || true; fi
  rm -f -- "$scratch/agent"
  rmdir -- "$scratch"
  sync
}
trap cleanup EXIT TERM INT
# Host sends its verified bundled binary directly; Windows drives stay unmounted.
dd bs=65536 count="$agent_size" iflag=count_bytes,fullblock status=none of="$scratch/agent"
[[ "$(stat -c %s "$scratch/agent")" == "$agent_size" ]]
chmod 0700 "$scratch/agent"
"$scratch/agent" --prepare-container-storage
containerd --config /etc/containerd/config.toml &
containerd_pid=$!
for attempt in $(seq 1 100); do
  [[ -S /run/containerd/containerd.sock ]] && break
  kill -0 "$containerd_pid"
  sleep 0.1
done
"$scratch/agent" &
agent_pid=$!
wait "$agent_pid"
