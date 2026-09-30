// The launcher's in-container repairs, on temporary folders. No npm downloads.
import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import test from "node:test"

const { workspace, missingOptional } = createRequire(import.meta.url)("../cli/src/launcher/assets/prepare.cjs")

function folder(t, files) {
  const root = mkdtempSync(join(tmpdir(), "yougori-prepare-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true })
    writeFileSync(join(root, path), text)
  }
  return root
}

test("shell scripts from Windows get LF line endings and an executable bit", t => {
  const root = folder(t, {
    "scripts/start.sh": "#!/bin/sh\r\necho start\r\n",
    "server/deploy.bash": "echo deploy\r\n",
    "bin/cli": "#!/usr/bin/env node\r\nconsole.log(1)\r\n",
    "scripts/notes.txt": "plain\r\n",
    "src/app.js": "console.log(1)\r\n",
    "node_modules/x/run.sh": "echo x\r\n",
  })
  workspace(root, false)
  const read = path => readFileSync(join(root, path), "utf8")
  assert.equal(read("scripts/start.sh"), "#!/bin/sh\necho start\n")
  assert.equal(read("server/deploy.bash"), "echo deploy\n")
  assert.equal(read("bin/cli"), "#!/usr/bin/env node\nconsole.log(1)\r\n", "other interpreters only need a clean #! line")
  assert.equal(read("scripts/notes.txt"), "plain\r\n", "files without #! in scripts/ are data")
  assert.equal(read("src/app.js"), "console.log(1)\r\n")
  assert.equal(read("node_modules/x/run.sh"), "echo x\r\n")
  if (process.platform !== "win32") {
    assert.ok(statSync(join(root, "scripts/start.sh")).mode & 0o100)
    assert.ok(statSync(join(root, "bin/cli")).mode & 0o100)
    assert.ok(!(statSync(join(root, "scripts/notes.txt")).mode & 0o100))
  }
})

test("two-way sync keeps line endings, so nothing changes on the PC", t => {
  const root = folder(t, { "scripts/start.sh": "echo start\r\n" })
  workspace(root, true)
  assert.equal(readFileSync(join(root, "scripts/start.sh"), "utf8"), "echo start\r\n")
})

test("Linux builds npm skipped are found for this CPU and C library only", t => {
  const root = folder(t, {
    "node_modules/rollup/package.json": JSON.stringify({ optionalDependencies: {
      "@rollup/rollup-linux-x64-gnu": "4.1.0", "@rollup/rollup-linux-x64-musl": "4.1.0", "@rollup/rollup-win32-x64-msvc": "4.1.0",
    } }),
    "node_modules/@img/sharp/package.json": JSON.stringify({ optionalDependencies: {
      "@img/sharp-linux-x64": "0.33.0", "@img/sharp-libvips-linux-x64": "1.0.0", "@img/sharp-linuxmusl-x64": "0.33.0",
    } }),
    "node_modules/@img/sharp-libvips-linux-x64/package.json": "{}",
    "node_modules/esbuild/package.json": JSON.stringify({ optionalDependencies: { "@esbuild/linux-x64": "0.24.0" } }),
    "node_modules/@esbuild/linux-x64/package.json": "{}",
  })
  assert.deepEqual(missingOptional(root, "x64", false).sort(), ["@img/sharp-linux-x64@0.33.0", "@rollup/rollup-linux-x64-gnu@4.1.0"])
  assert.deepEqual(missingOptional(root, "x64", true).sort(), ["@img/sharp-linuxmusl-x64@0.33.0", "@rollup/rollup-linux-x64-musl@4.1.0"])
  assert.deepEqual(missingOptional(root, "arm64", false), [])
  assert.deepEqual(missingOptional(join(root, "missing"), "x64", false), [])
})
