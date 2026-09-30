import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"

const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash"
const unix = path => path.replaceAll("\\", "/").replace(/^([A-Za-z]):/, (_, drive) => `/${drive.toLowerCase()}`)
const script = readFileSync(new URL("./install/install.sh", import.meta.url), "utf8").replaceAll("\r\n", "\n")

function install(t, { extension = "deb", badHash = false, aptExit = "0", engineOnly = "0", os = "Linux", assetKey = "linux-x86_64" } = {}) {
  const root = mkdtempSync(join(tmpdir(), "yougori-installer-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const bin = join(root, "bin")
  mkdirSync(bin)
  const stubs = {
    uname: 'if [ "$1" = "-s" ]; then echo "$FIXTURE_OS"; else echo x86_64; fi',
    id: 'echo 1000',
    curl: 'case "$*" in *latest.json*) printf "%s" "$FIXTURE_MANIFEST" ;; *) while [ "$#" -gt 0 ]; do if [ "$1" = "-o" ]; then shift; printf "package" > "$1"; exit 0; fi; shift; done; exit 1 ;; esac',
    sudo: 'exec "$@"',
    "apt-get": 'printf "%s\\n" "$@" > "$FIXTURE_APT_LOG"; exit "$FIXTURE_APT_EXIT"',
    ln: '[ "$1" = "-sf" ] && { [ "$2" = "/usr/bin/yougori" ] || [ "$2" = "$HOME/.local/opt/yougori-engine/cli/yougori" ]; } && [ "$3" = "$HOME/.local/bin/yougori" ]',
    tar: 'while [ "$#" -gt 0 ]; do if [ "$1" = "-C" ]; then shift; mkdir -p "$1/cli"; printf "fixture engine" > "$1/yougori-engine"; printf "fixture cli" > "$1/cli/yougori"; exit 0; fi; shift; done; exit 1',
    codesign: 'exit 0',
    yougori: 'exit 0',
  }
  for (const [name, body] of Object.entries(stubs)) {
    const path = join(bin, name)
    writeFileSync(path, `#!/bin/sh\n${body}\n`)
    chmodSync(path, 0o755)
  }
  const sha = createHash("sha256").update("package").digest("hex")
  const log = join(root, "apt.log")
  const env = { ...process.env, HOME: unix(join(root, "home")), FIXTURE_BIN: unix(bin), FIXTURE_OS: os,
      YOUGORI_ENGINE_ONLY: engineOnly, YOUGORI_AUTOSTART: "", YOUGORI_RELEASES_URL: "https://fixture.invalid/latest.json",
      FIXTURE_APT_LOG: unix(log), FIXTURE_APT_EXIT: aptExit,
      FIXTURE_MANIFEST: JSON.stringify({ version: "1.0.0", assets: { [assetKey]: { url: `https://fixture.invalid/Yougori.${extension}`, sha256: badHash ? "0".repeat(64) : sha } } }),
  }
  if (engineOnly === null) delete env.YOUGORI_ENGINE_ONLY
  const result = spawnSync(bash, ["--noprofile", "--norc", "-c", 'export PATH="$FIXTURE_BIN:$PATH"; /bin/sh -s'], {
    input: script, encoding: "utf8", timeout: 15000, env,
  })
  if (result.error) throw result.error
  return { result, apt: existsSync(log) ? readFileSync(log, "utf8") : null, engineInstalled: existsSync(join(root, "home/.local/opt/yougori-engine/yougori-engine")), cliInstalled: existsSync(join(root, "home/.local/opt/yougori-engine/cli/yougori")) }
}

for (const [os, platform] of [["Linux", "linux"], ["Darwin", "macos"]]) {
  for (const engineOnly of [null, "", "1"]) test(`${os} command installs CLI and engine without the desktop (override ${engineOnly ?? "unset"})`, t => {
    const { result, apt, engineInstalled, cliInstalled } = install(t, { os, engineOnly, assetKey: `${platform}-x86_64-engine`, extension: "tar.gz" })
    assert.equal(result.status, 0, result.stderr)
    assert.equal(apt, null)
    assert.equal(engineInstalled, true)
    assert.equal(cliInstalled, true)
    assert.match(result.stdout, /engine only, no desktop app/)
  })
}

test("default CLI install never falls back to a desktop-only release", t => {
  const { result, apt, engineInstalled } = install(t, { engineOnly: null })
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /no download for linux-x86_64-engine/)
  assert.equal(apt, null)
  assert.equal(engineInstalled, false)
})

test("Linux download installer installs the verified Debian package with its dependencies", t => {
  const { result, apt } = install(t)
  assert.equal(result.status, 0, result.stderr)
  assert.match(apt, /^install\n-y\n.*Yougori\.deb\n$/)
  assert.match(result.stdout, /1\.0\.0 is installed/)
})

test("Linux download installer stops before installation on a hash or package format mismatch", t => {
  for (const options of [{ badHash: true }, { extension: "AppImage" }]) {
    const { result, apt } = install(t, options)
    assert.notEqual(result.status, 0)
    assert.equal(apt, null)
    assert.match(result.stderr, /SHA-256|expected a \.deb/)
  }
})

test("Linux download installer reports package manager failures", t => {
  const { result } = install(t, { aptExit: "42" })
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /package installation failed/)
  assert.doesNotMatch(result.stdout, /is installed/)
})
