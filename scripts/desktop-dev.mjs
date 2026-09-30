import { spawn, spawnSync } from "node:child_process"
import { existsSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { stripVTControlCharacters } from "node:util"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")

// Keep attachment separate from Tauri's build/run lifecycle. A second Tauri
// process exits after showing the existing window and takes its Vite with it.
export async function launchDesktopDev(io, args = []) {
  const running = await io.status()
  if (!running) return io.fresh(args)

  if (running.engineOnly) {
    // The standalone engine can hand over only when idle. Its backend makes
    // that decision; never implement it here by stopping guests or killing it.
    const result = await io.show()
    if (!result.handover) throw new Error("The running engine did not hand over to the desktop.")
    await io.waitForExit(running.pid, 90_000)
    return io.fresh(args)
  }

  const runtime = await io.inspect(running.pid)
  let frontend
  try {
    if (runtime.development) {
      if (runtime.frontend === "other") {
        throw new Error("Port 1420 belongs to another server. Free that port before opening the development dashboard.")
      }
      if (runtime.frontend === "available") frontend = await io.startFrontend()
    }
    // Raw app_show addresses this engine without starting a replacement if it
    // exits during attachment. Headless requests leave its window hidden.
    if (!args.includes("--headless")) await io.show()
    io.log(`Reusing Yougori (PID ${running.pid}). Its workloads keep running.`)
    io.log("The existing native build is in use. Restart Yougori when you need to apply Rust changes.")
    if (frontend) {
      io.log("Frontend hot reload is ready. Keep this terminal open; Ctrl+C stops this frontend server.")
      await io.waitForExit(running.pid)
    }
    return 0
  } finally {
    await frontend?.close()
  }
}

function capture(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", windowsHide: true, timeout: 30_000 })
  if (result.error) throw result.error
  return result
}

function powershell(script, args = []) {
  const result = capture("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(root, "scripts", script), ...args])
  if (result.status !== 0) throw new Error(result.stderr.trim() || result.stdout.trim() || `${script} failed`)
  return JSON.parse(result.stdout)
}

function runNode(script, args = []) {
  return new Promise((accept, reject) => {
    const child = spawn(process.execPath, [join(root, script), ...args], { cwd: root, stdio: "inherit", windowsHide: true })
    child.once("error", reject)
    child.once("exit", code => accept(code ?? 1))
  })
}

function adapters() {
  const cli = join(root, "src-tauri/resources/cli/yougori-cli.exe")
  let interrupted = false
  let frontendFailure
  const onInterrupt = () => { interrupted = true }
  const call = args => {
    const result = capture(cli, args)
    let reply
    try { reply = JSON.parse(result.stdout) } catch { throw new Error(result.stderr.trim() || "Yougori CLI returned an invalid response.") }
    if (result.status !== 0 || !reply.ok) throw new Error(reply.error || "Yougori CLI request failed.")
    return reply.result
  }
  return {
    log: console.log,
    // The Windows preflight is the guard that needs attachment. Preserve the
    // existing native launcher on other platforms.
    status: () => {
      if (process.platform !== "win32" || !existsSync(cli)) return null
      try { return call(["app", "status"]) } catch {
        // Fresh launch still runs the process guard: an unreachable existing
        // engine is never assumed safe to replace.
        return null
      }
    },
    show: () => call(["call", "app_show"]),
    inspect: pid => powershell("inspect-dev-runtime.ps1", ["-RuntimeProcessId", String(pid)]),
    waitForExit: async (pid, timeoutMs = Infinity) => {
      const deadline = Date.now() + timeoutMs
      while (!interrupted) {
        if (frontendFailure) throw frontendFailure
        if (Date.now() >= deadline) throw new Error("The background engine did not finish handing over to the desktop.")
        try { process.kill(pid, 0) } catch (error) {
          if (error.code === "ESRCH") return
          throw error
        }
        await new Promise(accept => setTimeout(accept, 500))
      }
    },
    startFrontend: async () => {
      const patched = await runNode("scripts/patch-novnc.mjs")
      if (patched !== 0) throw new Error("Could not prepare the frontend dependencies.")
      // Use an absolute CLI path so another launch can verify this server's
      // workspace through process inspection, even when npm used a relative path.
      const server = spawn(process.execPath, [join(root, "node_modules/vite/bin/vite.js"), "--port", "1420", "--strictPort"], {
        cwd: root, stdio: ["ignore", "pipe", "pipe"], windowsHide: true,
      })
      const exited = new Promise(accept => server.once("exit", accept))
      server.stderr.on("data", data => process.stderr.write(data))
      server.on("exit", code => { frontendFailure = new Error(`The frontend server exited (${code ?? "signal"}).`) })
      let timer
      try {
        await new Promise((accept, reject) => {
          timer = setTimeout(() => reject(new Error("The frontend server did not become ready.")), 30_000)
          let output = ""
          server.stdout.on("data", data => {
            process.stdout.write(data)
            output = (output + data).slice(-4096)
            if (/ready in\s+\d+\s*ms/.test(stripVTControlCharacters(output))) accept()
          })
          server.once("error", reject)
          server.once("exit", code => reject(new Error(`The frontend server exited before startup (${code ?? "signal"}).`)))
        })
      } catch (error) {
        server.kill()
        throw error
      } finally { clearTimeout(timer) }
      process.on("SIGINT", onInterrupt)
      process.on("SIGTERM", onInterrupt)
      return { close: async () => {
        process.off("SIGINT", onInterrupt)
        process.off("SIGTERM", onInterrupt)
        server.kill()
        await exited
      } }
    },
    fresh: async args => {
      const prepared = await runNode("scripts/prepare-dev.mjs", ["--desktop"])
      if (prepared !== 0) return prepared
      return runNode("node_modules/@tauri-apps/cli/tauri.js", ["dev", ...args])
    },
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.exitCode = await launchDesktopDev(adapters(), process.argv.slice(2)) } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
