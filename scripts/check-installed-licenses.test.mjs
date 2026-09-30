import assert from "node:assert/strict"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import test from "node:test"
import { checkInstalledLicenses } from "./check-installed-licenses.mjs"

test("installed notices must exist and match, including nested GPL text", async t => {
  const parent = resolve(tmpdir())
  const root = await mkdtemp(join(parent, "yougori-installed-notices-test-"))
  t.after(async () => {
    assert.equal(dirname(resolve(root)), parent)
    assert.ok(root.startsWith(join(parent, "yougori-installed-notices-test-")))
    await rm(root, { recursive: true, force: true })
  })
  const installed = join(root, "installed")
  const resources = {}
  const names = ["LICENSE", "COPYING", "NOTICE", "COMMERCIAL_LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "APPLICATION_LICENSES.txt", "RUNTIME_LICENSES.txt", "WORKSPACE_LICENSES.txt"]
  for (const name of names) {
    resources[`../${name}`] = name
    await mkdir(installed, { recursive: true })
    await writeFile(join(root, name), `notice ${name}\n`)
    await writeFile(join(installed, name), `notice ${name}\r\n`)
  }
  resources["resources/runtime/"] = "runtime/"
  for (const directory of [join(root, "src-tauri/resources/runtime"), join(installed, "runtime")]) {
    await mkdir(directory, { recursive: true })
    await writeFile(join(directory, "COPYING"), "GPL component notice\n")
    await writeFile(join(directory, "COPYING3"), "GPL version 3 notice\n")
    await writeFile(join(directory, "p11-kit-COPYING"), "p11-kit notice\n")
  }
  await writeFile(join(root, "src-tauri/tauri.windows.conf.json"), JSON.stringify({ bundle: { resources } }))
  assert.equal(await checkInstalledLicenses(root, installed, "windows"), 11)
  await writeFile(join(installed, "runtime/COPYING3"), "wrong GPL version")
  await assert.rejects(checkInstalledLicenses(root, installed, "windows"), /differs/)
  await writeFile(join(installed, "runtime/COPYING3"), "GPL version 3 notice\n")
  await writeFile(join(installed, "runtime/COPYING"), "wrong license")
  await assert.rejects(checkInstalledLicenses(root, installed, "windows"), /differs/)
  await writeFile(join(installed, "runtime/COPYING"), "GPL component notice\n")
  await rm(join(installed, "COPYING"))
  await assert.rejects(checkInstalledLicenses(root, installed, "windows"), /ENOENT/)
  assert.ok((await readFile(join(root, "COPYING"), "utf8")).includes("notice"))
})
