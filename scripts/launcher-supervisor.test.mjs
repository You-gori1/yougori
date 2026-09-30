// Exercise the actual Bash retry loop with isolated files and fake workloads.
// No engine, package downloads, containers or real process-group signals.
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"

const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash"
const unix = path => path.replaceAll("\\", "/").replace(/^([A-Za-z]):/, (_, drive) => `/${drive.toLowerCase()}`)
const supervisor = readFileSync(new URL("../cli/src/launcher/assets/dev.sh", import.meta.url), "utf8").replaceAll("\r\n", "\n")

function run(t, { installFailures = 0, runFailures = 0, code = 1, restart = false, cancelRetry = false, oom = false, oldOom = 0, proxyDies = false, nested = null } = {}) {
  const root = mkdtempSync(join(tmpdir(), "yougori-supervisor-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  mkdirSync(join(root, "launch"))
  mkdirSync(join(root, "project"))
  mkdirSync(join(root, "cgroup"))
  writeFileSync(join(root, "cgroup/memory.events"), `oom_kill ${oldOom}\n`)
  writeFileSync(join(root, "cgroup/memory.max"), "1073741824\n")
  writeFileSync(join(root, "cgroup/cpu.max"), "100000 100000\n")
  writeFileSync(join(root, "project/package.json"), "{}")
  // A package folder inside the project installs best effort, after the project itself.
  if (nested) {
    mkdirSync(join(root, "project/server"))
    writeFileSync(join(root, "project/server/package.json"), "{}")
  }
  const serverLine = nested ? `\nserver\tnode_modules\t0\tserver/ dependencies\tbash "$FIXTURE_ROOT/server-install.sh"` : ""
  writeFileSync(join(root, "launch/dev.env"), `YOUGORI_APP_PORT=3000
YOUGORI_PROXY_PORT=43119
YOUGORI_NONCE=test
YOUGORI_INSTALLS='${unix(join(root, "project"))}\tnode_modules\t1\tproject\tbash "$FIXTURE_ROOT/install.sh"${serverLine}'
YOUGORI_RUN='bash "$FIXTURE_ROOT/run.sh"'
`)
  for (const [name, failures] of [["install", installFailures], ["run", runFailures], ["server-install", nested?.failures ?? 0]]) {
    writeFileSync(join(root, `${name}.sh`), `n=$(cat "$FIXTURE_ROOT/${name}.count" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$FIXTURE_ROOT/${name}.count"
if [ "$n" -le ${failures} ]; then
  ${proxyDies && name === "install" ? 'touch "$FIXTURE_ROOT/proxy-dead"' : "true"}
  ${oom ? `echo "oom_kill $((n + ${oldOom}))" > "$FIXTURE_ROOT/cgroup/memory.events"` : "true"}
  exit ${code}
fi
${name.endsWith("install") ? "mkdir -p node_modules" : "true"}
`)
  }
  // Preserve the supervisor logic; redirect only its two fixed guest paths.
  writeFileSync(join(root, "dev.sh"), supervisor
    .replace("L=/yougori/launch", 'L="$FIXTURE_ROOT/launch"')
    .replaceAll("/sys/fs/cgroup", '"$FIXTURE_ROOT/cgroup"')
    .replace("cd /workspace", 'cd "$FIXTURE_ROOT/project"'))
  const fixture = `
setsid() {
  if [ "$1" = node ]; then
    rm -f "$FIXTURE_ROOT/proxy-dead"
    echo started >> "$FIXTURE_ROOT/proxy-starts"
    return 0
  fi
  "$@"
}
kill() { if [ "$1" = -0 ] && [ -f "$FIXTURE_ROOT/proxy-dead" ]; then return 1; fi; return 0; }
mkfifo() { touch "$1"; }
read() {
  case "$*" in
    *'-t 2 '*) ${cancelRetry ? "exit 0" : "return 1"} ;;
    *'-t 0.5 '*)
      if [ "$FIXTURE_RESTART" = 1 ] && [ ! -f "$FIXTURE_ROOT/restarted" ]; then
        touch "$FIXTURE_ROOT/restarted" "$L/restart"
        return 1
      fi
      echo idle > "$FIXTURE_ROOT/idle"
      exit 0 ;;
    *) return 1 ;;
  esac
}
. "$FIXTURE_ROOT/dev.sh"
`
  const result = spawnSync(bash, ["--noprofile", "--norc", "-c", fixture], {
    env: { ...process.env, FIXTURE_ROOT: unix(root), FIXTURE_RESTART: restart ? "1" : "0" },
    encoding: "utf8", timeout: 15000,
  })
  if (result.error) throw result.error
  assert.equal(result.status, 0, result.stderr)
  const count = name => existsSync(join(root, `${name}.count`)) ? Number(readFileSync(join(root, `${name}.count`), "utf8")) : 0
  return {
    installs: count("install"), runs: count("run"), serverInstalls: count("server-install"), idle: existsSync(join(root, "idle")),
    proxies: readFileSync(join(root, "proxy-starts"), "utf8").trim().split("\n").length,
    events: [...result.stdout.matchAll(/\[yougori-test:([^\]]+)\]/g)].map(match => match[1]),
  }
}

test("dependency installation succeeds on attempt two, then starts the app", t => {
  const result = run(t, { installFailures: 1, code: 137 })
  assert.equal(result.installs, 2)
  assert.equal(result.runs, 1)
  assert.deepEqual(result.events.filter(e => e.startsWith("retrying:")), ["retrying:install:2:137"])
  assert.equal(result.events.at(-1), "exited:0")
})

test("dev server retries once without reinstalling successful dependencies", t => {
  const result = run(t, { runFailures: 1 })
  assert.equal(result.installs, 1)
  assert.equal(result.runs, 2)
  assert.deepEqual(result.events.filter(e => e.startsWith("attempt:")), ["attempt:1", "attempt:2"])
  assert.equal(result.events.at(-1), "exited:0")
})

test("permanent install failure stops at two and preserves the killed exit code", t => {
  const result = run(t, { installFailures: 99, code: 137 })
  assert.equal(result.installs, 2)
  assert.equal(result.runs, 0)
  assert.equal(result.events.at(-1), "install-failed:137")
  assert.ok(result.idle)
})

test("install and run failures share the same two-attempt budget", t => {
  const result = run(t, { installFailures: 1, runFailures: 99 })
  assert.equal(result.installs, 2)
  assert.equal(result.runs, 1)
  assert.equal(result.events.at(-1), "exited:1")
  assert.ok(result.idle)
})

test("an explicit restart grants another bounded two attempts", t => {
  const result = run(t, { installFailures: 99, restart: true })
  assert.equal(result.installs, 4)
  assert.deepEqual(result.events.filter(e => e.startsWith("attempt:")), ["attempt:1", "attempt:2", "attempt:1", "attempt:2"])
})

test("memory diagnostics require a new OOM kill and include the actual limits", t => {
  const confirmed = run(t, { installFailures: 99, code: 137, oom: true, oldOom: 9 })
  assert.equal(confirmed.events.filter(e => e === "failure:install:137:oom:1073741824:1.00").length, 2)
  const unknown = run(t, { installFailures: 99, code: 137, oldOom: 9 })
  assert.equal(unknown.events.filter(e => e === "failure:install:137:killed:1073741824:1.00").length, 2)
  const ordinary = run(t, { runFailures: 99 })
  assert.ok(ordinary.events.includes("failure:run:1:exit:1073741824:1.00"))
})

test("a proxy killed during installation is reported and restarted on retry", t => {
  const result = run(t, { installFailures: 1, proxyDies: true, code: 137, oom: true })
  assert.ok(result.events.includes("proxy-stopped"))
  assert.equal(result.proxies, 2)
  assert.equal(result.runs, 1)
})

test("successful exits and cancellation during backoff do not launch another attempt", t => {
  const success = run(t)
  assert.equal(success.runs, 1)
  assert.ok(!success.events.some(e => e.startsWith("retrying:")))
  const cancelled = run(t, { installFailures: 99, cancelRetry: true })
  assert.equal(cancelled.installs, 1)
  assert.equal(cancelled.runs, 0)
})

test("a package folder that fails to install warns and the app still starts", t => {
  const result = run(t, { nested: { failures: 99 } })
  assert.equal(result.serverInstalls, 1)
  assert.equal(result.runs, 1)
  assert.ok(result.events.includes("install-warning:1:server/ dependencies"))
  assert.deepEqual(result.events.filter(e => e.startsWith("installing:")), ["installing:project", "installing:server/ dependencies"])
  assert.equal(result.events.at(-1), "exited:0")
})

test("installed package folders are not installed again on retry", t => {
  const result = run(t, { nested: { failures: 0 }, runFailures: 1 })
  assert.equal(result.installs, 1)
  assert.equal(result.serverInstalls, 1)
  assert.equal(result.runs, 2)
})

test("a command that is not found is not retried", t => {
  const result = run(t, { runFailures: 99, code: 127 })
  assert.equal(result.runs, 1)
  assert.ok(!result.events.some(e => e.startsWith("retrying:")))
  assert.ok(result.events.includes("failure:run:127:exit:1073741824:1.00"))
  assert.equal(result.events.at(-1), "exited:127")
})
