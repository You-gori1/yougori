import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { pinPublisher } from "./release-manifest.mjs"

// Execute only platform selection, before any network, files, PATH or installs.
const script = pinPublisher(readFileSync(new URL("./install/install.ps1", import.meta.url), "utf8"), "CN=Installer Test")
const boundary = script.indexOf("Write-Host 'Finding the latest Yougori release...'")
assert.ok(boundary > 0)
const selection = script.slice(0, boundary) + '\nWrite-Output $platform\n'

for (const exit of [0, 1]) test(`Windows skill setup uses the installed CLI and tolerates a conflict (exit ${exit})`, { skip: process.platform !== "win32" }, t => {
  const root = mkdtempSync(join(tmpdir(), "yougori skill setup "))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const cli = join(root, 'installed cli.ps1')
  const log = join(root, 'arguments.txt')
  writeFileSync(cli, `param($group, $verb)\n[IO.File]::WriteAllText($env:FIXTURE_SKILL_LOG, "$group $verb")\nWrite-Output '{"ok":true,"result":{"path":"fixture skill"}}'\nexit ${exit}\n`)
  const start = script.indexOf("Write-Host 'Setting up the Yougori skill...'")
  const end = script.indexOf("if ($env:YOUGORI_AUTOSTART", start)
  assert.ok(start > 0 && end > start)
  const fixture = `$ErrorActionPreference = 'Stop'; $cli = [PSCustomObject]@{ Source = $env:FIXTURE_SKILL_CLI };\n${script.slice(start,end)}\nWrite-Output 'INSTALL_COMPLETED'`
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(fixture,'utf16le').toString('base64')], {
    encoding:'utf8', windowsHide:true, timeout:15000,
    env:{...process.env, FIXTURE_SKILL_CLI:cli, FIXTURE_SKILL_LOG:log},
  })
  assert.equal(result.status, 0, result.stderr)
  assert.equal(readFileSync(log,'utf8'), 'skills install')
  assert.match(result.stdout, /INSTALL_COMPLETED/)
  assert.match(result.stdout + result.stderr, exit === 0 ? /Yougori skill ready: fixture skill/ : /existing custom skills were preserved/)
})

test("Windows command downloads preserve the URL query and add command attribution", { skip: process.platform !== "win32" }, () => {
  const start = script.indexOf('# The server saves aggregate command/app totals')
  const end = script.indexOf('\n$hash =', start)
  assert.ok(start > 0 && end > start)
  const fragment = script.slice(start, end)
  for (const [url, expected] of [
    ['https://fixture.invalid/setup.exe', 'https://fixture.invalid/setup.exe?source=command'],
    ['https://fixture.invalid/setup.exe?download=1#fragment', 'https://fixture.invalid/setup.exe?download=1&source=command'],
  ]) {
    const fixture = `$asset = [PSCustomObject]@{ url = '${url}' }; $download = 'unused'; function Invoke-WebRequest { param($Uri, $OutFile, [switch]$UseBasicParsing); Write-Output $Uri }; function Fail($message) { throw $message };\n${fragment}`
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(fixture, 'utf16le').toString('base64')], { encoding: 'utf8', windowsHide: true, timeout: 15000 })
    assert.equal(result.status, 0, result.stderr)
    assert.equal(result.stdout.trim(), expected)
  }
})

for (const [arch, platform] of [["AMD64", "x86_64"], ["ARM64", "aarch64"]]) {
  for (const override of [null, "", "1", "0"]) {
    test(`Windows ${arch} selects the engine by default (override ${override ?? "unset"})`, { skip: process.platform !== "win32" }, () => {
      const env = { ...process.env, PROCESSOR_ARCHITECTURE: arch, YOUGORI_ENGINE_ONLY: override, YOUGORI_ALLOW_UNSIGNED: "" }
      if (override === null) delete env.YOUGORI_ENGINE_ONLY
      const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(selection, "utf16le").toString("base64")], {
        encoding: "utf8", windowsHide: true, timeout: 15000, env,
      })
      if (result.error) throw result.error
      assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout.trim(), `windows-${platform}${override === "0" ? "" : "-engine"}`)
    })
  }
}
