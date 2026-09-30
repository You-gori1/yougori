import { createHash } from "node:crypto"
import { readFile } from "node:fs/promises"
import { windowsPeImports } from "./release-preflight.mjs"

const sha = bytes => createHash("sha256").update(bytes).digest("hex")
const normalized = bytes => bytes.toString("utf8").replace(/\r\n?/g, "\n")

export function validateAngleRecord(build, policy) {
  if (build.schemaVersion !== 1 || build.profile !== "angle-d3d11-only"
    || build.revision !== policy.revision || !build.inputs?.length || !build.binaries?.length) {
    throw new Error("Missing or unreviewed ANGLE build identity")
  }
  if (build.fontPreprocessing?.dataPresent !== false || build.fontPreprocessing?.disabledBody !== "return nullptr;") {
    throw new Error("ANGLE font exclusion has not been verified")
  }
  const components = new Map(policy.components.map(item => [item.id, item]))
  for (const input of build.inputs) {
    if (!/^[a-f0-9]{64}$/.test(input.sha256) || !components.has(input.component)) {
      throw new Error(`Unreviewed embedded ANGLE component: ${input.component}`)
    }
    if (policy.forbiddenSourcePrefixes.some(prefix => input.path.startsWith(prefix))) {
      throw new Error(`Excluded ANGLE implementation returned: ${input.path}`)
    }
  }
  for (const component of components.values()) {
    if (component.review !== "approved" || !component.license || !component.basis || !component.notice) {
      throw new Error(`Incomplete native component license review: ${component.id}`)
    }
    // An interface-only exception must not cover implementation paths.
    if (component.id === "khronos-apache-interfaces") {
      for (const input of build.inputs.filter(item => item.component === component.id)) {
        if (!policy.interfaceHeaders.includes(input.path)) throw new Error(`Unreviewed Apache interface header: ${input.path}`)
      }
    }
  }
}

export function validateAngleNotices(notices, components) {
  for (const component of components) {
    if (!notices.includes(component.notice)) throw new Error(`Native component notice missing: ${component.id}`)
  }
}

export async function checkNative(root, report, checkedFile) {
  const read = async name => readFile(await checkedFile(root, name))
  const json = async name => JSON.parse(await read(name))
  const peInputs = report.inputs.filter(item => /^src-tauri\/resources\/runtime\/qemu(?:-secure)?\/[^/]+\.(?:exe|dll)$/i.test(item.path))
  const config = await json("compliance/native.json")
  if (config.schemaVersion !== 1) throw new Error("Missing native component policy")
  if (!peInputs.length) return
  for (const name of ["compliance/native.json", "compliance/evidence/windows-pe-imports.json", "compliance/evidence/windows-dlls.json"]) {
    if (!report.inputs.some(item => item.path === name)) throw new Error(`Native evidence is not in the release inventory: ${name}`)
  }
  const importBytes = await read("compliance/evidence/windows-pe-imports.json")
  if (sha(normalized(importBytes)) !== config.reviewedPeInventorySha256) {
    throw new Error("Native binaries or imports changed after their source/license review")
  }
  const imports = JSON.parse(importBytes)
  const dlls = await json("compliance/evidence/windows-dlls.json")
  if (imports.length !== peInputs.length || new Set(imports.map(item => item.file)).size !== imports.length) {
    throw new Error("Incomplete native PE inventory")
  }
  for (const entry of peInputs) {
    const data = await read(entry.path)
    const observed = imports.find(item => item.file === entry.path)
    if (!observed || observed.sha256 !== sha(data)
      || JSON.stringify(observed.imports) !== JSON.stringify(windowsPeImports(data))) {
      throw new Error(`Native import evidence changed: ${entry.path}`)
    }
    if (/\.dll$/i.test(entry.path)) {
      const origin = dlls.find(item => item.file === entry.path)
      if (!origin || origin.sha256 !== observed.sha256 || !origin.provenance || origin.provenance.startsWith("unresolved")) {
        throw new Error(`Unresolved native DLL source: ${entry.path}`)
      }
    }
  }
  const policy = config.angle
  if (!policy?.reviewedBuildSha256) throw new Error("ANGLE needs an explicit embedded-component review")
  const buildBytes = await read(policy.evidence)
  if (sha(normalized(buildBytes)) !== policy.reviewedBuildSha256) throw new Error("ANGLE build changed after its embedded-component review")
  const build = JSON.parse(buildBytes)
  validateAngleRecord(build, policy)
  if (!report.components.some(item => item.id === build.sourceComponent && item.status === "collected")) {
    throw new Error("Missing corresponding ANGLE source component")
  }
  for (const file of [build.buildScript, build.inspector, build.arguments, build.notices, ...build.evidence]) {
    if (sha(normalized(await read(file.path))) !== file.sha256) throw new Error(`ANGLE build evidence changed: ${file.path}`)
    if (!report.inputs.some(item => item.path === file.path)) throw new Error(`ANGLE evidence is not in the release inventory: ${file.path}`)
  }
  const notices = normalized(await read(build.notices.path))
  if (!normalized(await read("src-tauri/resources/RUNTIME_LICENSES.txt")).includes(notices.trim())) {
    throw new Error("Embedded ANGLE notices are missing from the shipped runtime notices")
  }
  validateAngleNotices(notices, policy.components)
  for (const part of ["qemu", "qemu-secure"]) {
    const prefix = `src-tauri/resources/runtime/${part}/`
    for (const binary of build.binaries) {
      const name = binary.file === "libEGL.dll" ? "libEGL_angle.dll" : binary.file
      if (sha(await read(prefix + name)) !== binary.sha256) throw new Error(`ANGLE DLL does not match its reviewed build: ${part}/${name}`)
    }
    if (normalized(await read(prefix + "ANGLE_BUILD.json")) !== normalized(buildBytes)
      || normalized(await read(prefix + "ANGLE-NOTICES.txt")) !== notices) throw new Error(`${part}: embedded ANGLE provenance/notices differ`)
    const files = new Map(imports.filter(item => item.file.startsWith(prefix)).map(item => [item.file.slice(prefix.length).toLowerCase(), item]))
    const pending = ["qemu-system-x86_64.exe"]
    const reachable = new Set()
    for (const edge of config.dynamicLoads) {
      if (!files.has(edge.from) || !files.has(edge.to) || !edge.basis) throw new Error(`Unresolved dynamic native load: ${edge.from} -> ${edge.to}`)
    }
    while (pending.length) {
      const name = pending.pop().toLowerCase()
      if (reachable.has(name)) continue
      reachable.add(name)
      pending.push(...(files.get(name)?.imports ?? []), ...config.dynamicLoads.filter(edge => edge.from === name).map(edge => edge.to))
    }
    for (const name of config.forbiddenQemuDependencies) {
      if (reachable.has(name)) throw new Error(`Forbidden QEMU native dependency: ${name}`)
    }
    for (const name of ["libegl.dll", "libegl_angle.dll", "libglesv2.dll"]) {
      if (!reachable.has(name)) throw new Error(`Missing dynamic graphics dependency evidence: ${name}`)
    }
  }
}
