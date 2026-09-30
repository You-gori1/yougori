import { build } from "vite"
import { readFile, writeFile } from "node:fs/promises"
import { fileURLToPath } from "node:url"
import { resolve } from "node:path"

const root = fileURLToPath(new URL("../../", import.meta.url))
const records = JSON.parse(await readFile(resolve(root, "compliance/evidence/application-dependencies.json"), "utf8"))
const packages = new Map()
const chunks = []
await build({
  root,
  build: { outDir: resolve(root, "build/compliance/audit-2026-09-14/frontend"), emptyOutDir: false },
  plugins: [{
    name: "read-only-license-audit",
    generateBundle(_options, bundle) {
      for (const [filename, output] of Object.entries(bundle)) {
        if (output.type !== "chunk") continue
        chunks.push({ filename, moduleCount: Object.keys(output.modules).length })
        for (const [id, details] of Object.entries(output.modules)) {
          const normalized = id.replaceAll("\\", "/").replaceAll("\0", "")
          const match = normalized.match(/^(.*\/node_modules\/)((?:@[^/]+\/)?[^/]+)\//)
          if (match && details.renderedLength > 0) {
            const packageRoot = match[1] + match[2]
            if (!packages.has(packageRoot)) packages.set(packageRoot, { packageRoot, moduleCount: 0, renderedBytes: 0 })
            packages.get(packageRoot).moduleCount++
            packages.get(packageRoot).renderedBytes += details.renderedLength
          }
        }
      }
    },
  }],
})
const results = []
for (const pkg of packages.values()) {
  const metadata = JSON.parse(await readFile(resolve(pkg.packageRoot, "package.json"), "utf8"))
  const notice = records.find(x => x.ecosystem === "npm" && x.name === metadata.name && x.version === metadata.version)
  results.push({ name: metadata.name, version: metadata.version, license: metadata.license,
    moduleCount: pkg.moduleCount, renderedBytes: pkg.renderedBytes, noticeRecorded: Boolean(notice?.texts?.length) })
}
const result = { packages: results.sort((a, b) => a.name.localeCompare(b.name)), chunks,
  missingNotices: results.filter(x => !x.noticeRecorded),
  note: "Production Vite build modules with nonzero emitted code; Tailwind CSS checked separately by frontend inventory." }
await writeFile(new URL("frontend-build.json", import.meta.url), JSON.stringify(result, null, 2) + "\n")
console.log(JSON.stringify({ includedPackages: results.length, missingNotices: result.missingNotices }))
