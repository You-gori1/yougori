# Test fixture only. Package managers and all network requests are mocked.
od_tool=$1
od_test_pm=$2
od_test_script=$3
od_test_mode=$4
od_test_root=$5
export od_test_mode od_test_root od_tool
command() {
  if [ "$1" != -v ]; then builtin command "$@"; return; fi
  case "$2" in
    "$od_test_pm") printf '%s\n' "$od_test_pm";;
    node) case "$PATH" in *yougori/v22.1.0-x64/bin*) printf '%s/.local/share/yougori/v22.1.0-x64/bin/node\n' "$HOME";; *) printf '%s/node\n' "$od_test_root";; esac;;
    npm) printf npm;;
    ollama) printf '%s/ollama\n' "$od_test_root";;
    zstd) return 1;;
    apk|apt-get|dnf|microdnf|yum|zypper|pacman|swupd|sudo|doas|git) return 1;;
    *) builtin command "$@";;
  esac
}
id() { if [ "$od_test_mode" = nonroot ]; then printf 1001; else printf 0; fi; }
od_mock_packages() { printf '%s\n' "$*" >> "$od_test_root/packages"; }
apk() {
  od_mock_packages apk "$@"
  case " $* " in *' ollama '*)
    if [ "$od_test_mode" = package-failure ]; then return 1; fi
    cp "$od_test_root/node" "$od_test_root/ollama";;
  esac
}
apt-get() { od_mock_packages apt-get "$@"; }
dnf() { od_mock_packages dnf "$@"; }
microdnf() { od_mock_packages microdnf "$@"; }
yum() { od_mock_packages yum "$@"; }
zypper() { od_mock_packages zypper "$@"; }
pacman() { od_mock_packages pacman "$@"; }
swupd() { od_mock_packages swupd "$@"; }
env() { shift; "$@"; }
node() {
  case "$od_test_mode:$PATH" in old-node:*yougori/v22.1.0-x64/bin*) return 0;; old-node:*|bad-checksum:*) return 1;; *) return 0;; esac
}
uname() { case "$od_test_mode" in arm64) printf aarch64;; unsupported-arch) printf riscv64;; *) printf x86_64;; esac; }
sha256sum() {
  od_mock_packages verify-checksum
  if [ "$od_test_mode" = bad-checksum ]; then return 1; fi
  return 0
}
tar() {
  if [ "$od_tool" = ollama ]; then
    od_mock_packages extract-ollama
    if [ "$od_test_mode" = extract-failure ]; then return 2; fi
    builtin command cat >/dev/null
    mkdir -p "$HOME/.local/share/yougori/ollama/bin"
    cp "$od_test_root/node" "$HOME/.local/share/yougori/ollama/bin/ollama"
    return
  fi
  od_mock_packages extract-node
  mkdir -p "$HOME/.local/share/yougori/v22.1.0-x64/bin"
  cp "$od_test_root/node" "$HOME/.local/share/yougori/v22.1.0-x64/bin/node"
}
zstd() {
  od_mock_packages decompress-ollama
  # Refuse an uncompressed staging file, including successful-download tests.
  if [ "$1" != -dc ]; then return 19; fi
  printf fake-tar
  if [ "$od_test_mode" = decompress-failure ]; then return 1; fi
}
export -f zstd tar od_mock_packages
npm() {
  od_mock_packages npm "$@"
  if [ "$od_test_mode" = package-failure ]; then return 1; fi
  mkdir -p "$HOME/.local/share/yougori/$od_tool/bin"
  cp "$od_test_root/node" "$HOME/.local/share/yougori/$od_tool/bin/$od_tool"
}
curl() {
  od_mock_packages curl "$@"
  if [ "$od_test_mode" = download-failure ]; then return 22; fi
  for od_arg in "$@"; do od_dest=$od_arg; done
  case "$od_dest" in
    */SHASUMS256.txt) printf '%s  node-v22.1.0-linux-x64.tar.gz\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa > "$od_dest"; return;;
    */node.tar.gz) printf fake-archive > "$od_dest"; return;;
    */ollama.tar.zst) printf fake-archive > "$od_dest"; return;;
  esac
  if [ "$od_tool" = openclaw ]; then
    printf '%s\n' '#!/bin/sh' \
      'printf "upstream-openclaw %s\n" "$*" >> "$od_test_root/packages"' \
      'if [ "$od_test_mode" = upstream-failure ]; then printf "Upstream runtime safety check failed.\n" >&2; exit 37; fi' \
      'test "$1" = --prefix || exit 9' \
      'od_mock_prefix=$2' \
      'mkdir -p "$od_mock_prefix/bin"' \
      'printf "%s\n" "#!/bin/sh" '\''printf "openclaw %s\\n" "$*" >> "$od_test_root/packages"'\'' '\''if [ "$od_test_mode" = version-failure ]; then exit 42; fi'\'' '\''printf "mock-openclaw-version\\n"'\'' > "$od_mock_prefix/bin/openclaw"' \
      'chmod 755 "$od_mock_prefix/bin/openclaw"' > "$od_dest"
    return
  fi
  if [ "$od_tool" = opencode ]; then
    printf '%s\n' '#!/bin/sh' 'mkdir -p "$HOME/.opencode/bin"' 'printf "#!/bin/sh\nprintf mock-version\n" > "$HOME/.opencode/bin/opencode"' 'chmod 755 "$HOME/.opencode/bin/opencode"' > "$od_dest"
    return
  fi
  printf '%s\n' '#!/bin/sh' 'printf "#!/bin/sh\nprintf mock-version\n" > "$HOME/.local/bin/'"$od_tool"'"' 'chmod 755 "$HOME/.local/bin/'"$od_tool"'"' > "$od_dest"
}
. "$od_test_script"
