#!/usr/bin/env bash
set -euo pipefail
# Build-time only: authenticate packages with Alpine's signed APK index, then
# retain just resize2fs and its private library closure (no guest download).
repo_root="$(realpath "${1:?repository required}")"
task_directory="$(mktemp -d /tmp/yougori-storage-tools.XXXXXX)"
trap 'case "$task_directory" in /tmp/yougori-storage-tools.*) rm -rf -- "$task_directory" ;; esac' EXIT
rootfs="$task_directory/root"
mkdir -p "$rootfs"
archive="$repo_root/build/appliance-cache/alpine-minirootfs-3.24.1-x86_64.tar.gz"
expected="$(awk 'NR==1 {print $1}' "$archive.checksum")"
test "$(sha256sum "$archive" | cut -d' ' -f1)" = "$expected"
tar -xzf "$archive" -C "$rootfs"
cp /etc/resolv.conf "$rootfs/etc/resolv.conf"
# Use the host CA trust for verified HTTPS bootstrap inside the clean root.
mkdir -p "$rootfs/etc/ssl/certs"
cp /etc/ssl/certs/ca-certificates.crt "$rootfs/etc/ssl/certs/ca-certificates.crt"
chroot "$rootfs" /sbin/apk add --no-cache e2fsprogs-extra=1.47.4-r0 musl=1.2.6-r2
destination="$repo_root/build/appliance-cache/storage-tools"
mkdir -p "$destination"
install -m 0755 "$rootfs/usr/sbin/resize2fs" "$destination/resize2fs"
chroot "$rootfs" /usr/bin/ldd /usr/sbin/resize2fs
while read -r library; do
    case "$library" in /lib/*.so*|/usr/lib/*.so*) cp -L "$rootfs$library" "$destination/$(basename "$library")" ;; *) exit 1 ;; esac
done < <(chroot "$rootfs" /usr/bin/ldd /usr/sbin/resize2fs | awk '{for(i=1;i<=NF;i++) if($i ~ /^\//) print $i}' | sort -u)
chroot "$rootfs" /sbin/apk info -v > "$destination/PACKAGES.txt"
notices="$repo_root/src-tauri/resources/runtime/appliance/storage-notices"
mkdir -p "$notices"
curl --fail --location --proto '=https' --tlsv1.2 --retry 3 https://raw.githubusercontent.com/tytso/e2fsprogs/v1.47.4/NOTICE -o "$notices/e2fsprogs-NOTICE.txt"
curl --fail --location --proto '=https' --tlsv1.2 --retry 3 'https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.6' -o "$notices/musl-COPYRIGHT.txt"
cp "$destination/PACKAGES.txt" "$notices/BUILD-PACKAGES.txt"
echo "Private offline filesystem tools staged in $destination"
