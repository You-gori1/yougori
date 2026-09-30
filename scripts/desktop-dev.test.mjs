import assert from "node:assert/strict"
import test from "node:test"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { launchDesktopDev } from "./desktop-dev.mjs"

function harness({ running = { pid: 333, headless: true, engineOnly: false }, runtime = { development: true, frontend: "available" }, showError, handover = false, startupError } = {}) {
  const calls = []
  return {
    calls,
    io: {
      status: async () => running,
      inspect: async pid => { calls.push(["inspect", pid]); return runtime },
      startFrontend: async () => {
        calls.push(["frontend"])
        if (startupError) throw new Error(startupError)
        return { close: async () => calls.push(["close-frontend"]) }
      },
      show: async () => {
        calls.push(["show"])
        if (showError) throw new Error(showError)
        return { visible: !handover, handover }
      },
      waitForExit: async (pid, timeout) => calls.push(["wait", pid, timeout]),
      fresh: async args => { calls.push(["fresh", args]); return 0 },
      log: () => {},
    },
  }
}

test("CLI-started desktop gets a frontend before its dashboard opens; its engine is reused", async () => {
  const { io, calls } = harness()
  assert.equal(await launchDesktopDev(io), 0)
  assert.deepEqual(calls, [["inspect", 333], ["frontend"], ["show"], ["wait", 333, undefined], ["close-frontend"]])
})

test("repeated desktop:dev focuses the running app without replacing its Vite or native process", async () => {
  const { io, calls } = harness({ runtime: { development: true, frontend: "workspace" } })
  assert.equal(await launchDesktopDev(io), 0)
  assert.deepEqual(calls, [["inspect", 333], ["show"]])
})

test("an installed desktop opens without starting a development server", async () => {
  const { io, calls } = harness({ runtime: { development: false, frontend: "available" } })
  await launchDesktopDev(io)
  assert.deepEqual(calls, [["inspect", 333], ["show"]])
})

test("a fresh launch preserves Tauri arguments and builds normally", async () => {
  const { io, calls } = harness({ running: null })
  const args = ["--", "--", "--headless"]
  await launchDesktopDev(io, args)
  assert.deepEqual(calls, [["fresh", args]])
})

test("headless reattachment does not open a window", async () => {
  const { io, calls } = harness()
  await launchDesktopDev(io, ["--", "--", "--headless"])
  assert.equal(calls.some(([name]) => name === "show"), false)
  assert.equal(calls.some(([name]) => name === "frontend"), true)
})

test("an unrelated listener is preserved and never displayed in the app", async () => {
  const { io, calls } = harness({ runtime: { development: true, frontend: "other" } })
  await assert.rejects(launchDesktopDev(io), /belongs to another server/)
  assert.deepEqual(calls, [["inspect", 333]])
})

test("frontend startup failure never opens a dashboard with no server", async () => {
  const { io, calls } = harness({ startupError: "address in use" })
  await assert.rejects(launchDesktopDev(io), /address in use/)
  assert.deepEqual(calls, [["inspect", 333], ["frontend"]])
})

test("a failed dashboard request cleans up only the frontend this invocation started", async () => {
  const { io, calls } = harness({ showError: "engine exited" })
  await assert.rejects(launchDesktopDev(io), /engine exited/)
  assert.deepEqual(calls, [["inspect", 333], ["frontend"], ["show"], ["close-frontend"]])
})

test("an idle standalone engine hands over before a fresh native build", async () => {
  const { io, calls } = harness({ running: { pid: 333, engineOnly: true }, handover: true })
  await launchDesktopDev(io)
  assert.deepEqual(calls, [["show"], ["wait", 333, 90_000], ["fresh", []]])
})

test("a busy standalone engine's refusal is honored without building or stopping it", async () => {
  const { io, calls } = harness({ running: { pid: 333, engineOnly: true }, showError: "engine is running environments" })
  await assert.rejects(launchDesktopDev(io), /running environments/)
  assert.deepEqual(calls, [["show"]])
})

test("Windows inspection distinguishes this checkout's Vite from unrelated listeners without stopping anything", { skip: process.platform !== "win32" }, () => {
  const command = `
    $ErrorActionPreference = 'Stop'
    function Get-Process { param($Id, $ErrorAction); [pscustomobject]@{ Path='C:\\repo\\src-tauri\\target\\debug\\yougori.exe' } }
    function Stop-Process { throw 'Inspection must not stop processes' }
    function Get-NetTCPConnection {
      param($LocalPort, $State, $ErrorAction)
      if ($env:YOUGORI_INSPECT_CASE -ne 'empty') { [pscustomobject]@{ OwningProcess=111 } }
    }
    function Get-CimInstance {
      param($ClassName, $Filter, $ErrorAction)
      $root = $env:YOUGORI_INSPECT_ROOT
      switch ($env:YOUGORI_INSPECT_CASE) {
        'workspace' { $line = 'node "' + $root + '\\node_modules\\vite\\bin\\vite.js"' }
        'sibling' { $line = 'node "' + $root + '-other\\node_modules\\vite\\bin\\vite.js"' }
        default { $line = 'node "C:\\unrelated\\server.js"' }
      }
      [pscustomobject]@{ Name='node.exe'; CommandLine=$line }
    }
    & $env:YOUGORI_INSPECT_SCRIPT -RuntimeProcessId 333
  `
  for (const [scenario, frontend] of [["empty", "available"], ["workspace", "workspace"], ["sibling", "other"], ["unrelated", "other"]]) {
    const reply = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
      encoding: "utf8", timeout: 15_000, windowsHide: true,
      env: {
        ...process.env,
        YOUGORI_INSPECT_CASE: scenario,
        YOUGORI_INSPECT_ROOT: fileURLToPath(new URL("../", import.meta.url)).replace(/[\\/]$/, ""),
        YOUGORI_INSPECT_SCRIPT: fileURLToPath(new URL("inspect-dev-runtime.ps1", import.meta.url)),
      },
    })
    assert.equal(reply.status, 0, reply.stderr)
    assert.deepEqual(JSON.parse(reply.stdout), { development: true, frontend })
  }
})
