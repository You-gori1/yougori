import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import test from "node:test"

// Mock process/network inspection and termination inside a separate PowerShell.
// These tests never inspect or terminate real listeners, desktops or guests.
function runCase(scenario) {
  const command = `
    $ErrorActionPreference = 'Stop'
    $global:yougoriDevTestListenerQueries = 0
    $global:yougoriDevTestStopped = @()
    function Get-Process {
      param($Name, $ErrorAction)
      if ($env:YOUGORI_DEV_TEST_CASE -eq 'desktop-headless') {
        [pscustomobject]@{ Id=333; SessionId=[System.Diagnostics.Process]::GetCurrentProcess().SessionId; MainWindowHandle=0 }
      } elseif ($env:YOUGORI_DEV_TEST_CASE -eq 'desktop-other-session') {
        [pscustomobject]@{ Id=444; SessionId=([System.Diagnostics.Process]::GetCurrentProcess().SessionId + 1) }
      }
    }
    function Get-NetTCPConnection {
      param($LocalPort, $State, $ErrorAction)
      $global:yougoriDevTestListenerQueries++
      if ($global:yougoriDevTestListenerQueries -eq 1) { [pscustomobject]@{ OwningProcess = 111 } }
    }
    function Get-CimInstance {
      param($ClassName, $Filter, $ErrorAction)
      if ($Filter -eq "Name = 'yougori.exe'" -and $env:YOUGORI_DEV_TEST_CASE.StartsWith('desktop-')) {
        $session = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
        if ($env:YOUGORI_DEV_TEST_CASE -eq 'desktop-headless') {
          [pscustomobject]@{ ProcessId=333; SessionId=$session; ExecutablePath='C:\\repo\\src-tauri\\target\\debug\\yougori.exe' }
        } elseif ($env:YOUGORI_DEV_TEST_CASE -eq 'desktop-other-session') {
          [pscustomobject]@{ ProcessId=444; SessionId=($session + 1); ExecutablePath='C:\\repo\\src-tauri\\target\\debug\\yougori.exe' }
        } elseif ($env:YOUGORI_DEV_TEST_CASE -eq 'desktop-cli-model') {
          [pscustomobject]@{ ProcessId=555; SessionId=$session; ExecutablePath='C:\\repo\\src-tauri\\resources\\cli\\yougori.exe' }
        }
      } elseif ($Filter -eq 'ProcessId = 111') {
        if ($env:YOUGORI_DEV_TEST_CASE -eq 'unrelated') {
          [pscustomobject]@{ Name='node.exe'; CommandLine='C:\\unrelated\\server.js'; ParentProcessId=0; ProcessId=111 }
        } else {
          [pscustomobject]@{ Name='node.exe'; CommandLine=($env:YOUGORI_DEV_TEST_ROOT + '\\node_modules\\vite\\bin\\vite.js'); ParentProcessId=222; ProcessId=111 }
        }
      } elseif ($Filter -eq 'ProcessId = 222' -and $env:YOUGORI_DEV_TEST_CASE -eq 'active') {
        [pscustomobject]@{ Name='node.exe'; CommandLine=($env:YOUGORI_DEV_TEST_ROOT + '\\node_modules\\@tauri-apps\\cli\\tauri.js dev'); ParentProcessId=0; ProcessId=222 }
      } elseif ($Filter -eq 'ProcessId = 222' -and $env:YOUGORI_DEV_TEST_CASE -eq 'active-attachment') {
        [pscustomobject]@{ Name='node.exe'; CommandLine='node scripts/desktop-dev.mjs'; ParentProcessId=0; ProcessId=222 }
      }
    }
    function Stop-Process {
      param($Id, [switch]$Force, $ErrorAction)
      $global:yougoriDevTestStopped += $Id
      if ($env:YOUGORI_DEV_TEST_CASE -ne 'orphan' -or $Id -ne 111) { throw 'UNSAFE_PROCESS_TERMINATION' }
    }
    try {
      if ($env:YOUGORI_DEV_TEST_CASE.StartsWith('desktop-')) {
        & $env:YOUGORI_DEV_TEST_SCRIPT -CheckDesktopOnly
      } else {
        & $env:YOUGORI_DEV_TEST_SCRIPT
      }
      Write-Output 'CHECK_PASSED'
    } catch { Write-Output $_.Exception.Message }
    Write-Output ('LISTENER_QUERIES:' + $global:yougoriDevTestListenerQueries)
    Write-Output ('STOPPED:' + ($global:yougoriDevTestStopped -join ','))
  `
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
    encoding: "utf8", windowsHide: true, timeout: 15_000,
    env: {
      ...process.env,
      YOUGORI_DEV_TEST_CASE: scenario,
      YOUGORI_DEV_TEST_ROOT: fileURLToPath(new URL("../", import.meta.url)).replace(/[\\/]$/, ""),
      YOUGORI_DEV_TEST_SCRIPT: fileURLToPath(new URL("ensure-dev-port.ps1", import.meta.url)),
    },
  })
  if (result.error) throw result.error
  assert.equal(result.status, 0, result.stderr)
  return result.stdout
}

test("a second dev launch does not force-kill a live desktop or its guests", { skip: process.platform !== "win32" }, () => {
  const output = runCase("active")
  assert.match(output, /Use its terminal/)
  assert.match(output, /STOPPED:\s*$/)
  assert.doesNotMatch(output, /UNSAFE_PROCESS_TERMINATION/)
})

test("an unrelated process owning the dev port is preserved", { skip: process.platform !== "win32" }, () => {
  const output = runCase("unrelated")
  assert.match(output, /already owned by PID 111/)
  assert.match(output, /STOPPED:\s*$/)
})

test("a frontend attached to a CLI-started app is not mistaken for an orphan", { skip: process.platform !== "win32" }, () => {
  const output = runCase("active-attachment")
  assert.match(output, /launcher PID 222/)
  assert.match(output, /STOPPED:\s*$/)
  assert.doesNotMatch(output, /CHECK_PASSED|UNSAFE_PROCESS_TERMINATION/)
})

test("only an orphaned workspace Vite server is eligible for cleanup", { skip: process.platform !== "win32" }, () => {
  assert.match(runCase("orphan"), /STOPPED:111\s*$/)
})

test("desktop preflight catches a headless engine before touching the development server", { skip: process.platform !== "win32" }, () => {
  const output = runCase("desktop-headless")
  assert.match(output, /already running \(PID 333\)/)
  assert.match(output, /app quit --yes/)
  assert.match(output, /LISTENER_QUERIES:0/)
  assert.match(output, /STOPPED:\s*$/)
  assert.doesNotMatch(output, /CHECK_PASSED|UNSAFE_PROCESS_TERMINATION/)
})

test("desktop preflight allows a fresh launch without inspecting or stopping listeners", { skip: process.platform !== "win32" }, () => {
  for (const scenario of ["desktop-empty", "desktop-other-session", "desktop-cli-model"]) {
    const output = runCase(scenario)
    assert.match(output, /CHECK_PASSED/)
    assert.match(output, /LISTENER_QUERIES:0/)
    assert.match(output, /STOPPED:\s*$/)
    assert.doesNotMatch(output, /already running|UNSAFE_PROCESS_TERMINATION/)
  }
})
