// Writes the files yougori.com serves for installs and updates, into artifacts/website/:
//   releases/latest.json   version, notes and each asset's HTTPS URL and SHA-256
//   install.ps1            Windows installer, pinned to the publisher that signed this release
//   install.sh             macOS/Linux installer
// Usage:
//   node scripts/release-manifest.mjs --base-url https://yougori.com/releases/1.2.0 [--notes TEXT] \
//     windows-x86_64=path/Yougori_1.2.0_x64-setup.exe windows-x86_64-engine=artifacts/engine/….zip …
// Asset keys are PLATFORM or PLATFORM-engine, PLATFORM being windows|macos|linux-x86_64|aarch64.
// Upload each asset to BASE_URL/FILE_NAME, then copy artifacts/website/ to the website unchanged.
import { execFileSync, spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { basename, dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { checkCompliance } from "./compliance-check.mjs"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const KEY = /^(windows|macos|linux)-(x86_64|aarch64)(-engine)?$/

export function parseArgs(argv) {
  const options = { baseUrl: "", notes: "", assets: {} }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === "--base-url") options.baseUrl = argv[++i] ?? ""
    else if (arg === "--notes") options.notes = argv[++i] ?? ""
    else {
      const [key, path] = arg.split(/=(.*)/s)
      if (!KEY.test(key) || !path) throw new Error(`Expected PLATFORM=PATH, got ${arg}`)
      options.assets[key] = path
    }
  }
  if (!/^https:\/\/[^\s]+$/.test(options.baseUrl)) throw new Error("--base-url must be an HTTPS URL")
  if (!Object.keys(options.assets).length) throw new Error("Give at least one PLATFORM=PATH asset")
  options.baseUrl = options.baseUrl.replace(/\/+$/, "")
  return options
}

export function sourceMetadata(report, commit = report.sourceCommit) {
  const publication = report.publication
  if (publication?.status !== "verified" || !/^https:\/\/[^\s]+$/.test(publication.bundleUrl ?? "")
    || !/^[a-f0-9]{64}$/.test(publication.bundleSha256 ?? "") || !/^[a-f0-9]{40}$/.test(commit ?? "")) {
    throw new Error("Release downloads need verified matching source: publication.bundleUrl, publication.bundleSha256 and sourceCommit")
  }
  return { url: publication.bundleUrl, sha256: publication.bundleSha256, commit }
}

export function manifest(version, notes, baseUrl, files, source) {
  if (!source?.url || !source.sha256 || !source.commit) throw new Error("Release manifest requires matching source download metadata")
  const assets = {}
  for (const [key, { name, sha256 }] of Object.entries(files)) assets[key] = { url: `${baseUrl}/${encodeURIComponent(name)}`, sha256 }
  return { version, notes, assets, source }
}

/** The Windows script refuses to install anything not signed by exactly this subject. */
export function pinPublisher(script, subject) {
  const placeholder = "$ExpectedPublisher = '__YOUGORI_PUBLISHER__'"
  if (!script.includes(placeholder)) throw new Error("install.ps1 has no publisher placeholder")
  if (!subject.trim()) throw new Error("The release signer has no subject")
  return script.replace(placeholder, () => `$ExpectedPublisher = '${subject.replace(/'/g, "''")}'`)
}

export function windowsSigner(path) {
  const script = "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false); Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security/Microsoft.PowerShell.Security.psd1'); $s = Get-AuthenticodeSignature -LiteralPath $env:YOUGORI_RELEASE_SIGNED_FILE; if ($s.Status -ne 'Valid' -or -not $s.TimeStamperCertificate) { exit 3 }; $s.SignerCertificate.Subject"
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
    env: { ...process.env, YOUGORI_RELEASE_SIGNED_FILE: resolve(path) }, encoding: "utf8", windowsHide: true,
  })
  if (result.status !== 0) throw new Error(`${path} needs a trusted, timestamped signature before publication`)
  return result.stdout.trim()
}

async function main() {
  const options = parseArgs(process.argv.slice(2))
  await checkCompliance(root, { distribution: true })
  if (execFileSync("git", ["status", "--porcelain"], { cwd: root, encoding: "utf8", windowsHide: true }).trim()) {
    throw new Error("Commit the reviewed release sources before generating public download metadata")
  }
  const commit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8", windowsHide: true }).trim()
  const source = sourceMetadata(JSON.parse(readFileSync(join(root, "compliance/release.json"), "utf8")), commit)
  const version = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version
  const files = {}
  let publisher = ""
  for (const [key, path] of Object.entries(options.assets)) {
    const bytes = readFileSync(path)
    files[key] = { name: basename(path), sha256: createHash("sha256").update(bytes).digest("hex") }
    if (key.startsWith("windows-") && path.toLowerCase().endsWith(".exe")) {
      const subject = windowsSigner(path)
      if (publisher && publisher !== subject) throw new Error("Windows assets are signed by different publishers")
      publisher = subject
    }
  }
  const out = join(root, "artifacts/website")
  mkdirSync(join(out, "releases"), { recursive: true })
  const windows = Object.keys(files).some(key => key.startsWith("windows-"))
  if (windows) {
    if (!publisher) throw new Error("Include the signed Windows installer (.exe) so install.ps1 can pin its publisher")
    writeFileSync(join(out, "install.ps1"), pinPublisher(readFileSync(join(root, "scripts/install/install.ps1"), "utf8"), publisher))
  }
  copyFileSync(join(root, "scripts/install/install.sh"), join(out, "install.sh"))
  // Publish the manifest last, after the Windows publisher has been verified and pinned.
  writeFileSync(join(out, "releases/latest.json"), JSON.stringify({
    ...manifest(version, options.notes, options.baseUrl, files, source), channel: "production",
  }, null, 2) + "\n")
  console.log(JSON.stringify({ version, publisher: publisher || null, folder: out, assets: Object.keys(files) }))
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1 })
}
