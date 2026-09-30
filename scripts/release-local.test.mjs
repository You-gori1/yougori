import assert from "node:assert/strict"
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { options, packageNames, recordArtifact, runStep } from "./release-local.mjs"

test("local candidates reject unsafe output and unsupported or ambiguous package identities", () => {
  assert.throws(() => options(["--unknown"]), /Usage/)
  assert.throws(() => options(["--output"]), /Usage/)
  const source = join(tmpdir(), "source-checkout")
  assert.throws(() => options(["--output", source], source), /developer checkout/)
  assert.throws(() => options(["--output", tmpdir()], source), /developer checkout/)
  assert.deepEqual(packageNames("1.0.0", "win32", "x64"), ["nsis/Yougori_1.0.0_x64-setup.exe", "msi/Yougori_1.0.0_x64_en-US.msi"])
  assert.throws(() => packageNames("../old", "win32", "x64"), /version/)
  assert.deepEqual(packageNames("1.0.0", "linux", "arm64"), ["deb/Yougori_1.0.0_arm64.deb"])
})

test("candidate collection preserves exact bytes, refuses missing assets and cannot overwrite an earlier package", async t => {
  const fixture = await mkdtemp(join(tmpdir(), "yougori local release "))
  t.after(() => rm(fixture, { recursive: true, force: true }))
  const source = join(fixture, "input")
  const output = join(fixture, "packages")
  await mkdir(output)
  await writeFile(source, "exact release bytes")
  const item = await recordArtifact(source, output, "package.zip")
  assert.equal(item.bytes, 19)
  assert.match(item.sha256, /^[a-f0-9]{64}$/)
  assert.equal(await readFile(join(output, "package.zip"), "utf8"), "exact release bytes")
  await writeFile(source, "different candidate")
  await assert.rejects(recordArtifact(source, output, "package.zip"), /EEXIST/)
  await assert.rejects(recordArtifact(join(fixture, "missing"), output, "missing.zip"), /ENOENT/)
  await assert.rejects(recordArtifact(source, output, "../outside"), /file name/)
  assert.equal(await readFile(join(output, "package.zip"), "utf8"), "exact release bytes")
})

test("local gate captures both streams, records failures and bounds hung build processes", async t => {
  const logs = await mkdtemp(join(tmpdir(), "yougori release logs "))
  t.after(() => rm(logs, { recursive: true, force: true }))
  const context = { cwd: logs, logs, env: process.env }
  const failure = await runStep("failed", process.execPath, ["-e", 'process.stdout.write("out");process.stderr.write("error");process.exitCode=7'], context)
  assert.equal(failure.exitCode, 7)
  const content = await readFile(join(logs, "failed.log"), "utf8")
  assert.match(content, /out/)
  assert.match(content, /error/)
  const absent = await runStep("absent", join(logs, "not-a-program"), [], context)
  assert.equal(absent.exitCode, null)
  assert.match(absent.error, /ENOENT/)
  const hung = await runStep("hung", process.execPath, ["-e", "setInterval(()=>{},1000)"], { ...context, timeoutMs: 500 })
  assert.equal(hung.timedOut, true)
  assert.notEqual(hung.exitCode, 0)
})
