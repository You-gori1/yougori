import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { buildEngine } from "./package-engine.mjs"
import { linuxTarget, linuxArmTarget } from "./release-preflight.mjs"

function fixture(t, arch = "x64") {
  const root = mkdtempSync(join(tmpdir(), "yougori-engine-package-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const targetRoot = join(root, "custom target")
  const target = arch === "arm64" ? linuxArmTarget : linuxTarget
  const directory = join(targetRoot, target, "release")
  mkdirSync(directory, { recursive: true })
  const binary = join(directory, "yougori-engine")
  const elf = Buffer.alloc(64)
  elf.write("\x7fELF")
  elf[4] = 2
  elf[5] = 1
  elf.writeUInt16LE(3, 16)
  elf.writeUInt16LE(arch === "arm64" ? 183 : 62, 18)
  writeFileSync(binary, elf)
  const calls = []
  const options = { projectRoot: root, platform: "linux", arch, env: {},
    execute(command, args, settings) {
      calls.push({ command, args, settings })
      return args[0] === "metadata" ? { status: 0, stdout: JSON.stringify({ target_directory: targetRoot }) } : { status: 0 }
    } }
  return { binary, calls, options }
}

test("engine packaging locates the locked native build in Cargo's configured directory", t => {
  const { binary, calls, options } = fixture(t)
  assert.equal(buildEngine(options), binary)
  assert.deepEqual(calls[0].args.slice(0, 5), ["build", "--locked", "--release", "--target", linuxTarget])
})

test("engine packaging refuses failed builds, failed metadata and stale foreign binaries", t => {
  const { binary, options } = fixture(t)
  assert.throws(() => buildEngine({ ...options, execute: () => ({ status: 1 }) }), /build failed/)
  assert.throws(() => buildEngine({ ...options, execute: (_command, args) => args[0] === "metadata" ? { status: 1, stderr: "metadata failed" } : { status: 0 } }), /metadata failed/)
  writeFileSync(binary, Buffer.from("MZ wrong platform"))
  assert.throws(() => buildEngine(options), /ELF engine/)
  assert.throws(() => buildEngine({ ...options, arch: "ia32" }), /Supported desktop builds/)
  assert.throws(() => buildEngine({ ...options, env: { VITE_YOUGORI_TEST_ADAPTER: "1" } }), /test adapter/)
})

test("ARM64 engine packaging checks the actual ELF architecture", t => {
  const { binary, calls, options } = fixture(t, "arm64")
  assert.equal(buildEngine(options), binary)
  assert.deepEqual(calls[0].args.slice(0, 5), ["build", "--locked", "--release", "--target", linuxArmTarget])
  const foreign = Buffer.alloc(64)
  foreign.write("\x7fELF")
  foreign[4] = 2
  foreign[5] = 1
  foreign.writeUInt16LE(3, 16)
  foreign.writeUInt16LE(62, 18)
  writeFileSync(binary, foreign)
  assert.throws(() => buildEngine(options), /arm64 Linux ELF/)
})

test("Windows engine builds request the portable runtime before locating the output", t => {
  const { calls, options } = fixture(t)
  assert.throws(() => buildEngine({ ...options, platform: "win32" }), /ENOENT/)
  assert.match(calls[0].settings.env.RUSTFLAGS, /target-feature=\+crt-static/)
})
