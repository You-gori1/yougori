import { spawnSync } from "node:child_process"
import { join } from "node:path"

export function signWindowsPayload(root, path, env = process.env, run = spawnSync) {
  if (!env.YOUGORI_SIGN_CERT_SHA1) return
  const result = run("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
    "-File", join(root, "scripts/sign-windows-release.ps1"), "-Action", "Sign", "-Path", path],
  { env, stdio: "inherit", windowsHide: true })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error("Release payload signing failed; packaging was stopped")
}
