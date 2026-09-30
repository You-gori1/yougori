#!/bin/bash
# Run only inside the newly imported, Yougori-owned Ubuntu distribution.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
test "$(id -u)" = 0
test -f /etc/opendock-cuda-runtime
test -e /dev/dxg || { echo 'WSL GPU bridge missing. Update WSL and the Windows NVIDIA driver.' >&2; exit 1; }
if [[ -f /etc/opendock-cuda-ready ]] && command -v nvidia-ctk >/dev/null && command -v nerdctl >/dev/null; then
  echo 'CUDA dependencies are already installed; the Yougori agent was updated.'
  exit 0
fi
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl gnupg iproute2 iptables util-linux fuse3 e2fsprogs
work_dir=$(mktemp -d /tmp/opendock-cuda-setup.XXXXXXXX)
trap 'rm -rf -- "$work_dir"' EXIT
curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
  https://github.com/containerd/nerdctl/releases/download/v2.3.5/nerdctl-full-2.3.5-linux-amd64.tar.gz -o "$work_dir/nerdctl.tar.gz"
echo "b697295c623639734aaab737523c808fd3cc8d3046039fd94fff1744e4c317aa  $work_dir/nerdctl.tar.gz" | sha256sum -c -
mkdir "$work_dir/nerdctl"
tar -xzf "$work_dir/nerdctl.tar.gz" -C "$work_dir/nerdctl"
install -d /usr/local/bin /usr/local/libexec/cni /etc/containerd /etc/nerdctl
for binary in containerd containerd-shim-runc-v2 nerdctl runc; do
  install -m 0755 "$work_dir/nerdctl/bin/$binary" /usr/local/bin/
done
for plugin in bridge firewall host-local loopback portmap tuning; do
  install -m 0755 "$work_dir/nerdctl/libexec/cni/$plugin" /usr/local/libexec/cni/
done
curl --fail --location --proto '=https' --tlsv1.2 https://nvidia.github.io/libnvidia-container/gpgkey -o "$work_dir/nvidia.asc"
gpg --batch --yes --dearmor --output /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg "$work_dir/nvidia.asc"
printf '%s\n' 'deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://nvidia.github.io/libnvidia-container/stable/deb/amd64 /' > /etc/apt/sources.list.d/nvidia-container-toolkit.list
apt-get update
# CDI needs only the base tools, never a Linux GPU driver or privileged runtime.
apt-get install -y --no-install-recommends nvidia-container-toolkit-base=1.20.0-1
printf '%s\n' 'version = 2' 'disabled_plugins = ["io.containerd.grpc.v1.cri"]' > /etc/containerd/config.toml
printf '%s\n' 'cgroup_manager = "cgroupfs"' > /etc/nerdctl/nerdctl.toml
apt-get clean
touch /etc/opendock-cuda-ready
echo 'Yougori CUDA runtime installed.'
