import { createHash } from "node:crypto"
import { readdir, readFile } from "node:fs/promises"
import { join, resolve } from "node:path"
import { pathToFileURL } from "node:url"
import ts from "typescript"

const normalize = text => text.replace(/\r\n?/g, "\n")
const sha256 = text => createHash("sha256").update(text).digest("hex")
const sourceRoots = ["src", "assets", "public", "src-tauri/icons"]
const sourceConfigs = ["components.json", "index.html", "vite.config.ts"]

async function walk(root, directory) {
  let entries
  try { entries = await readdir(join(root, directory), { withFileTypes: true }) }
  catch (error) { if (error.code === "ENOENT") return []; throw error }
  const files = []
  for (const entry of entries) {
    // Python's bytecode cache can appear under src/ after local tests. It is
    // generated, Git-ignored output rather than a frontend source input.
    if (entry.name === "__pycache__" || entry.name.endsWith(".pyc")) continue
    const name = `${directory}/${entry.name}`
    if (entry.isSymbolicLink()) throw new Error(`Frontend links need source review: ${name}`)
    if (entry.isDirectory()) files.push(...await walk(root, name))
    else if (entry.isFile()) files.push(name)
  }
  return files.sort()
}

function packageName(specifier) {
  if (/^(?:[./#]|@\/|\w+:)/.test(specifier)) return null
  return specifier.startsWith("@") ? specifier.split("/").slice(0, 2).join("/") : specifier.split("/")[0]
}

export function importedPackages(source, css = false) {
  const specifiers = []
  if (css) {
    const pattern = /@(?:import|reference|plugin)\s+(?:url\(\s*)?["']([^"']+)["']|url\(\s*["']?(~[^\s"')]+)["']?\s*\)/g
    for (const match of source.matchAll(pattern)) specifiers.push((match[1] ?? match[2]).replace(/^~/, ""))
  } else {
    // Parse actual code: examples inside strings/comments are not dependencies.
    // Descend into template expressions so executable lazy imports still count.
    const tree = ts.createSourceFile("inventory.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
    const add = node => { if (node && (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node))) specifiers.push(node.text) }
    function visit(node) {
      if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) add(node.moduleSpecifier)
      if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword
        || (ts.isIdentifier(node.expression) && node.expression.text === "require"))) add(node.arguments[0])
      if (ts.isImportEqualsDeclaration(node) && ts.isExternalModuleReference(node.moduleReference)) add(node.moduleReference.expression)
      ts.forEachChild(node, visit)
    }
    visit(tree)
  }
  return [...new Set(specifiers.map(packageName).filter(Boolean))].sort()
}

export async function frontendInventory(root) {
  const manifest = JSON.parse(await readFile(join(root, "compliance/frontend.json"), "utf8"))
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.vendoredComponents) || !Array.isArray(manifest.generatedAssets)) {
    throw new Error("Missing frontend provenance inventory")
  }
  const files = (await Promise.all(sourceRoots.map(directory => walk(root, directory)))).flat()
  for (const entry of await readdir(root, { withFileTypes: true })) {
    if (sourceConfigs.includes(entry.name) || /\.(?:svg|png|jpe?g|webp|gif|ico|woff2?|ttf|otf)$/i.test(entry.name)) {
      if (!entry.isFile() || entry.isSymbolicLink()) throw new Error(`Frontend input is not a regular file: ${entry.name}`)
      files.push(entry.name)
    }
  }
  const lock = JSON.parse(await readFile(join(root, "package-lock.json"), "utf8"))
  const imports = new Map()
  function add(name, importer) {
    const packagePath = `node_modules/${name}`
    const pkg = lock.packages[packagePath]
    if (!pkg) throw new Error(`Frontend package has no locked source: ${name} (${importer})`)
    if (!imports.has(name)) imports.set(name, { name, packagePath, version: pkg.version, integrity: pkg.integrity, importers: [] })
    imports.get(name).importers.push(importer)
  }
  for (const path of files) {
    if (!path.startsWith("src/") || /(?:\.(?:test|spec)\.|\.d\.ts$|\/__tests__\/)/.test(path)) continue
    if (!/\.(?:[cm]?[jt]sx?|css)$/.test(path)) continue
    for (const name of importedPackages(await readFile(join(root, path), "utf8"), path.endsWith(".css"))) add(name, path)
  }
  for (const entry of manifest.generatedAssets) {
    if (!entry.package || !entry.reason || !entry.inputs?.length || entry.inputs.some(path => !files.includes(path))) {
      throw new Error("Generated asset provenance must identify its package, reason and existing inputs")
    }
    for (const path of entry.inputs) add(entry.package, path)
  }
  for (const component of manifest.vendoredComponents) {
    if (!component.id || !component.license || !component.upstream || !component.files?.length || !component.noticeFiles?.length) {
      throw new Error("Incomplete copied-code provenance")
    }
    for (const path of component.files) {
      if (!files.includes(path)) throw new Error(`Copied source is missing: ${path}`)
    }
    for (const directory of component.watchedDirectories ?? []) {
      const unregistered = files.filter(path => path.startsWith(`${directory}/`) && /\.[jt]sx?$/.test(path) && !component.files.includes(path))
      if (unregistered.length) throw new Error(`Copied code needs attribution review: ${unregistered.join(", ")}`)
    }
    files.push(...component.noticeFiles)
  }
  return {
    schemaVersion: 1,
    files: [...new Set([...files, "compliance/frontend.json"])].sort(),
    npmImports: [...imports.values()].map(item => ({ ...item, importers: [...new Set(item.importers)].sort() })).sort((a, b) => a.name.localeCompare(b.name, "en")),
    vendoredComponents: manifest.vendoredComponents,
  }
}

export async function checkFrontend(root, report, checkedFile) {
  const inventory = await frontendInventory(root)
  const inputs = new Set(report.inputs.map(item => item.path))
  for (const path of inventory.files) {
    await checkedFile(root, path)
    if (!inputs.has(path)) throw new Error(`Frontend source or asset needs compliance review: ${path}`)
  }
  const evidence = JSON.parse(await readFile(join(root, "compliance/evidence/frontend-dependencies.json"), "utf8"))
  if (JSON.stringify(evidence) !== JSON.stringify(inventory)) throw new Error("Frontend provenance changed; regenerate dependency notices and review copied code/assets")
  const records = JSON.parse(await readFile(join(root, "compliance/evidence/application-dependencies.json"), "utf8"))
  const notices = normalize(await readFile(join(root, "src-tauri/resources/APPLICATION_LICENSES.txt"), "utf8"))
  for (const component of inventory.vendoredComponents) {
    const record = records.find(item => item.ecosystem === "vendored" && item.name === component.id)
    if (!record || record.license !== component.license) throw new Error(`Missing copied-code notice record: ${component.id}`)
    for (const path of component.noticeFiles) {
      const contents = normalize(await readFile(await checkedFile(root, path), "utf8"))
      if (!contents.trim() || !notices.includes(contents) || !record.texts.some(item => item.path === path && item.sha256 === sha256(contents))) {
        throw new Error(`Missing copied-code license text: ${path}`)
      }
    }
  }
  for (const pkg of inventory.npmImports) {
    const record = records.find(item => item.ecosystem === "npm" && item.name === pkg.name && item.version === pkg.version && item.integrity === pkg.integrity)
    if (!record?.texts?.length) throw new Error(`Missing shipped npm license notices: ${pkg.name} ${pkg.version}`)
    // Re-check installed notice bytes where available, including dev packages
    // such as Tailwind whose CSS becomes part of the production application.
    for (const item of record.texts.filter(item => item.installedPath?.startsWith(`${pkg.packagePath}/`))) {
      const contents = normalize(await readFile(await checkedFile(root, item.installedPath), "utf8"))
      if (!notices.includes(contents)) throw new Error(`Missing shipped npm license text: ${item.installedPath}`)
    }
  }
  return inventory
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await frontendInventory(resolve(process.argv[2] ?? "."))))
}
