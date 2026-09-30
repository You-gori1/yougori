import { spawnSync } from "node:child_process"
import { accessSync, constants } from "node:fs"
import { resolve } from "node:path"
import { pathToFileURL } from "node:url"
import { nativeTarget } from "./release-preflight.mjs"

export function macosPaths(arch) {
  nativeTarget("darwin", arch)
  const prefix = arch === "arm64" ? "/opt/homebrew" : "/usr/local"
  const qemu = `${prefix}/opt/qemu`
  return { brew: `${prefix}/bin/brew`, qemu: `${qemu}/bin/qemu-system-x86_64`,
    image: `${qemu}/bin/qemu-img`, cloudflared: `${prefix}/opt/cloudflared/bin/cloudflared`,
    code: `${qemu}/share/qemu/edk2-x86_64-code.fd`, vars: `${qemu}/share/qemu/edk2-i386-vars.fd` }
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000, maxBuffer: 2 * 1024 * 1024 })
  if (result.error || result.status !== 0) throw new Error(`${command}: ${result.error?.message || result.stderr || "failed"}`)
  return result.stdout.trim()
}

export function checkMacos({ install = false } = {}) {
  if (process.platform !== "darwin") throw new Error("Run this step on the borrowed Mac, not Windows or WSL. Source changes and portable tests can be done here first.")
  if (process.getuid?.() === 0) throw new Error("Run Yougori setup/build as your normal Mac user, never sudo.")
  const paths = macosPaths(process.arch)
  const version = run("/usr/bin/sw_vers", ["-productVersion"])
  if (Number(version.split(".")[0]) < 14) throw new Error("The macOS preview targets macOS 14 or newer.")
  const translated = spawnSync("/usr/sbin/sysctl", ["-in", "sysctl.proc_translated"], { encoding: "utf8", timeout: 5000 })
  if (translated.stdout?.trim() === "1") throw new Error("This terminal/Node is running under Rosetta. Use native Apple Silicon Terminal, Node, Rust and Homebrew.")
  try { accessSync(paths.brew, constants.X_OK) } catch {
    throw new Error(`Native Homebrew is missing at ${paths.brew}. Install it from https://brew.sh, then retry. This script does not download/execute an installer or request sudo.`)
  }
  if (install) {
    console.log("Installing macOS runtime dependencies: QEMU and Cloudflare Tunnel. No VM data, host permissions or security settings are changed.")
    const result = spawnSync(paths.brew, ["install", "qemu", "cloudflared"], { stdio: "inherit" })
    if (result.error || result.status !== 0) throw new Error(result.error?.message || "Homebrew installation failed")
  }
  for (const name of ["qemu", "image", "cloudflared"]) {
    try { accessSync(paths[name], constants.X_OK) } catch {
      throw new Error(`Missing executable: ${paths[name]}. Run npm run macos:setup (or brew install qemu cloudflared).`)
    }
    const description = run("/usr/bin/file", ["-L", paths[name]])
    const expected = process.arch === "arm64" ? "arm64" : "x86_64"
    if (!description.includes("Mach-O") || !description.includes(expected)) throw new Error(`${name} is not a native ${expected} macOS executable: ${description}`)
  }
  for (const name of ["code", "vars"]) {
    try { accessSync(paths[name], constants.R_OK) } catch { throw new Error(`QEMU UEFI firmware is missing: ${paths[name]}. Reinstall Homebrew qemu; do not substitute unrelated firmware.`) }
  }
  // Homebrew signs QEMU for Apple's hypervisor/JIT requirements. A damaged
  // signature is a reinstall problem, never a reason to disable Gatekeeper.
  run("/usr/bin/codesign", ["--verify", "--strict", paths.qemu])
  const machines = run(paths.qemu, ["-machine", "help"])
  for (const board of ["q35", "microvm"]) if (!machines.includes(board)) throw new Error(`Installed QEMU lacks the ${board} machine`)
  const accelerators = run(paths.qemu, ["-accel", "help"])
  if (!accelerators.includes("tcg")) throw new Error("Installed QEMU lacks the software fallback")
  console.log(`macOS ${version} · ${nativeTarget()}\n${run(paths.qemu, ["--version"]).split("\n")[0]}\n${run(paths.image, ["--version"]).split("\n")[0]}\n${run(paths.cloudflared, ["--version"])}`)
  console.log(process.arch === "arm64"
    ? "Apple Silicon: x86-64 guests use slower software emulation. ARM64 guest images and GPU/CUDA are not supported by this preview."
    : "Intel: containers/full VMs try HVF then software fallback. MicroVMs use software emulation. GPU/CUDA is unavailable.")
  console.log("Dependencies checked. This does not certify guest boot, interactive GUI, signing or notarization. See docs/macos.md.")
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.slice(2).some(arg => arg !== "--install")) throw new Error("Usage: node scripts/macos-setup.mjs [--install]")
    checkMacos({ install: process.argv.includes("--install") })
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
