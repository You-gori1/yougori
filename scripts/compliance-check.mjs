import { createHash } from "node:crypto"
import { execFile } from "node:child_process"
import { promisify } from "node:util"
import { createReadStream } from "node:fs"
import { lstat, readFile, realpath, readdir } from "node:fs/promises"
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { checkFrontend } from "./compliance-frontend.mjs"
import { checkNative } from "./compliance-native.mjs"

async function hashFile(path, normalization) {
  const hash = createHash("sha256")
  if (normalization === "lf") {
    return hash.update((await readFile(path, "utf8")).replace(/\r\n?/g, "\n")).digest("hex")
  }
  for await (const chunk of createReadStream(path)) hash.update(chunk)
  return hash.digest("hex")
}

export async function checkedFile(root, name) {
  if (typeof name !== "string" || !name || name.includes("\\") || name.includes(":") || name.includes("\0")
    || name.startsWith("/") || name.split("/").some(part => !part || part === "." || part === "..")) {
    throw new Error(`Unsafe compliance path: ${name}`)
  }
  const base = await realpath(root)
  const path = join(base, name)
  const actual = await realpath(path)
  const tail = relative(base, actual)
  if (isAbsolute(tail) || tail === ".." || tail.startsWith(`..${sep}`)) throw new Error(`Compliance path escapes its directory: ${name}`)
  let current = base
  for (const part of name.split("/")) {
    current = join(current, part)
    if ((await lstat(current)).isSymbolicLink()) throw new Error(`Compliance links are not accepted: ${name}`)
  }
  if (!(await lstat(path)).isFile()) throw new Error(`Compliance material is not a file: ${name}`)
  return path
}

async function verifyFiles(root, records) {
  if (!Array.isArray(records) || !records.length) throw new Error("Empty compliance file inventory")
  const names = new Set()
  for (const entry of records) {
    const name = entry.path ?? entry.file
    if (names.has(name?.toLowerCase())) throw new Error(`Duplicate compliance file: ${name}`)
    names.add(name?.toLowerCase())
    if (!/^[a-f0-9]{64}$/.test(entry.sha256)) throw new Error(`Invalid compliance checksum: ${name}`)
    if (entry.normalization !== undefined && entry.normalization !== "lf") throw new Error(`Invalid checksum normalization: ${name}`)
    if (entry.normalization && (entry.file !== undefined || name.startsWith("src-tauri/resources/runtime/"))) {
      throw new Error(`Runtime and archive checksums must cover raw bytes: ${name}`)
    }
    const path = await checkedFile(root, name)
    if (await hashFile(path, entry.normalization) !== entry.sha256) throw new Error(`Compliance material changed: ${name}. Regenerate its release evidence and review source coverage.`)
  }
}

async function runtimeFiles(root) {
  const files = []
  async function walk(path, prefix) {
    for (const entry of await readdir(path, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) throw new Error(`Uninventoried runtime link: ${prefix}/${entry.name}`)
      if (entry.isDirectory()) await walk(join(path, entry.name), `${prefix}/${entry.name}`)
      else files.push(`${prefix}/${entry.name}`)
    }
  }
  await walk(join(root, "src-tauri/resources/runtime"), "src-tauri/resources/runtime")
  return files.sort()
}

export async function checkSourceIndex(sourceDirectory, report) {
  const index = JSON.parse(await readFile(await checkedFile(sourceDirectory, "SOURCE_INDEX.json"), "utf8"))
  const expected = Object.fromEntries(["schemaVersion", "scope", "archives", "components", "blockers"].map(key => [key, report[key]]))
  if (JSON.stringify(index) !== JSON.stringify(expected)) throw new Error("Source index does not match the reviewed release")
  const sums = await readFile(await checkedFile(sourceDirectory, "SHA256SUMS"), "utf8")
  if (sums.replace(/\r\n/g, "\n") !== report.archives.map(item => `${item.sha256}  ${item.file}\n`).join("")) {
    throw new Error("Source checksums do not match the reviewed release")
  }
}

export async function checkCompliance(root, { distribution = false, packaging = false, preview = false, archives = false, sourceDirectory = join(root, "build/compliance/bundle") } = {}) {
  if (preview) archives = true
  const report = JSON.parse(await readFile(join(root, "compliance/release.json"), "utf8"))
  if (report.schemaVersion !== 1 || !Array.isArray(report.blockers)) throw new Error("Invalid compliance report")
  await verifyFiles(root, report.inputs)
  try {
    await promisify(execFile)(process.platform === "win32" ? "python" : "python3", [
      fileURLToPath(new URL("./compliance-dependencies.py", import.meta.url)), "--root", root,
    ])
  } catch (error) {
    throw new Error(`Dependency coverage verification failed: ${error.stderr || error.message}`)
  }
  await checkFrontend(root, report, checkedFile)
  const expected = report.inputs.filter(item => item.path.startsWith("src-tauri/resources/runtime/")).map(item => item.path).sort()
  if (JSON.stringify(await runtimeFiles(root)) !== JSON.stringify(expected)) throw new Error("Runtime files were added or removed after the compliance inventory")
  await checkNative(root, report, checkedFile)
  if (!Array.isArray(report.archives) || !report.archives.length) throw new Error("Missing source archive inventory")
  if (preview && report.blockers.length) throw new Error("Preview blocked: unresolved source/license items")
  if (distribution || packaging) {
    if (report.blockers.length || report.review?.status !== "approved") {
      throw new Error(`Packaging blocked: ${report.blockers.length} unresolved compliance items; source/license review is ${report.review?.status ?? "missing"}. See compliance/release.json. Development and tests remain available.`)
    }
    if (!report.review.reviewer || !report.review.date || !report.review.evidence) throw new Error("Distribution needs a recorded source/license review")
    if (!/^[a-f0-9]{64}$/.test(report.reviewFingerprint ?? "") || report.review.sourceFingerprint !== report.reviewFingerprint) {
      throw new Error("Source/license approval does not cover the current source fingerprint")
    }
  }
  if (distribution || packaging || archives) {
    await verifyFiles(sourceDirectory, report.archives)
    await checkSourceIndex(sourceDirectory, report)
  }
  try {
    await promisify(execFile)(process.platform === "win32" ? "python" : "python3", [
      fileURLToPath(new URL("./compliance-application.py", import.meta.url)),
      "check", "--root", root, "--bundle", sourceDirectory,
      ...(!(distribution || packaging || archives) ? ["--manifest-only"] : []),
    ])
  } catch (error) {
    throw new Error(`Application source verification failed: ${error.stderr || error.message}`)
  }
  if (distribution) {
    const publication = report.publication
    if (publication?.status !== "verified" || !publication.manifestUrl?.startsWith("https://")
      || !publication.verifiedAt || !publication.evidence) throw new Error("Source downloads have not been published and verified for this release")
    if (publication.manifestSha256 !== await hashFile(await checkedFile(sourceDirectory, "SOURCE_INDEX.json"))) {
      throw new Error("Published source manifest does not match the current source index")
    }
  }
  return { blockers: report.blockers.length, archives: report.archives.length, packagingReady: packaging || distribution, distributionReady: distribution }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
  try {
    const args = process.argv.slice(2)
    if (args.some(arg => !["--distribution", "--package", "--preview", "--archives"].includes(arg))) throw new Error("Usage: node scripts/compliance-check.mjs [--distribution | --package | --preview] [--archives]")
    const result = await checkCompliance(root, { distribution: args.includes("--distribution"), packaging: args.includes("--package"), preview: args.includes("--preview"), archives: args.includes("--archives") })
    console.log(`Compliance evidence verified: ${result.archives} source archives, ${result.blockers} unresolved items. ${result.distributionReady ? "Distribution checks passed." : result.packagingReady ? "Local packaging checks passed; publication is checked separately." : "This is not distribution approval."}`)
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
