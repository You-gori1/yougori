import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import test from "node:test"
import { checkedFile, checkCompliance } from "./compliance-check.mjs"
import { frontendInventory, importedPackages } from "./compliance-frontend.mjs"

const hash = value => createHash("sha256").update(value).digest("hex")

async function fixture(t) {
  const parent = resolve(tmpdir())
  const root = await mkdtemp(join(parent, "yougori-compliance-test-"))
  t.after(async () => {
    // Delete only this explicitly created temporary fixture directory.
    assert.equal(dirname(resolve(root)), parent)
    assert.ok(root.startsWith(join(parent, "yougori-compliance-test-")))
    await rm(root, { recursive: true, force: true })
  })
  await mkdir(join(root, "src-tauri/resources/runtime"), { recursive: true })
  await mkdir(join(root, "compliance"))
  await mkdir(join(root, "compliance/evidence"))
  await mkdir(join(root, "build/compliance/bundle"), { recursive: true })
  await writeFile(join(root, "src-tauri/resources/runtime/guest.bin"), "runtime-v1")
  await writeFile(join(root, "build/compliance/bundle/source.tar.gz"), "source-v1")
  const report = {
    schemaVersion: 1,
    scope: "fixture only",
    components: [],
    inputs: [{ path: "src-tauri/resources/runtime/guest.bin", sha256: hash("runtime-v1") }],
    archives: [{ file: "source.tar.gz", sha256: hash("source-v1") }],
    blockers: [],
    reviewFingerprint: "f".repeat(64),
    review: { status: "approved", sourceFingerprint: "f".repeat(64), reviewer: "fixture", date: "2026-09-13", evidence: "fixture-only" },
    publication: { status: "verified", manifestUrl: "https://example.invalid/source", verifiedAt: "2026-09-13", evidence: "fixture-only" },
  }
  const save = async () => {
    const index = JSON.stringify(Object.fromEntries(["schemaVersion", "scope", "archives", "components", "blockers"].map(key => [key, report[key]])))
    await writeFile(join(root, "build/compliance/bundle/SOURCE_INDEX.json"), index)
    await writeFile(join(root, "build/compliance/bundle/SHA256SUMS"), report.archives.map(item => `${item.sha256}  ${item.file}\n`).join(""))
    report.publication.manifestSha256 ??= hash(index)
    await writeFile(join(root, "compliance/release.json"), JSON.stringify(report))
  }
  const put = async (path, value) => {
    await mkdir(dirname(join(root, path)), { recursive: true })
    await writeFile(join(root, path), typeof value === "string" ? value : JSON.stringify(value))
  }
  const applicationScript = fileURLToPath(new URL("./compliance-application.py", import.meta.url))
  const python = process.platform === "win32" ? "python" : "python3"
  const refresh = async () => {
    const inventory = await frontendInventory(root)
    await put("compliance/evidence/frontend-dependencies.json", inventory)
    for (const path of [...inventory.files, "package-lock.json", "compliance/evidence/frontend-dependencies.json",
      "compliance/evidence/application-dependencies.json", "src-tauri/resources/APPLICATION_LICENSES.txt"]) {
      const sha256 = hash(await readFile(join(root, path)))
      report.inputs = report.inputs.filter(item => item.path !== path)
      report.inputs.push({ path, sha256 })
    }
    if (existsSync(join(root, ".git"))) {
      execFileSync("git", ["-C", root, "add", "--", ".", ":!build", ":!node_modules"])
      const application = JSON.parse(execFileSync(python, [applicationScript, "collect", "--root", root], { encoding: "utf8" }))
      report.components = report.components.filter(item => item.id !== application.id)
      report.components.push(application)
      report.archives = report.archives.filter(item => item.file !== application.file)
      report.archives.push(Object.fromEntries(["file", "sha256", "bytes"].map(key => [key, application[key]])))
    }
    await save()
  }
  await put("compliance/frontend.json", { schemaVersion: 1, vendoredComponents: [], generatedAssets: [] })
  await put("compliance/native.json", { schemaVersion: 1 })
  await put("package-lock.json", { packages: {} })
  await put("compliance/evidence/application-dependencies.json", [])
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", "fixture notices")
  await put(".gitignore", "__pycache__/\n*.pyc\n")
  await refresh()
  // Exercise the real application-source gate, using a tiny tracked source tree.
  const required = JSON.parse(execFileSync(python, ["-c",
    "import json,runpy,sys; print(json.dumps(runpy.run_path(sys.argv[1])['REQUIRED']))", applicationScript], { encoding: "utf8" }))
  for (const name of required) {
    try { await readFile(join(root, name)) } catch { await put(name, name.endsWith("Cargo.lock") ? "version = 3\npackage = []\n" : "fixture source\n") }
    if (name.endsWith("Cargo.lock")) report.inputs.push({ path: name, sha256: hash(await readFile(join(root, name))) })
  }
  await refresh()
  execFileSync("git", ["init", "-q", root])
  execFileSync("git", ["-C", root, "add", "--", ...required])
  const application = JSON.parse(execFileSync(python, [applicationScript, "collect", "--root", root], { encoding: "utf8" }))
  report.components.push(application)
  report.archives.push(Object.fromEntries(["file", "sha256", "bytes"].map(key => [key, application[key]])))
  report.publication.manifestSha256 = undefined
  await save()
  return { root, report, save, put, refresh }
}

test("a complete fixture passes; a changed runtime requires new source review", async t => {
  const { root } = await fixture(t)
  assert.equal((await checkCompliance(root, { distribution: true })).distributionReady, true)
  await writeFile(join(root, "src-tauri/resources/runtime/guest.bin"), "runtime-v2")
  await assert.rejects(checkCompliance(root, { distribution: true }), /material changed/)
})

test("local packages can be prepared before publishing their matching sources", async t => {
  const { root, report, save } = await fixture(t)
  report.publication = { status: "not-published" }
  await save()
  assert.equal((await checkCompliance(root, { packaging: true })).packagingReady, true)
  await assert.rejects(checkCompliance(root, { distribution: true }), /not been published/)
  report.blockers.push({ id: "missing-source" })
  await save()
  await assert.rejects(checkCompliance(root, { packaging: true }), /Packaging blocked/)
})

test("new runtime files cannot evade the inventory", async t => {
  const { root } = await fixture(t)
  await writeFile(join(root, "src-tauri/resources/runtime/new-library.dll"), "new")
  await assert.rejects(checkCompliance(root), /files were added or removed/)
})

test("previews require complete sources without claiming release approval", async t => {
  const { root, report, save } = await fixture(t)
  report.review = { status: "pending" }
  report.publication = { status: "not-published" }
  await save()
  const result = await checkCompliance(root, { preview: true })
  assert.equal(result.packagingReady, false)
  assert.equal(result.distributionReady, false)
  await assert.rejects(checkCompliance(root, { packaging: true }), /Packaging blocked/)
  await assert.rejects(checkCompliance(root, { preview: true, distribution: true }), /Packaging blocked/)
  report.blockers.push({ id: "missing-source" })
  await save()
  await assert.rejects(checkCompliance(root, { preview: true }), /Preview blocked/)
})

test("Git newline conversion is accepted for text inputs, never runtime binaries", async t => {
  const { root, report, save, refresh } = await fixture(t)
  await writeFile(join(root, "LICENSE"), "license\nnotice\n")
  await refresh()
  await writeFile(join(root, "LICENSE"), "license\r\nnotice\r\n")
  report.inputs.push({ path: "LICENSE", sha256: hash("license\nnotice\n"), normalization: "lf" })
  await save()
  await checkCompliance(root)
  await writeFile(join(root, "LICENSE"), "license\nchanged notice\n")
  await assert.rejects(checkCompliance(root), /material changed/)
  report.inputs[0].normalization = "lf"
  await save()
  await assert.rejects(checkCompliance(root), /must cover raw bytes/)
})

test("collected sources do not override an unresolved finding", async t => {
  const { root, report, save } = await fixture(t)
  report.blockers.push({ id: "missing-upstream-source", reason: "fixture" })
  await save()
  assert.equal((await checkCompliance(root)).blockers, 1)
  await assert.rejects(checkCompliance(root, { distribution: true }), /Packaging blocked/)
})

test("a missing or altered source archive blocks packaging", async t => {
  const { root } = await fixture(t)
  const path = join(root, "build/compliance/bundle/source.tar.gz")
  await writeFile(path, "wrong-source")
  await assert.rejects(checkCompliance(root, { distribution: true }), /material changed/)
  await rm(path)
  await assert.rejects(checkCompliance(root, { distribution: true }), /ENOENT/)
})

test("review and publication must be recorded, not inferred from an empty blocker list", async t => {
  const { root, report, save } = await fixture(t)
  report.review.status = "pending"
  await save()
  await assert.rejects(checkCompliance(root, { distribution: true }), /Packaging blocked/)
  report.review.status = "approved"
  delete report.review.evidence
  await save()
  await assert.rejects(checkCompliance(root, { distribution: true }), /recorded source\/license review/)
  report.review.evidence = "fixture"
  report.publication.status = "not-published"
  await save()
  await assert.rejects(checkCompliance(root, { distribution: true }), /published and verified/)
})

test("inventory paths reject traversal, Windows streams and duplicate names", async t => {
  const { root, report, save } = await fixture(t)
  for (const path of ["../escape", "C:/escape", "runtime.bin:stream", "x\\y", "/absolute", "a/./b"]) {
    await assert.rejects(checkedFile(root, path), /Unsafe compliance path/)
  }
  report.inputs.push({ ...report.inputs[0] })
  await save()
  await assert.rejects(checkCompliance(root), /Duplicate compliance file/)
})

test("new frontend source and asset files require review", async t => {
  const { root, put, refresh } = await fixture(t)
  await put("src/new-code.ts", "export const value = 1")
  await assert.rejects(checkCompliance(root), /source or asset needs compliance review/)
  await refresh()
  await checkCompliance(root)
  await put("assets/new-image.svg", "<svg/>")
  await assert.rejects(checkCompliance(root), /source or asset needs compliance review/)
})

test("generated Python bytecode under src is not release source", async t => {
  const { root, put } = await fixture(t)
  await put("src/lib/__pycache__/model-terminal-chat.cpython-310.pyc", "generated bytecode")
  assert.ok(!(await frontendInventory(root)).files.some(path => path.includes("__pycache__")))
  await checkCompliance(root)
})

test("copied-code notices cannot be omitted even after rehashing release inputs", async t => {
  const { root, put, refresh } = await fixture(t)
  const path = "src/components/ui/button.tsx"
  const license = "src/components/ui/LICENSE.txt"
  const contents = "upstream MIT permission text\n"
  await put(path, "export const Button = () => null")
  await put(license, contents)
  await put("compliance/frontend.json", { schemaVersion: 1, generatedAssets: [], vendoredComponents: [{
    id: "fixture-ui", license: "MIT", upstream: "https://example.invalid/ui", files: [path],
    noticeFiles: [license], watchedDirectories: ["src/components/ui"],
  }] })
  await put("compliance/evidence/application-dependencies.json", [{ ecosystem: "vendored", name: "fixture-ui", license: "MIT", texts: [{ path: license, sha256: hash(contents) }] }])
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", contents)
  await refresh()
  await checkCompliance(root)
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", "credit accidentally removed")
  await refresh()
  await assert.rejects(checkCompliance(root), /Missing copied-code license text/)
  await put("src/components/ui/new-component.tsx", "export const New = () => null")
  await assert.rejects(refresh(), /Copied code needs attribution review/)
})

test("development dependencies imported by production CSS require shipped notices", async t => {
  const { root, put, refresh } = await fixture(t)
  const contents = "MIT License\nCopyright (c) Tailwind Labs, Inc.\n"
  await put("package-lock.json", { packages: { "node_modules/tailwindcss": { version: "4.3.3", dev: true, integrity: "fixture-integrity" } } })
  await put("src/styles.css", '@import "tailwindcss";')
  await put("node_modules/tailwindcss/LICENSE", contents)
  await refresh()
  await assert.rejects(checkCompliance(root), /Missing shipped npm license notices: tailwindcss/)
  await put("compliance/evidence/application-dependencies.json", [{ ecosystem: "npm", name: "tailwindcss", version: "4.3.3", integrity: "fixture-integrity", texts: [{ path: "LICENSE", installedPath: "node_modules/tailwindcss/LICENSE", sha256: hash(contents) }] }])
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", contents)
  await refresh()
  await checkCompliance(root)
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", "license accidentally removed")
  await refresh()
  await assert.rejects(checkCompliance(root), /Missing shipped npm license text/)
})

test("import inventory includes lazy modules, re-exports and generated CSS", () => {
  assert.deepEqual(importedPackages('import { x } from "@scope/pkg/sub"; export { y } from "re-export"; const z = import("lazy"); import "@/local"; import "node:fs";'), ["@scope/pkg", "lazy", "re-export"])
  assert.deepEqual(importedPackages('@import "tailwindcss"; @reference "./local.css"; @plugin "css-plugin"; a { background: url(local-image.svg); }', true), ["css-plugin", "tailwindcss"])
})

test("import inventory separates displayed examples from executed template expressions", () => {
  assert.deepEqual(importedPackages('const example = `import OpenAI from "openai";`; /* import "comment" */ const label = `value ${import("actual")}`; const p = require(`required`);'), ["actual", "required"])
})

test("a new standalone Rust lockfile cannot bypass dependency coverage", async t => {
  const { root, put } = await fixture(t)
  await put("new-tool/Cargo.lock", 'version = 3\n[[package]]\nname = "unrecorded"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "abc"\n')
  await assert.rejects(checkCompliance(root), /unrecorded 1.0.0: missing notices/)
})

test("ordinary evidence checks detect backend changes outside frontend inputs", async t => {
  const { root, put } = await fixture(t)
  await put("src-tauri/src/main.rs", "changed backend source")
  await assert.rejects(checkCompliance(root), /Application source manifest is stale/)
})

test("Rust attribution cannot be removed while retaining package records", async t => {
  const { root, report, put, refresh } = await fixture(t)
  const lock = 'version = 3\n[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "abc"\n'
  const section = `${"=".repeat(72)}\ncargo: example 1.0.0\nDeclared license: MIT\nFile: LICENSE\n\nFixture attribution\n`
  await put("engine/Cargo.lock", lock)
  report.inputs.find(item => item.path === "engine/Cargo.lock").sha256 = hash(lock)
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", section)
  await put("compliance/evidence/application-dependencies.json", [{ ecosystem: "cargo", name: "example", version: "1.0.0", license: "MIT", checksum: "abc", texts: [{ path: "LICENSE" }], noticeSectionSha256: [hash(section)] }])
  await refresh()
  await checkCompliance(root)
  await put("src-tauri/resources/APPLICATION_LICENSES.txt", section.replace("Fixture attribution", "removed"))
  await refresh()
  await assert.rejects(checkCompliance(root), /full notice text missing/)
})

test("source index and checksums must describe the reviewed archive set", async t => {
  const { root, put, save } = await fixture(t)
  await put("build/compliance/bundle/SOURCE_INDEX.json", { schemaVersion: 1, archives: [] })
  await assert.rejects(checkCompliance(root, { archives: true }), /Source index does not match/)
  await save()
  await put("build/compliance/bundle/SHA256SUMS", "stale checksums")
  await assert.rejects(checkCompliance(root, { packaging: true }), /Source checksums do not match/)
})

test("previous publication verification cannot approve a different source index", async t => {
  const { root, report, save } = await fixture(t)
  report.publication.manifestSha256 = "0".repeat(64)
  await save()
  await assert.rejects(checkCompliance(root, { distribution: true }), /Published source manifest does not match/)
})

test("all installer configurations carry the AGPL license, commercial option and dependency notices", async () => {
  const json = async name => JSON.parse(await readFile(new URL(`../${name}`, import.meta.url), "utf8"))
  assert.equal((await json("package.json")).license, "AGPL-3.0-only")
  assert.equal((await json("package-lock.json")).packages[""].license, "AGPL-3.0-only")
  const base = await json("src-tauri/tauri.conf.json")
  assert.equal(base.bundle.license, "AGPL-3.0-only")
  assert.equal(base.bundle.licenseFile, "../COPYING")
  assert.match(base.build.beforeBuildCommand, /release:package/)
  for (const platform of ["windows", "linux", "macos"]) {
    const { resources } = (await json(`src-tauri/tauri.${platform}.conf.json`)).bundle
    assert.equal(resources["../LICENSE"], "LICENSE")
    assert.equal(resources["../COPYING"], "COPYING")
    assert.equal(resources["../COMMERCIAL_LICENSE.txt"], "COMMERCIAL_LICENSE.txt")
    assert.equal(resources["../NOTICE"], "NOTICE")
    for (const file of ["APPLICATION_LICENSES.txt", "RUNTIME_LICENSES.txt", "WORKSPACE_LICENSES.txt"]) {
      assert.equal(resources[`resources/${file}`], file)
    }
  }
})
