import assert from "node:assert/strict"
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { bundleVault } from "./bundle-vault.mjs"
import { windowsTarget } from "./release-preflight.mjs"

function executable(dll) {
  const buffer = Buffer.alloc(1024)
  buffer.write("MZ")
  buffer.writeUInt32LE(128, 60)
  buffer.writeUInt32LE(0x4550, 128)
  buffer.writeUInt16LE(0x8664, 132)
  buffer.writeUInt16LE(1, 134)
  buffer.writeUInt16LE(240, 148)
  buffer.writeUInt16LE(0x20b, 152)
  buffer.writeUInt32LE(4096, 272)
  buffer.writeUInt32LE(40, 276)
  buffer.writeUInt32LE(4096, 404)
  buffer.writeUInt32LE(512, 408)
  buffer.writeUInt32LE(512, 412)
  buffer.writeUInt32LE(4160, 524)
  buffer.write(`${dll}\0`, 576)
  return buffer
}

function fixture(t, dll) {
  const root = mkdtempSync(join(tmpdir(), "yougori-vault-bundle-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const targetRoot = join(root, "custom target directory")
  const binaryDirectory = join(targetRoot, windowsTarget, "release")
  mkdirSync(binaryDirectory, { recursive: true })
  const binary = executable(dll)
  writeFileSync(join(binaryDirectory, "yougori-vault.exe"), binary)
  const calls = []
  const options = { root, platform: "win32", arch: "x64", env: { CARGO_BUILD_TARGET: windowsTarget, RUSTFLAGS: "-C debuginfo=0" },
    run(command, args, settings) {
      calls.push({ command, args, settings })
      return args[0] === "metadata" ? { status: 0, stdout: JSON.stringify({ target_directory: targetRoot }) } : { status: 0 }
    } }
  return { root, binary, calls, options }
}

test("vault bundling uses the actual Cargo target and portable Windows runtime", t => {
  const { root, binary, calls, options } = fixture(t, "KERNEL32.dll")
  bundleVault(options)
  assert.deepEqual(calls[0].args.slice(0, 7), ["build", "--locked", "--release", "--bin", "yougori-vault", "--target", windowsTarget])
  assert.equal(calls[0].settings.env.RUSTFLAGS, "-C debuginfo=0 -C target-feature=+crt-static")
  assert.deepEqual(readFileSync(join(root, "src-tauri/resources/vault/yougori-vault.exe")), binary)
  assert.deepEqual(readFileSync(join(root, "src-tauri/target/release/vault/yougori-vault.exe")), binary)
})

test("vault bundling rejects an external Visual C++ runtime before replacing the payload", t => {
  const { root, options } = fixture(t, "VCRUNTIME140.dll")
  assert.throws(() => bundleVault(options), /Personal Vault requires an unbundled Visual C\+\+ runtime/)
  assert.equal(existsSync(join(root, "src-tauri/resources/vault/yougori-vault.exe")), false)
})

test("vault bundling reports compiler and metadata failures without copying stale binaries", t => {
  const { root, options } = fixture(t, "KERNEL32.dll")
  assert.throws(() => bundleVault({ ...options, run: () => ({ status: 1 }) }), /build failed/)
  assert.throws(() => bundleVault({ ...options, run: (_command, args) => args[0] === "metadata"
    ? { error: new Error("metadata failed") } : { status: 0 } }), /metadata failed/)
  assert.equal(existsSync(join(root, "src-tauri/resources/vault/yougori-vault.exe")), false)
})
