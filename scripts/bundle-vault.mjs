import { spawnSync } from "node:child_process"
import { copyFileSync, mkdirSync, readFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { signWindowsPayload } from "./windows-payload-signing.mjs"
import { assertPortableCli, nativeTarget, portableCliBuildEnv, releaseTarget } from "./release-preflight.mjs"

export function bundleVault({
  root = resolve(dirname(fileURLToPath(import.meta.url)), ".."),
  platform = process.platform,
  arch = process.arch,
  env = process.env,
  run = spawnSync,
} = {}) {
  if (platform !== "win32") return
  const target = releaseTarget(env, platform, arch) || nativeTarget(platform, arch)
  const buildEnv = portableCliBuildEnv(env, platform)
  const result = run("cargo", ["build", "--locked", "--release", "--bin", "yougori-vault", "--target", target,
    "--manifest-path", join(root, "vault/Cargo.toml")], { cwd: root, env: buildEnv, stdio: "inherit", windowsHide: true })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`Personal Vault build failed (exit ${result.status ?? "unknown"})`)
  const metadata = run("cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1", "--manifest-path", join(root, "vault/Cargo.toml")],
    { cwd: root, env: buildEnv, encoding: "utf8", windowsHide: true })
  if (metadata.error) throw metadata.error
  if (metadata.status !== 0) throw new Error(metadata.stderr || "Cannot resolve Personal Vault Cargo target directory")
  const binary = join(JSON.parse(metadata.stdout).target_directory, target, "release/yougori-vault.exe")
  assertPortableCli(readFileSync(binary), "Personal Vault")
  const destination = join(root, "src-tauri/resources/vault")
  mkdirSync(destination, { recursive: true })
  copyFileSync(binary, join(destination, "yougori-vault.exe"))
  signWindowsPayload(root, join(destination, "yougori-vault.exe"), env, run)
  // Keep the unpackaged development release executable usable as well.
  const standalone = join(root, "src-tauri/target/release/vault")
  mkdirSync(standalone, { recursive: true })
  copyFileSync(join(destination, "yougori-vault.exe"), join(standalone, "yougori-vault.exe"))
  console.log("Bundled protected native Personal Vault broker")
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) bundleVault()
