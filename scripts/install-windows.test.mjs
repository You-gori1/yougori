import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { readFileSync } from "node:fs"
import test from "node:test"
import { pinPublisher } from "./release-manifest.mjs"

// Execute only platform selection, before any network, files, PATH or installs.
const script = pinPublisher(readFileSync(new URL("./install/install.ps1", import.meta.url), "utf8"), "CN=Installer Test")
const boundary = script.indexOf("Write-Host 'Finding the latest Yougori release...'")
assert.ok(boundary > 0)
const selection = script.slice(0, boundary) + '\nWrite-Output $platform\n'

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
