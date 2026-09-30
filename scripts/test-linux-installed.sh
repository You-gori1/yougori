#!/usr/bin/env bash
# Run as an ordinary desktop user, with Xvfb/dbus-run-session in headless CI.
set -euo pipefail
if [ "$(id -u)" = 0 ]; then
  echo 'Run the installed-app smoke test as a normal user, not root.' >&2
  exit 1
fi
if yougori-cli app status >/dev/null 2>&1; then
  echo 'Another Yougori engine is running for this user; it was not touched.' >&2
  exit 1
fi
test_root=$(mktemp -d /tmp/yougori-installed-test.XXXXXX)
export XDG_DATA_HOME="$test_root/data"
export XDG_CONFIG_HOME="$test_root/config"
export XDG_CACHE_HOME="$test_root/cache"
export XDG_RUNTIME_DIR="$test_root/run"
mkdir -m 700 "$XDG_RUNTIME_DIR"
cleanup() {
  yougori-cli app quit --yes >/dev/null 2>&1 || true
  echo "Smoke-test data/logs retained at $test_root"
}
trap cleanup EXIT
yougori app start --app /usr/bin/yougori-desktop
yougori-cli app status
yougori-cli env list
yougori-cli call get_platform_state
yougori-cli app show
yougori-cli app quit --yes
