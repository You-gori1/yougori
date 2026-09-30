#!/bin/bash
# Yougori launcher supervisor. Runs in a terminal session, so the dev script sees a real terminal
# (colours, prompts, shortcuts) exactly as on the PC. Installs dependencies when package files
# change, starts the proxy that publishes the app, and restarts the dev script on request.
set -u
L=/yougori/launch
. "$L/dev.env"
mark() { printf '\n[yougori-%s:%s]\n' "$YOUGORI_NONCE" "$1"; }
cd /workspace || { mark "error:no-workspace"; exit 1; }
export PORT="$YOUGORI_APP_PORT" NODE_ENV=development BROWSER=none COREPACK_ENABLE_DOWNLOAD_PROMPT=0
export npm_config_cache=/yougori/cache/npm YARN_CACHE_FOLDER=/yougori/cache/yarn BUN_INSTALL_CACHE_DIR=/yougori/cache/bun
export npm_config_progress=false
[ -p "$L/wait.fifo" ] || { rm -f "$L/wait.fifo"; mkfifo "$L/wait.fifo"; }

# PIDs from an earlier container boot may now belong to unrelated processes.
rm -f "$L/dev.pid" "$L/restart" "$L/proxy.pid"
proxy_pid=0
ensure_proxy() {
  if [ "$proxy_pid" -gt 1 ] && kill -0 "$proxy_pid" 2>/dev/null; then return; fi
  setsid node "$L/proxy.cjs" "$YOUGORI_PROXY_PORT" "$YOUGORI_APP_PORT" "$YOUGORI_NONCE" </dev/null &
  proxy_pid=$!
  first=1
}
pid=0
cleanup() {
  if [ "$proxy_pid" -gt 1 ]; then kill -TERM -- "-$proxy_pid" 2>/dev/null || true; fi
  if [ "$pid" -gt 1 ]; then kill -TERM -- "-$pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 0' HUP INT TERM

# Compare counters around each phase: old OOM events must not be blamed for a
# new failure. Missing counters mean the cause is unconfirmed, even for 137.
oom_kills() {
  awk '$1 == "oom_kill" {print $2}' /sys/fs/cgroup/memory.events 2>/dev/null
}
failure() {
  local phase="$1" code="$2" before="$3" after reason=exit memory cpu
  after=$(oom_kills)
  if [[ "$before" =~ ^[0-9]+$ && "$after" =~ ^[0-9]+$ ]] && [ "$after" -gt "$before" ]; then
    reason=oom
  elif [ "$code" = 137 ]; then reason=killed; fi
  memory=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || true)
  cpu=$(awk '$1 != "max" && $2 > 0 {printf "%.2f", $1 / $2}' /sys/fs/cgroup/cpu.max 2>/dev/null)
  mark "failure:$phase:$code:$reason:$memory:$cpu"
  if [ "$proxy_pid" -gt 1 ] && ! kill -0 "$proxy_pid" 2>/dev/null; then mark proxy-stopped; fi
}

fingerprint() {
  cat package.json package-lock.json npm-shrinkwrap.json pnpm-lock.yaml pnpm-workspace.yaml yarn.lock \
    bun.lock bun.lockb .npmrc .yarnrc.yml requirements*.txt pyproject.toml setup.py setup.cfg .python-version 2>/dev/null | sha256sum | cut -d' ' -f1
}
# YOUGORI_INSTALLS has one line per install: folder, marker, required, label and command, separated
# by tabs. Each runs again when its command or its folder's package files change, or when its
# marker (such as node_modules) is gone. Only the project's own install must succeed; other package
# folders and tools warn and let the app start.
dependencies() {
  local folder marker required label command now saved code
  while IFS=$'\t' builtin read -r -u 4 folder marker required label command; do
    [ -n "$command" ] && [ -d "$folder" ] || continue
    now=$(cd -- "$folder" && { printf '%s\n' "$command"; fingerprint; } | sha256sum | cut -d' ' -f1)
    saved="$L/installed-$(printf '%s' "$folder $label" | sha256sum | cut -c1-16)"
    if [ "$now" = "$(cat "$saved" 2>/dev/null)" ] && (cd -- "$folder" && [ -e "$marker" ]); then continue; fi
    mark "installing:$label"
    (cd -- "$folder" && eval "$command")
    code=$?
    if [ "$code" = 0 ]; then
      # Projects without dependencies create no node_modules; the marker still records success.
      case "$marker" in /*) ;; *) (cd -- "$folder" && mkdir -p -- "$marker") ;; esac
      printf '%s' "$now" > "$saved"
    elif [ "$required" = 1 ]; then
      return "$code"
    else
      mark "install-warning:$code:$label"
    fi
  done 4<<< "${YOUGORI_INSTALLS:-}"
  return 0
}

exec 3<&0
first=1
attempt=1
while :; do
  . "$L/dev.env"
  # Only an explicit restart or a synced edit opens a new two-attempt budget.
  if [ -f "$L/restart" ]; then attempt=1; fi
  rm -f "$L/restart"
  mark "attempt:$attempt"
  ensure_proxy
  oom_before=$(oom_kills)
  dependencies
  code=$?
  phase=install
  if [ "$code" = 0 ]; then
    phase=run
    # Copies from Windows lose executable bits and may carry CRLF line endings; see prepare.cjs.
    node "$L/prepare.cjs" workspace "${YOUGORI_TWO_WAY:-0}" </dev/null 2>/dev/null || true
    # Memory pressure during installation may also have killed the proxy.
    ensure_proxy
    oom_before=$(oom_kills)
    mark starting
    # The proxy found the first server itself; later ones need a nudge to report ready again.
    [ "$first" = 1 ] || kill -USR2 "$proxy_pid" 2>/dev/null || true
    first=0
    setsid bash -c "$YOUGORI_RUN" <&3 &
    pid=$!
    echo "$pid" > "$L/dev.pid"
    wait "$pid"
    code=$?
    # A failed parent can leave children holding the server port open.
    kill -TERM -- "-$pid" 2>/dev/null || true
    pid=0
    rm -f "$L/dev.pid"
    if [ -f "$L/restart" ]; then mark restarting; continue; fi
  fi
  if [ -f "$L/restart" ]; then mark restarting; continue; fi
  if [ "$code" != 0 ]; then failure "$phase" "$code" "$oom_before"; fi
  # A command that is missing (127) or not executable (126) fails the same way every time.
  retry=1
  if [ "$phase" = run ] && { [ "$code" = 126 ] || [ "$code" = 127 ]; }; then retry=0; fi
  if [ "$code" != 0 ] && [ "$attempt" -lt 2 ] && [ "$retry" = 1 ]; then
    attempt=$((attempt + 1))
    mark "retrying:$phase:$attempt:$code"
    # Brief backoff; signals still stop the supervisor and a manual restart
    # is picked up at the top of the loop.
    read -r -t 2 _ <> "$L/wait.fifo" || true
    continue
  fi
  if [ "$phase" = install ]; then mark "install-failed:$code"; else mark "exited:$code"; fi
  # Wait for a restart request (a saved change, or r + Enter) without starting processes.
  until [ -f "$L/restart" ]; do read -r -t 0.5 _ <> "$L/wait.fifo" || true; done
  mark restarting
done
