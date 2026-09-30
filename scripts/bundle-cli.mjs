import { spawnSync } from "node:child_process"
import { chmodSync, copyFileSync, mkdirSync, readFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { signWindowsPayload } from "./windows-payload-signing.mjs"
import { assertPortableCli, assertMacCli, portableCliBuildEnv, releaseTarget, nativeTarget } from "./release-preflight.mjs"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const development = process.argv.includes("--dev")
const target = development ? process.env.CARGO_BUILD_TARGET || null : releaseTarget() || nativeTarget()
const args = ["build", "--locked", ...(development ? [] : ["--release"]), "--manifest-path", join(root, "cli/Cargo.toml")]
if (target) args.push("--target", target)
const result = spawnSync("cargo", args, { stdio: "inherit", cwd: root, windowsHide: true, env: development ? process.env : portableCliBuildEnv() })
if (result.error) throw result.error
if (result.status !== 0) process.exit(result.status ?? 1)
const name = process.platform === "win32" ? "yougori.exe" : "yougori"
// Cargo config can override target-dir independently of CARGO_TARGET_DIR.
const metadata = spawnSync("cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1", "--manifest-path", join(root, "cli/Cargo.toml")], { encoding: "utf8", cwd: root, windowsHide: true })
if (metadata.error) throw metadata.error
if (metadata.status !== 0) throw new Error(metadata.stderr || "Cannot resolve Cargo target directory")
const targetRoot = JSON.parse(metadata.stdout).target_directory
const destination = join(root, "src-tauri/resources/cli")
mkdirSync(destination, { recursive: true })
const binary = development
  ? target ? join(targetRoot, target, "debug", name) : join(targetRoot, "debug", name)
  : join(targetRoot, target, "release", name)
if (development) {
  if (!readFileSync(binary).length) throw new Error("The development CLI is empty")
} else if (process.platform === "win32") assertPortableCli(readFileSync(binary))
else if (process.platform === "darwin") assertMacCli(readFileSync(binary), process.arch)
else {
  const elf = readFileSync(binary)
  if (elf.toString("hex", 0, 4) !== "7f454c46" || elf[4] !== 2 || elf.readUInt16LE(18) !== 62) throw new Error("Expected an x86-64 Linux ELF CLI")
}
copyFileSync(binary, join(destination, name))
if (process.platform === "win32" && !development) signWindowsPayload(root, join(destination, name))
const compatibilityName = process.platform === "win32" ? "yougori-cli.exe" : "yougori-cli"
copyFileSync(join(destination, name), join(destination, compatibilityName))
// Keep existing agent skills and user scripts working after the rename.
const legacyName = process.platform === "win32" ? "opendock-cli.exe" : "opendock-cli"
copyFileSync(join(destination, name), join(destination, legacyName))
if (process.platform !== "win32") for (const file of [name, compatibilityName, legacyName]) chmodSync(join(destination, file), 0o755)
console.log(`Bundled ${development ? "development " : ""}${name} (no Node.js dependency at runtime)`)
