import { spawnSync } from "node:child_process"

// Check the desktop and port before building or starting another Tauri dev
// session. The predev hook repeats the port check immediately before Vite.
if (process.platform === "win32") {
  const desktop = process.argv.includes("--desktop")
  const preflights = desktop ? [["-CheckDesktopOnly"], []] : [[]]
  for (const extra of preflights) {
    const result = spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/ensure-dev-port.ps1", ...extra], { stdio: "inherit", windowsHide: true })
    if (result.error) throw result.error
    if (result.status !== 0) process.exit(result.status ?? 1)
  }
  if (desktop) {
    // Tauri dev builds the engine from current Rust sources. Refresh the public
    // CLI before exposing it on PATH so both sides use the same control pipe.
    const bundle = spawnSync(process.execPath, ["scripts/bundle-cli.mjs", "--dev"], { stdio: "inherit", windowsHide: true })
    if (bundle.error) throw bundle.error
    if (bundle.status !== 0) {
      process.exitCode = bundle.status ?? 1
      process.exit()
    }
    const cli = spawnSync("powershell", [
      "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/install-cli-path.ps1",
      "-InstallDirectory", "src-tauri/resources",
    ], { stdio: "inherit", windowsHide: true })
    if (cli.error) throw cli.error
    process.exitCode = cli.status ?? 1
    if (cli.status === 0) console.log("Yougori CLI registered for this user. Reopen Windows Terminal or PowerShell, then run yougori for all commands.")
  }
}
