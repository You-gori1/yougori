#!/bin/sh
# Yougori CLI and engine installer for macOS and Linux. Download the desktop app separately.
#   curl -fsSL https://yougori.com/install.sh | bash
# Downloads the current release, checks its SHA-256 (and the Apple signature on macOS), installs it
# for this user by default, or through apt for an explicit Debian/Ubuntu desktop install. Links
# `yougori` into ~/.local/bin and runs `yougori doctor`.
# Environment overrides: YOUGORI_ENGINE_ONLY=0 (opt into the desktop bundle),
# YOUGORI_RELEASES_URL (HTTPS manifest), YOUGORI_AUTOSTART=1.
set -eu

fail() { printf 'Yougori install failed: %s\n' "$1" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "this installer needs '$1'"; }
need curl
need uname
need tar

manifest_url="${YOUGORI_RELEASES_URL:-https://yougori.com/releases/latest.json}"
case "$manifest_url" in https://*) ;; *) fail "YOUGORI_RELEASES_URL must be HTTPS" ;; esac
engine_only="${YOUGORI_ENGINE_ONLY:-1}"

os=$(uname -s)
arch=$(uname -m)
case "$arch" in x86_64|amd64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) fail "unsupported processor $arch" ;; esac
case "$os" in Darwin) platform="macos-$arch" ;; Linux) platform="linux-$arch" ;; *) fail "unsupported system $os (on Windows use: irm https://yougori.com/install.ps1 | iex)" ;; esac
[ "$engine_only" = "1" ] && platform="$platform-engine"

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

echo "Finding the latest Yougori release..."
manifest=$(curl -fsSL "$manifest_url") || fail "cannot read $manifest_url"
# Read one asset field without requiring jq: the manifest is small, flat JSON per asset.
field() {
  printf '%s' "$manifest" | tr -d '\n' | sed -n "s/.*\"$platform\"[[:space:]]*:[[:space:]]*{\([^}]*\)}.*/\1/p" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
}
version=$(printf '%s' "$manifest" | tr -d '\n' | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
url=$(field url)
sha=$(field sha256)
[ -n "$url" ] && [ -n "$sha" ] || fail "release $version has no download for $platform yet"
case "$url" in https://*) ;; *) fail "the release download is not HTTPS" ;; esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
file="$work/$(basename "$url")"
echo "Downloading Yougori $version..."
curl -fL --progress-bar "$url" -o "$file" || fail "download failed"
[ "$(hash_file "$file")" = "$(printf '%s' "$sha" | tr 'A-F' 'a-f')" ] || fail "the download does not match the published SHA-256. Nothing was installed."

bin="$HOME/.local/bin"
mkdir -p "$bin"
case "$platform" in
  *-engine)
    # The engine and CLI without the desktop app. It needs no display; Linux uses the system QEMU.
    case "$file" in *.tar.gz) ;; *) fail "expected a .tar.gz engine archive" ;; esac
    staged="$work/unpacked"
    mkdir -p "$staged"
    tar -xzf "$file" -C "$staged" || fail "cannot unpack the engine archive. Nothing was installed."
    [ -f "$staged/yougori-engine" ] && [ -f "$staged/cli/yougori" ] || fail "the engine archive is incomplete. Nothing was installed."
    if [ "$os" = "Darwin" ]; then
      codesign --verify --strict "$staged/yougori-engine" 2>/dev/null || fail "the engine signature is not valid. Nothing was installed."
    fi
    target="$HOME/.local/opt/yougori-engine"
    [ -x "$target/cli/yougori" ] && "$target/cli/yougori" app quit --yes >/dev/null 2>&1 && sleep 5 || true
    mkdir -p "$target"
    cp -R "$staged/." "$target/"
    chmod 755 "$target/yougori-engine" "$target/cli/yougori"
    ln -sf "$target/cli/yougori" "$bin/yougori"
    if [ "$os" = "Linux" ]; then
      missing=""
      command -v qemu-system-x86_64 >/dev/null 2>&1 || missing="$missing qemu-system-x86_64"
      command -v qemu-img >/dev/null 2>&1 || missing="$missing qemu-img"
      if command -v ldconfig >/dev/null 2>&1 && ! ldconfig -p 2>/dev/null | grep -q 'libgtk-3.so.0'; then missing="$missing libgtk-3"; fi
      [ -z "$missing" ] || echo "Also install:$missing  (Debian/Ubuntu: sudo apt-get install -y qemu-system-x86 qemu-utils ovmf libgtk-3-0)"
    fi
    ;;
  macos-*)
    case "$file" in *.dmg) ;; *) fail "expected a .dmg for macOS" ;; esac
    mount="$work/mount"
    mkdir -p "$mount"
    hdiutil attach -nobrowse -quiet -mountpoint "$mount" "$file" || fail "cannot open the disk image"
    app=$(find "$mount" -maxdepth 1 -name '*.app' | head -n 1)
    [ -n "$app" ] || { hdiutil detach -quiet "$mount"; fail "no app in the disk image"; }
    codesign --verify --deep --strict "$app" 2>/dev/null || { hdiutil detach -quiet "$mount"; fail "the app signature is not valid. Nothing was installed."; }
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Yougori.app"
    cp -R "$app" "$HOME/Applications/Yougori.app"
    hdiutil detach -quiet "$mount"
    ln -sf "$HOME/Applications/Yougori.app/Contents/Resources/cli/yougori" "$bin/yougori"
    ;;
  linux-*)
    case "$file" in *.deb) ;; *) fail "expected a .deb installer for Linux" ;; esac
    need apt-get
    echo "Installing the Debian/Ubuntu package and its runtime dependencies (administrator access required)..."
    if [ "$(id -u)" = 0 ]; then
      apt-get install -y "$file" || fail "package installation failed"
    else
      need sudo
      sudo apt-get install -y "$file" || fail "package installation failed"
    fi
    # The package owns /usr/bin/yougori and the desktop runtime together.
    # Replace a previous per-user launcher so it cannot shadow the installed CLI.
    ln -sf /usr/bin/yougori "$bin/yougori"
    ;;
esac

case ":$PATH:" in *":$bin:"*) ;; *) echo "Add $bin to your PATH (for example in ~/.profile): export PATH=\"$bin:\$PATH\""; PATH="$bin:$PATH" ;; esac
[ "${YOUGORI_AUTOSTART:-}" = "1" ] && yougori app autostart on >/dev/null || true
yougori doctor --format table || true
echo
if [ "$engine_only" = "1" ]; then echo "Yougori $version is installed (engine only, no desktop app). Try: yougori status"
else echo "Yougori $version is installed. Try: yougori status"; fi
[ "${YOUGORI_AUTOSTART:-}" = "1" ] || echo "Keep the engine ready at login with: yougori app autostart on"
