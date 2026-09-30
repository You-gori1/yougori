import test from "node:test"
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, readdirSync, rmSync, symlinkSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"

const available = spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"], { windowsHide: true }).status === 0
const cargoAvailable = spawnSync("cargo", ["--version"], { windowsHide: true }).status === 0
const script = resolve("scripts/build-cache.ps1")

test("compiler cleanup preserves application data and rejects redirected caches", { skip: !available }, () => {
  const root = mkdtempSync(join(tmpdir(), "yougori build cache "))
  const workspace = join(root, "checkout")
  const profile = join(workspace, "src-tauri", "target", "debug")
  const outside = join(root, "outside")
  const run = (...args) => spawnSync("pwsh", ["-NoProfile", "-File", script, "-WorkspaceRoot", workspace, ...args], { encoding: "utf8", windowsHide: true })
  try {
    mkdirSync(profile, { recursive: true })
    mkdirSync(outside)
    writeFileSync(join(outside, "important"), "preserve")
    writeFileSync(join(workspace, "src-tauri", "Cargo.toml"), "[package]\nname='fixture'\n")
    writeFileSync(join(profile, ".cargo-lock"), "")
    for (const name of ["incremental", "deps", "build", ".fingerprint", "runtime", "models"]) {
      mkdirSync(join(profile, name))
      writeFileSync(join(profile, name, "keep-or-cache"), "fixture")
    }
    writeFileSync(join(profile, "yougori.exe"), "app")
    let result = run()
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /Reclaimable:/)
    assert.ok(existsSync(join(profile, "deps", "keep-or-cache")))

    // A linked compiler directory must not let cleanup reach unrelated files.
    rmSync(join(profile, "incremental"), { recursive: true })

    // Nested links also must be rejected before any deletion follows them.
    mkdirSync(join(profile, "incremental"))
    symlinkSync(outside, join(profile, "incremental", "nested"), process.platform === "win32" ? "junction" : "dir")
    result = run("-Clean")
    assert.notEqual(result.status, 0)
    assert.ok(existsSync(join(outside, "important")))
    rmSync(join(profile, "incremental"), { recursive: true })
    symlinkSync(outside, join(profile, "incremental"), process.platform === "win32" ? "junction" : "dir")
    result = run("-Clean")
    assert.notEqual(result.status, 0)
    assert.ok(existsSync(join(outside, "important")))
    assert.ok(existsSync(join(profile, "deps", "keep-or-cache")))
    rmSync(join(profile, "incremental"), { recursive: true })

    result = run("-Clean")
    assert.equal(result.status, 0, result.stderr)
    for (const name of ["deps", "build", ".fingerprint"]) assert.equal(existsSync(join(profile, name)), false)
    for (const name of ["yougori.exe", "runtime/keep-or-cache", "models/keep-or-cache"]) assert.ok(existsSync(join(profile, name)))

    // External target junctions need an explicitly named allowed root.
    const externalProfile = join(outside, "debug")
    mkdirSync(join(externalProfile, ".fingerprint"), { recursive: true })
    mkdirSync(join(externalProfile, "deps"))
    writeFileSync(join(externalProfile, ".cargo-lock"), "")
    writeFileSync(join(externalProfile, "deps", "cache"), "cache")
    mkdirSync(join(workspace, "cli"))
    symlinkSync(outside, join(workspace, "cli", "target"), process.platform === "win32" ? "junction" : "dir")
    result = run("-Clean")
    assert.equal(result.status, 0, result.stderr)
    assert.ok(existsSync(join(externalProfile, "deps", "cache")))
    result = run("-Clean", "-CacheRoot", outside)
    assert.equal(result.status, 0, result.stderr)
    assert.equal(existsSync(join(externalProfile, "deps")), false)
    assert.ok(existsSync(join(outside, "important")))
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test("Cargo development and test builds do not create incremental compiler state", { skip: !cargoAvailable }, () => {
  const root = mkdtempSync(join(tmpdir(), "yougori-cache-policy-"))
  try {
    mkdirSync(join(root, "src"))
    writeFileSync(join(root, "Cargo.toml"), '[package]\nname="yougori-cache-policy-probe"\nversion="0.0.0"\nedition="2021"\n')
    writeFileSync(join(root, "src", "lib.rs"), "pub fn answer() -> u32 { 42 }\n")
    // Cargo reads the real checkout configuration from cwd, even with an
    // external manifest. Only a tiny dependency-free fixture is compiled.
    // Test repository defaults independently of CI's resource-saving overrides.
    const env = { ...process.env }
    for (const key of Object.keys(env)) {
      if (/^CARGO_PROFILE_|^CARGO_INCREMENTAL$/i.test(key)) delete env[key]
    }
    for (const command of [["build"], ["test", "--no-run"]]) {
      const result = spawnSync("cargo", [...command, "--offline", "--verbose", "--manifest-path", join(root, "Cargo.toml"), "--target-dir", join(root, "target")], {
        encoding: "utf8", windowsHide: true, timeout: 60_000, env,
      })
      assert.equal(result.status, 0, result.stderr)
      assert.match(result.stderr, /debuginfo=line-tables-only/)
      assert.doesNotMatch(result.stderr, /-C incremental=/)
    }
    const incremental = join(root, "target", "debug", "incremental")
    assert.deepEqual(existsSync(incremental) ? readdirSync(incremental) : [], [])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
