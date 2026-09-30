import assert from "node:assert/strict"
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { manifest, parseArgs, pinPublisher, sourceMetadata, windowsSigner } from "./release-manifest.mjs"
import { enginePlatform, resourceMap } from "./package-engine.mjs"

const root = join(dirname(fileURLToPath(import.meta.url)), "..")

test("release arguments need an HTTPS base and known platform keys", () => {
  const options = parseArgs(["--base-url", "https://yougori.com/releases/1.2.0/", "--notes", "n", "windows-x86_64=a.exe", "linux-x86_64-engine=b.tar.gz"])
  assert.equal(options.baseUrl, "https://yougori.com/releases/1.2.0")
  assert.deepEqual(options.assets, { "windows-x86_64": "a.exe", "linux-x86_64-engine": "b.tar.gz" })
  assert.throws(() => parseArgs(["--base-url", "http://x", "windows-x86_64=a.exe"]), /HTTPS/)
  assert.throws(() => parseArgs(["--base-url", "https://x", "windows=a.exe"]), /PLATFORM=PATH/)
  assert.throws(() => parseArgs(["--base-url", "https://x"]), /at least one/)
})

test("the manifest lists each asset's URL and hash in the shape the CLI and installers read", () => {
  const source = { url: "https://y.com/source.zip", sha256: "a".repeat(64), commit: "b".repeat(40) }
  const value = manifest("1.2.0", "notes", "https://y.com/r", { "windows-x86_64-engine": { name: "yougori engine.zip", sha256: "ab" } }, source)
  assert.deepEqual(value, { version: "1.2.0", notes: "notes", assets: { "windows-x86_64-engine": { url: "https://y.com/r/yougori%20engine.zip", sha256: "ab" } }, source })
  assert.throws(() => manifest("1", "", "https://y.com", {}), /requires matching source/)
})

test("release source links require completed publication and pinned source identity", () => {
  const report = { sourceCommit: "b".repeat(40), publication: { status: "verified", bundleUrl: "https://y.com/source.zip", bundleSha256: "a".repeat(64) } }
  assert.equal(sourceMetadata(report).commit, report.sourceCommit)
  assert.throws(() => sourceMetadata({ ...report, publication: { ...report.publication, status: "not-published" } }), /verified matching source/)
  assert.throws(() => sourceMetadata({ ...report, publication: { ...report.publication, bundleSha256: "" } }), /verified matching source/)
})

test("install.ps1 is pinned to the exact release signer and refuses to run unpinned", () => {
  const script = readFileSync(join(root, "scripts/install/install.ps1"), "utf8")
  const pinned = pinPublisher(script, "CN=Yougori O'Brien, O=Yougori, C=US")
  assert.match(pinned, /\$ExpectedPublisher = 'CN=Yougori O''Brien, O=Yougori, C=US'/)
  assert.match(pinned, /-eq \('__YOUGORI_' \+ 'PUBLISHER__'\)/)
  assert.match(pinned, /SignerCertificate\.Subject -eq \$ExpectedPublisher/)
  assert.throws(() => pinPublisher(pinned, "CN=Other"), /placeholder/)
})

test("engine packages use the desktop bundle's own resource map", () => {
  assert.equal(enginePlatform("win32", "x64"), "windows-x86_64-engine")
  assert.equal(enginePlatform("darwin", "arm64"), "macos-aarch64-engine")
  assert.throws(() => enginePlatform("linux", "arm64"), /Supported desktop builds/)
  assert.throws(() => enginePlatform("sunos", "x64"))
  const windows = resourceMap("win32")
  assert.equal(windows["resources/runtime/"], "runtime/")
  assert.equal(windows["resources/cli/yougori.exe"], "cli/yougori.exe")
  assert.equal(windows["resources/vault/yougori-vault.exe"], "vault/yougori-vault.exe")
  assert.equal(resourceMap("linux")["resources/cli/yougori"], "cli/yougori")
})

test("Windows release verification handles paths with spaces and refuses unsigned payloads", { skip: process.platform !== "win32" }, () => {
  const folder = mkdtempSync(join(tmpdir(), "yougori-release-signature-"))
  try {
    const signed = join(folder, "signed executable with spaces.exe")
    copyFileSync(join(process.env.SystemRoot ?? "C:\\Windows", "System32/cmd.exe"), signed)
    assert.match(windowsSigner(signed), /O=Microsoft Corporation/)
    const unsigned = join(folder, "unsigned executable with spaces.exe")
    writeFileSync(unsigned, "Unsigned test fixture")
    assert.throws(() => windowsSigner(unsigned), /trusted, timestamped signature/)
  } finally { rmSync(folder, { recursive: true, force: true }) }
})
